#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg(target_os = "none")]
macro_rules! stock {
    ($addr:expr, fn($($t:ty),*) $(-> $r:ty)?) => {
        unsafe { core::mem::transmute::<usize, extern "C" fn($($t),*) $(-> $r)?>($addr as usize) }
    };
}

#[cfg(target_os = "none")]
mod app;
#[cfg(target_os = "none")]
mod gps;
#[cfg(target_os = "none")]
mod hr;
#[cfg(target_os = "none")]
mod lv;
#[cfg(target_os = "none")]
mod media;
#[cfg(target_os = "none")]
mod music;
mod menu;
#[cfg(target_os = "none")]
mod motor;
#[cfg(target_os = "none")]
mod storage;
mod svc;
#[cfg(target_os = "none")]
mod ui;

#[cfg(target_os = "none")]
mod view {
    use crate::{app, menu, motor, svc, ui};
    use core::ffi::c_void;
    use core::ptr::addr_of_mut;
    use core::sync::atomic::Ordering;
    use wp2_core::events::{escape_due, map_key, Event, BUTTON, CROWN, TOUCH_DOWN, TOUCH_UP};
    use wp2_core::pkg::{F_PAGE, F_WIDGET};
    use wp2_core::stock::KEY_BACK;

    const PRELOAD: u32 = 2;
    const LAYOUT: u32 = 3;
    const DELETE: u32 = 4;
    const FOCUS: u32 = 7;
    const DEFOCUS: u32 = 8;
    const KEY: u32 = 0xD;
    // the launcher's early-suspend / late-resume to the top view (docs/recon.md, "Screen-off and the app view")
    const SUSPEND: u32 = 0x82;
    const WAKE: u32 = 0x83;
    // a key this soon after WAKE is the one that woke the panel (AOD exit passes it on to the view)
    const WAKE_KEY_MS: u32 = 1500;
    const ERROR_MS: u32 = 2000;

    struct Menu {
        entries: [Option<menu::Entry>; menu::ROWS],
        // row of the running (or last) app
        launched: usize,
        failed: Option<usize>,
        failed_at: u32,
        // millis() | 1 of a BUTTON for the running app, 0 if none
        button_at: u32,
        // the confirm screen's answer, done by the timer: ASK_CLOSE, or ASK_DELETE for row `asked`
        answer: u8,
        asked: usize,
        scr: *mut c_void,
        asleep: bool,
        // millis() | 1 of the last WAKE
        woke_at: u32,
        // Some(slot): this view is that app's page next to the face (view 0xF8 + slot, wp2-core cards.rs)
        page: Option<u8>,
    }
    const ASK_CLOSE: u8 = 1;
    const ASK_DELETE: u8 = 2;
    const ASK_WIDGET: u8 = 3;
    const ASK_PAGE: u8 = 4;

    // Lives in the loader block: zero again on every open of the view.
    static mut MENU: Menu = Menu {
        entries: [None; menu::ROWS],
        launched: 0,
        failed: None,
        failed_at: 0,
        button_at: 0,
        answer: 0,
        asked: 0,
        scr: core::ptr::null_mut(),
        asleep: false,
        woke_at: 0,
        page: None,
    };

    fn st() -> &'static mut Menu {
        unsafe { &mut *addr_of_mut!(MENU) }
    }

    #[unsafe(no_mangle)]
    #[unsafe(link_section = ".entry")]
    pub extern "C" fn loader_main(view_id: u32, msg: u32, data: *mut c_void) -> i32 {
        match msg {
            PRELOAD => ui::preload(view_id),
            LAYOUT => {
                let scr = ui::layout(data, input_cb, tick);
                let m = st();
                m.scr = scr;
                m.entries = menu::scan();
                menu::lv::build(scr, &m.entries, row_cb, long_cb);
                m.page = wp2_core::cards::page_slot(view_id);
                app::ON_PAGE.store(m.page.is_some(), Ordering::Release);
                match m.page {
                    Some(slot) => launch_page(m, slot),
                    None => launch_widget(m),
                }
                0
            }
            FOCUS | DEFOCUS => {
                let m = st();
                // a DEFOCUS after our SUSPEND is the AOD overlay, not a view over the app: no pause
                app::DEFOCUSED.store(msg == DEFOCUS && !m.asleep, Ordering::Release);
                // an app that ended on its page starts again when the page comes back
                if let (FOCUS, Some(slot), false) = (msg, m.page, app::running()) {
                    launch_page(m, slot);
                }
                0
            }
            SUSPEND | WAKE => {
                let m = st();
                m.asleep = msg == SUSPEND;
                m.woke_at = if msg == WAKE { app::millis() | 1 } else { 0 };
                0
            }
            KEY => {
                key(data);
                0
            }
            DELETE => {
                app::stop_app();
                hold_face(false);
                menu::lv::delete();
                ui::delete();
                0
            }
            _ => 0,
        }
    }

    fn show_app(on: bool) {
        ui::show(on);
        menu::lv::show(!on);
    }

    fn fail(row: usize, code: i32) {
        let m = st();
        if let Some(prev) = m.failed.replace(row) {
            restore(prev);
        }
        m.failed_at = app::millis();
        menu::lv::set_row_text(row, &menu::error_text(code));
    }

    fn restore(row: usize) {
        if let Some(e) = st().entries[row] {
            menu::lv::set_row_text(row, &menu::name_text(&e));
        }
    }

    // data: u32 key code, byte +4 = handled. In the menu the crown scrolls through LVGL, the button (to the
    // clock) and the back swipe (view stack pop to the app list) stay with the stock. In an app both are BUTTON.
    fn key(data: *mut c_void) {
        let code = unsafe { *(data as *const u32) };
        let Some(e) = map_key(code) else { return };
        let m = st();
        // on a page the keys stay with the stock, as on any card: the app has no "back"
        if !app::running() || m.page.is_some() {
            return;
        }
        if m.asleep || (m.woke_at != 0 && app::millis().wrapping_sub(m.woke_at) < WAKE_KEY_MS) {
            m.asleep = false; // one key at most: a WAKE that never comes must not eat the button for good
            unsafe { *(data as *mut u8).add(4) = 1 };
            return;
        }
        // a horizontal pager pages on a right swipe: the stock skips back for its workout screens (docs/recon.md),
        // so the swipe is no BUTTON while one lives; nor while the music tile's volume popup is open (the swipe closes
        // it). The crown press still is (and still escapes).
        if code & 0xFFFF == KEY_BACK && (crate::lv::hpager() || crate::music::popup()) {
            unsafe { *(data as *mut u8).add(4) = 1 };
            return;
        }
        // an app that owns "back" and read the last BUTTON gets a fresh escape window per press (ABI 1.3)
        let owned_read = app::OWN_BACK.load(Ordering::Acquire) && app::CONSUMED.load(Ordering::Acquire);
        let arm = e.kind == BUTTON && (m.button_at == 0 || owned_read);
        // armed only once the BUTTON is queued: a full queue drops it, the app never sees it. The app thread (prio
        // 14) cannot run between the push and the store on this thread.
        if to_app(e) && arm {
            app::CONSUMED.store(false, Ordering::Release);
            m.button_at = app::millis() | 1;
        }
        unsafe { *(data as *mut u8).add(4) = 1 };
    }

    // dropped while paused or on a full queue
    fn to_app(e: Event) -> bool {
        app::running() && !app::paused() && app::QUEUE.push(e)
    }

    // the image or the LVGL root (ABI 1.2): only shown while an app runs
    extern "C" fn input_cb(e: *mut c_void) {
        let code = ui::event_code(e);
        if code == ui::KNOB {
            let step = ui::knob_step(e).signum() as i16;
            if step != 0 {
                to_app(Event { kind: CROWN, delta: step, ..Event::default() });
            }
            return;
        }
        if code != ui::PRESSED && code != ui::RELEASED {
            return;
        }
        let (x, y) = ui::touch_point(e);
        let kind = if code == ui::PRESSED { TOUCH_DOWN } else { TOUCH_UP };
        let c = |v: i32| v.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
        to_app(Event { kind, x: c(x), y: c(y), ..Event::default() });
    }

    // CLICKED on a menu row (LVGL sends none after a scroll drag)
    extern "C" fn row_cb(e: *mut c_void) {
        let row = menu::lv::row_of(e);
        let m = st();
        if app::running() || menu::lv::confirm_open() {
            return;
        }
        let Some(entry) = m.entries.get(row).copied().flatten() else { return };
        m.launched = row;
        match app::launch(entry.slot) {
            Ok(()) => show_app(true),
            Err(code) => fail(row, code),
        }
    }

    // a tap on an app widget (stub card.rs) opened the view: start that slot as if its row was tapped; an
    // uninstalled slot leaves the menu
    fn launch_widget(m: &mut Menu) {
        let want = core::mem::take(&mut wp2_core::layout::stub_ram().launch);
        let Some(row) = m.entries.iter().position(|e| e.is_some_and(|e| e.slot + 1 == want)) else { return };
        m.launched = row;
        match app::launch(want - 1) {
            Ok(()) => show_app(true),
            Err(code) => fail(row, code),
        }
    }

    // the app's page (view 0xF8 + slot): start it at once, the stock pauses it (DEFOCUS) while the page is away. An
    // app that did not declare a page, or no app, leaves the menu
    fn launch_page(m: &mut Menu, slot: u8) {
        let Some(row) = m.entries.iter().position(|e| e.is_some_and(|e| e.slot == slot && e.features & F_PAGE != 0))
        else {
            return;
        };
        m.launched = row;
        match app::launch(slot) {
            Ok(()) => show_app(true),
            Err(code) => fail(row, code),
        }
    }

    // LONG_PRESSED on a menu row: what the app declared (widget, page) on or off, delete, cancel
    extern "C" fn long_cb(e: *mut c_void) {
        let row = menu::lv::row_of(e);
        if app::running() {
            return;
        }
        let m = st();
        let Some(entry) = m.entries.get(row).copied().flatten() else { return };
        let cards = cards_now();
        let mut b: [(&[u8], bool, usize); 4] = [(b"", false, 0); 4];
        let mut n = 0;
        let at = |ask: u8| usize::from(ask) << 4 | row;
        if entry.features & F_WIDGET != 0 {
            let on = wp2_core::cards::has_widget(&cards, entry.slot);
            b[n] = (if on { b"Remove widget" } else { b"Add widget" }, false, at(ASK_WIDGET));
            n += 1;
        }
        if entry.features & F_PAGE != 0 {
            let on = wp2_core::cards::has_page(&cards, entry.slot);
            b[n] = (if on { b"Remove page" } else { b"Add page" }, false, at(ASK_PAGE));
            n += 1;
        }
        b[n] = (b"Delete", true, at(ASK_DELETE));
        b[n + 1] = (b"Cancel", false, at(ASK_CLOSE));
        menu::lv::confirm(m.scr, &menu::name_text(&entry), &b[..n + 2], sheet_cb);
    }

    extern "C" fn sheet_cb(e: *mut c_void) {
        let m = st();
        let data = menu::lv::row_of(e);
        (m.answer, m.asked) = ((data >> 4) as u8, data & 0xF);
    }

    fn cards_now() -> wp2_core::cards::Cards {
        unsafe { (wp2_core::stock::CARDS_STORE as *const wp2_core::cards::Cards).read_volatile() }
    }

    // as the phone's 0x906D set: the settings copy, then store + lay out the cards again
    fn edit_cards(f: impl FnOnce(&mut wp2_core::cards::Cards)) {
        let mut c = cards_now();
        f(&mut c);
        unsafe { (wp2_core::stock::CARDS_STORE as *mut wp2_core::cards::Cards).write_volatile(c) };
        stock!(wp2_core::stock::CARDS_SAVE, fn(u32, u32) -> u32)(0xE4, 0);
    }

    // timer: the sheet's answer. Widget / page = the card list; delete = the stub's DELETE (package and data files)
    // and the app's cards, then the list again.
    fn answer(m: &mut Menu) {
        let answer = core::mem::take(&mut m.answer);
        if answer == 0 {
            return;
        }
        menu::lv::close_confirm();
        let Some(entry) = m.entries.get(m.asked).copied().flatten() else { return };
        use wp2_core::{cards, fs, layout};
        match answer {
            // ponytail: a full list (8 cards) just leaves it as it is
            ASK_WIDGET => return edit_cards(|c| drop(cards::toggle_widget(c, entry.slot))),
            ASK_PAGE => return edit_cards(|c| drop(cards::toggle_page(c, entry.slot))),
            ASK_DELETE => {}
            _ => return,
        }
        let c = cards_now();
        if cards::has_widget(&c, entry.slot) || cards::has_page(&c, entry.slot) {
            edit_cards(|c| cards::forget(c, entry.slot));
        }
        fs::unlink(&layout::slot_path(entry.slot, false));
        fs::unlink(&layout::data_path(entry.slot, false));
        fs::unlink(&layout::data_path(entry.slot, true));
        fs::unlink(&layout::widget_path(entry.slot));
        fs::unlink(&layout::blob_path(entry.slot));
        m.failed = None;
        menu::lv::delete();
        m.entries = menu::scan();
        menu::lv::build(m.scr, &m.entries, row_cb, long_cb);
    }

    extern "C" fn tick(_timer: *mut c_void) {
        crate::lv::tick();
        let m = st();
        app::pause_tick();
        motor::watchdog();
        svc::tick(app::running(), app::paused());
        crate::hr::tick(app::running(), app::paused());
        crate::gps::tick(app::running(), app::paused());
        crate::media::tick(app::running(), app::paused());
        crate::music::tick();
        hold_face(app::running() && m.page.is_none());
        if app::running() {
            let exited = app::exited();
            let owned = app::OWN_BACK.load(Ordering::Acquire);
            let consumed = app::CONSUMED.load(Ordering::Acquire);
            let polled = app::POLLED_AT.load(Ordering::Relaxed);
            let escape = escape_due(owned, m.button_at, consumed, polled, app::millis());
            if exited.is_some() || escape {
                app::stop_app();
                show_app(false);
                if let Some(code) = exited.filter(|&c| c != 0) {
                    fail(m.launched, code);
                }
            }
        }
        if !app::running() {
            m.button_at = 0;
            answer(m);
        }
        if let Some(row) = m.failed.filter(|_| app::millis().wrapping_sub(m.failed_at) >= ERROR_MS) {
            m.failed = None;
            restore(row);
        }
        ui::flush(ui::PUBLISHED.swap(wp2_core::gfx::EMPTY, Ordering::Acquire));
    }

    // The stock exempts its own workout views from "return to watch face"; an app gets the same while it runs. Before
    // heart rate, deep sleep stopped the launcher's clock and hid the return (docs/recon.md, "Return to watch face").
    fn hold_face(on: bool) {
        use wp2_core::stock::{RETURN_CB, RETURN_CLOCK};
        unsafe { (RETURN_CB as *mut u32).write_volatile(if on { 0 } else { RETURN_CLOCK }) };
    }

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo) -> ! {
        // a fault reboots the watch: a hung ui_service would also stop the motor watchdog
        unsafe { core::arch::asm!("udf #0", options(noreturn)) }
    }
}
