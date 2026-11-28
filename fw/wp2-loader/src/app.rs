use crate::{gps, hr, lv, media, motor, svc, ui};
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use wp2_core::gfx;
use wp2_core::events::{popup_covers, Api, Event, Queue, BUTTON, PAUSE, RESUME};
use wp2_core::heap::{self, RES};
use wp2_core::layout::stub_ram;
use wp2_core::load;
use wp2_core::stock::{K_THREAD_ABORT, OS_SLEEP, OS_THREAD_CREATE, SYS_CLOCK_TICK_GET};

// bits 0/1: a stock popup is open, ids at +2 / +4 (set_popup_msgbox / set_close_msgbox, docs/recon.md)
const POPUP_FLAGS: usize = 0x3800_FAF4;

pub const NONE: u8 = 0xFF;
// lowest preemptible priority (idle is 15): every stock thread but standby, actlog and shell_uart (also 14)
// runs first, so a busy app cannot stall BLE, sensors or the UI (docs/recon.md)
const APP_PRIO: i32 = 14;
const TEXT_MAX: usize = 64;

#[repr(C, align(8))]
struct ThreadBuf([u8; 0x118 + 0x2000]);

// k_thread (0x118) + stack; in the loader's .bss, so inside the loader block
static mut THREAD_BUF: ThreadBuf = ThreadBuf([0; 0x118 + 0x2000]);
pub static QUEUE: Queue = Queue::new();
static EXITED: AtomicBool = AtomicBool::new(false);
static EXIT_CODE: AtomicI32 = AtomicI32::new(0);
// view msg 8 (defocus) until msg 7 (focus)
pub static DEFOCUSED: AtomicBool = AtomicBool::new(false);
// the view is an app's page next to the face (lv_submit PAGE query, wp2-core lvcmd::page_query)
pub static ON_PAGE: AtomicBool = AtomicBool::new(false);
// ABI 1.3: the app handles BUTTON itself (own_back); CONSUMED: it popped the last BUTTON
pub static OWN_BACK: AtomicBool = AtomicBool::new(false);
pub static CONSUMED: AtomicBool = AtomicBool::new(false);
// millis() of the app's last next_event call: a busy app with own_back is not hung (events::escape_due)
pub static POLLED_AT: AtomicU32 = AtomicU32::new(0);

static API: Api = Api {
    abi: 1,
    abi_minor: 3,
    width: 466,
    height: 466,
    fill_rect,
    draw_text,
    present,
    next_event,
    motor: motor::motor,
    millis,
    exit,
    send_msg: svc::send,
    recv_msg: svc::recv,
    load_data: svc::load,
    save_data: svc::save,
    get_time: svc::time,
    get_battery: svc::battery,
    screen_on: svc::keep_screen_on,
    lv_submit: lv::submit,
    own_back,
};

type Entry = extern "C" fn(*const Api) -> i32;
type ThreadFn = extern "C" fn(u32, *const Api, u32);

pub fn running() -> bool {
    stub_ram().running_slot != NONE
}

// the app's exit code once it returned or called exit
pub fn exited() -> Option<i32> {
    EXITED.load(Ordering::Acquire).then(|| EXIT_CODE.load(Ordering::Relaxed))
}

pub fn launch(slot: u8) -> Result<(), i32> {
    let mut file = [0u8; 0x2C];
    let mut scratch = [0u8; 0x200];
    let (block, h) = load::load(slot, &mut file, &mut scratch)?;
    let ram = stub_ram();
    ram.app_block = block as u32;
    ram.app_size = h.image_size;
    EXITED.store(false, Ordering::Release);
    OWN_BACK.store(false, Ordering::Relaxed);
    CONSUMED.store(false, Ordering::Relaxed);
    POLLED_AT.store(millis(), Ordering::Relaxed);
    motor::reset();
    QUEUE.clear();
    crate::status::SHOWN.store(true, Ordering::Relaxed);
    // the slot's named files are this app's (another app's that were there go)
    crate::owner::adopt(slot, &h.name);
    svc::start(slot, h.abi_minor);
    ui::canvas().fill_rect(0, 0, 466, 466, 0);
    ui::PENDING.store(gfx::EMPTY, Ordering::Relaxed);
    ui::PUBLISHED.store(gfx::FULL, Ordering::Release);
    let buf = unsafe { &mut *addr_of_mut!(THREAD_BUF) };
    let entry = (block as u32 + h.entry) | 1;
    // os_thread_create(buf, size, entry, p1, p2, p3, prio, options, delay_ms)
    // the app (prio 14) does not run before this UI-thread pass ends, so running_slot is set after the create
    let tid = stock!(OS_THREAD_CREATE, fn(*mut u8, u32, ThreadFn, u32, *const Api, u32, i32, u32, i32) -> u32)(
        buf.0.as_mut_ptr(), buf.0.len() as u32, app_thread, entry, &API, 0, APP_PRIO, 0, 0,
    );
    if tid == 0 {
        svc::stop();
        unsafe { heap::free(RES, block) };
        ram.app_block = 0;
        ram.app_size = 0;
        return Err(-1);
    }
    ram.running_slot = slot;
    Ok(())
}

// UI thread only. The block is freed after the abort: the app thread never runs again.
pub fn stop_app() {
    if !running() {
        return;
    }
    // abort first: the app thread cannot start the motor again
    stock!(K_THREAD_ABORT, fn(*mut u8))(addr_of_mut!(THREAD_BUF) as *mut u8);
    motor::off();
    hr::stop();
    gps::stop();
    media::stop();
    crate::audio::stop();
    crate::mic::stop();
    svc::stop();
    // LVGL keeps no object, callback or font of the app once its block is gone
    crate::storage::stop();
    lv::teardown();
    crate::menu::lv::clear_app_icons();
    let ram = stub_ram();
    unsafe { heap::free(RES, ram.app_block as *mut u8) };
    ram.app_block = 0;
    ram.app_size = 0;
    ram.running_slot = NONE;
    motor::reset();
}

fn overlay() -> bool {
    let (flags, ids) = unsafe {
        let p = POPUP_FLAGS as *const u8;
        (p.read_volatile(), [(p.add(2) as *const u16).read_volatile(), (p.add(4) as *const u16).read_volatile()])
    };
    DEFOCUSED.load(Ordering::Acquire) || popup_covers(flags, ids)
}

// UI tick: a stock popup over a running app pauses it (PAUSE, app's motor off), its close resumes it (RESUME, redraw).
// The AOD overlay is a stock msgbox too, but no pause: the app keeps running and measuring in AOD.
pub fn pause_tick() {
    let paused = running() && overlay();
    if paused == motor::PAUSED.swap(paused, Ordering::AcqRel) {
        return;
    }
    svc::set_paused(paused);
    if paused {
        // no motor::off() here: the popup may already buzz the motor itself (charge popup: 300 ms);
        // the watchdog right after this cuts the motor only if the app left it on
        QUEUE.clear();
        QUEUE.push(Event { kind: PAUSE, ..Event::default() });
    } else {
        QUEUE.push(Event { kind: RESUME, ..Event::default() });
        ui::invalidate();
    }
}

pub fn paused() -> bool {
    motor::PAUSED.load(Ordering::Acquire)
}

extern "C" fn app_thread(entry: u32, api: *const Api, _: u32) {
    let entry: Entry = unsafe { core::mem::transmute(entry as usize) };
    let code = entry(api);
    park(code)
}

fn park(code: i32) -> ! {
    EXIT_CODE.store(code, Ordering::Relaxed);
    EXITED.store(true, Ordering::Release);
    loop {
        sleep(1000);
    }
}

fn sleep(ms: u32) {
    stock!(OS_SLEEP, fn(u32))(ms)
}

extern "C" fn fill_rect(x: i16, y: i16, w: u16, h: u16, c: u16) {
    ui::canvas().fill_rect(x, y, w, h, c);
    ui::mark(gfx::rect_box(x, y, w, h));
}

/// BLIT record (docs/bitmap-assets.md): `w * h` app-owned RGB565 pixels into the framebuffer, clipped like
/// `fill_rect`. The pixels are range-checked against the app block in `lv::submit` before this runs.
pub fn blit(px: *const u16, x: i16, y: i16, w: u16, h: u16) -> i32 {
    let src = unsafe { core::slice::from_raw_parts(px, usize::from(w) * usize::from(h)) };
    if ui::canvas().blit(x, y, w, h, src) {
        ui::mark(gfx::rect_box(x, y, w, h));
    }
    0
}

extern "C" fn draw_text(x: i16, y: i16, text: *const u8, len: usize, c: u16) {
    if text.is_null() {
        return;
    }
    let s = unsafe { core::slice::from_raw_parts(text, len.min(TEXT_MAX)) };
    let end = ui::canvas().text(x, y, s, c);
    ui::mark(gfx::rect_box(x, y, end.wrapping_sub(x) as u16, 16));
}

// publishes the dirty box (Release: after the pixels); paused, it stays pending and RESUME redraws everything
extern "C" fn present() {
    ui::count_present();
    if !paused() {
        let b = ui::PENDING.swap(gfx::EMPTY, Ordering::Relaxed);
        let _ = ui::PUBLISHED.fetch_update(Ordering::Release, Ordering::Relaxed, |p| Some(gfx::merge(p, b)));
    }
}

extern "C" fn next_event(ev: *mut Event, timeout_ms: u32) -> bool {
    let start = millis();
    loop {
        POLLED_AT.store(millis(), Ordering::Relaxed);
        // straight to the app, not through QUEUE: the UI thread is its only producer
        if let Some(e) = hr::due().or_else(gps::due) {
            if !ev.is_null() {
                unsafe { ev.write(e) };
            }
            return true;
        }
        if let Some(e) = QUEUE.pop() {
            if e.kind == BUTTON {
                CONSUMED.store(true, Ordering::Release);
            }
            if !ev.is_null() {
                unsafe { ev.write(e) };
            }
            return true;
        }
        if let Some(e) = svc::announce() {
            if !ev.is_null() {
                unsafe { ev.write(e) };
            }
            return true;
        }
        if timeout_ms != u32::MAX && millis().wrapping_sub(start) >= timeout_ms {
            return false;
        }
        sleep(5);
    }
}

extern "C" fn own_back(on: bool) {
    OWN_BACK.store(on, Ordering::Release);
}

pub extern "C" fn millis() -> u32 {
    stock!(SYS_CLOCK_TICK_GET, fn() -> u64)() as u32
}

extern "C" fn exit(code: i32) -> ! {
    park(code)
}
