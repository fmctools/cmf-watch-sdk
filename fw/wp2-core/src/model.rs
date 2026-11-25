//! Per-model stock facts for the stub and loader: every stock address, struct offset, ux2sys property id, BLE
//! opcode and view id they depend on, for the CMF Watch Pro 2 (JX402 1.0.0.73) and the Watch 3 Pro (jx402_08
//! 1.0.1.77). Shaped like `wfguard::Device`: generic code takes `&'static Model` and LTO folds the constants.
//! Nothing uses this table yet (milestone M1 of the Watch 3 Pro loader port); `stock.rs` stays the WP2 source.
//!
//! Rules:
//! - WP2 values reference the existing constants (`stock.rs`, `layout.rs`, `heap.rs`, `cards.rs`, `proto.rs`,
//!   `events.rs`); values that only exist as private constants of `wp2-stub`/`wp2-loader` or in
//!   `src/wp2apps/firmware.py` are retyped with a `mirrors` note (tests/test_model_stock.py checks the firmware.py
//!   ones against `model.json`).
//! - Every WP3 value has a `// source:` note: hunt = jimhooker cmf-watch3-port-notes `data/hunt-found.csv`, map =
//!   its `data/address-map.csv` (located rows only), kit = its other notes, own = own disassembly of the stock
//!   app.bin, upstream = already used by `wfguard::WP3` or `firmware.py`, plan = a design choice of the port plan
//!   (not a stock fact). Anything not re-derived for a build is `None`, never a guess.
//! - Functions carry the Thumb bit; code sites, literal slots and data do not.
//!
//! `model.json` next to `Cargo.toml` is this table exported for Python (`tests/test_model_stock.py` checks it
//! against stock images). It is generated: `UPDATE_MODEL_JSON=1 cargo test -p wp2-core model` rewrites it, and the
//! `json_is_current` test fails when it is stale.

use crate::{cards, events, heap, layout, proto, stock};

/// What a field holds; decides the checks the host tests and the stock-image verifier run on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// a string
    Text,
    /// a byte count
    Size,
    /// a plain number (register, count, heap size, message or key code)
    Val,
    /// a struct field offset
    Off,
    /// a view, msgbox, wake-lock or widget id
    Id,
    /// a BLE command opcode
    Op,
    /// a ux2sys property id
    Prop,
    /// a function in app.bin (Thumb bit set)
    Func,
    /// a code site or literal slot in app.bin that a patch rewrites (no Thumb bit)
    Site,
    /// data inside app.bin (tables, strings)
    Rom,
    /// RAM (or an MMIO-free RAM-mapped block): checked as a literal in app.bin
    Ram,
}

/// A field's value, model-independent, for the tests and the JSON export.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Val {
    Num(u32),
    /// not re-derived for this build
    Unknown,
    List(&'static [u32]),
    Text(&'static str),
}

pub trait Cell {
    fn val(&self) -> Val;
}

impl Cell for u32 {
    fn val(&self) -> Val {
        Val::Num(*self)
    }
}

impl Cell for Option<u32> {
    fn val(&self) -> Val {
        self.map_or(Val::Unknown, Val::Num)
    }
}

impl Cell for &'static [u32] {
    fn val(&self) -> Val {
        Val::List(self)
    }
}

impl Cell for &'static str {
    fn val(&self) -> Val {
        Val::Text(self)
    }
}

macro_rules! model {
    ($( $(#[$meta:meta])* $name:ident : $ty:ty = $kind:ident, )*) => {
        /// Stock facts of one firmware build (module docs).
        pub struct Model {
            $( $(#[$meta])* pub $name: $ty, )*
        }

        impl Model {
            /// Every field in declaration order: (name, kind, value).
            pub fn fields(&self) -> impl Iterator<Item = (&'static str, Kind, Val)> {
                [$( (stringify!($name), Kind::$kind, Cell::val(&self.$name)), )*].into_iter()
            }
        }
    };
}

model! {
    // --- identity and image (PLAN.md 1.1)
    name: &'static str = Text,
    /// first 16 hex digits of the stock app.bin sha256
    stock_sha256_prefix: &'static str = Text,
    /// length of the stock app.bin = offset of the stub
    app_size: u32 = Size,

    // --- boot hook and heap (1.1, 1.4)
    /// `bl stock_heap_init` (ResFbHeap init) that the stub's boot trampoline replaces
    boot_call: u32 = Site,
    stock_heap_init: u32 = Func,
    /// the three literal slots holding the ResFbHeap size; the stub takes its top 4 KiB
    heap_literals: &'static [u32] = Site,
    /// stock ResFbHeap size (the value in `heap_literals`)
    heap_size: u32 = Val,
    heap_base: u32 = Ram,
    /// the ResFbHeap descriptor
    heap: u32 = Ram,
    /// u32 free bytes in the descriptor
    heap_free: u32 = Off,
    /// first free-list node in the descriptor (node size at +4, & 0xFF_FFFF)
    heap_free_list: u32 = Off,
    /// StubRam = heap_base + heap_size - STUB_RAM_SIZE
    stub_ram: u32 = Ram,
    ui_mem_aligned_alloc: u32 = Func,
    ui_mem_free: u32 = Func,
    psram_cache_flush_invalidate: u32 = Func,
    os_thread_create: u32 = Func,
    os_sleep: u32 = Func,
    k_thread_abort: u32 = Func,
    sys_clock_tick_get: u32 = Func,
    /// loader app thread priority
    app_thread_prio: Option<u32> = Val,
    /// u8 popup flags, u16 ids at +2/+4 (`events::popup_covers`)
    popup_flags: u32 = Ram,
    /// msgbox id of the AOD overlay (no pause)
    aod_msgbox: Option<u32> = Id,

    // --- view table and view ids (1.1, 2.3, 2.4)
    view_table: u32 = Rom,
    view_table_end: u32 = Rom,
    /// bytes per view-table record
    view_record: u32 = Size,
    /// record word the stub puts its view proc in (stock uses word 1 or word 2; word 1 needs word 2 = 0)
    view_rec_proc: u32 = Off,
    /// u16 view id in a record (then u16 0x102, u16 width 466, u16 height 466)
    view_rec_id: u32 = Off,
    /// literal slots holding `view_table` / `view_table_end` that the stub repoints at its extended copy
    view_start_literals: &'static [u32] = Site,
    view_end_literals: &'static [u32] = Site,
    view_open: u32 = Func,
    /// posts "layout" for a view id
    view_layout: u32 = Func,
    /// highest view id in the stock table: the stub's ids go above it
    stock_view_id_max: u32 = Id,
    /// the loader's view (also its wake-lock id)
    menu_view: u32 = Id,
    /// app page views are page_view_base + slot (layout::SLOTS of them)
    page_view_base: u32 = Id,
    view_msg_preload: Option<u32> = Val,
    view_msg_layout: Option<u32> = Val,
    view_msg_delete: Option<u32> = Val,
    view_msg_focus: Option<u32> = Val,
    view_msg_defocus: Option<u32> = Val,
    view_msg_key: Option<u32> = Val,
    view_msg_suspend: Option<u32> = Val,
    view_msg_wake: Option<u32> = Val,
    key_crown_press: Option<u32> = Val,
    key_crown_cw: Option<u32> = Val,
    key_crown_ccw: Option<u32> = Val,
    key_back: Option<u32> = Val,

    // --- BLE channel (1.2, 2.1)
    /// `bl read_payload` in the dispatcher's default branch, which the stub hooks
    ble_call: u32 = Site,
    read_payload: u32 = Func,
    /// register holding the frame pointer at `ble_call` (the trampoline's `mov r2, rN`)
    ble_frame_reg: u32 = Val,
    /// the dispatcher's default branch (first instruction; `ble_call` follows a frame load and a literal load)
    ble_dispatch_default: u32 = Site,
    /// word jump table of the 0x9052.. opcodes (`ldr.w pc, [r2, r3, lsl #2]`)
    ble_dispatch_table: u32 = Rom,
    ble_dispatch_first: u32 = Op,
    /// highest opcode the dispatcher handles; the table has max - first + 1 entries
    ble_opcode_max: u32 = Op,
    /// opcode of the stub's DATA command on this model
    data_opcode: u32 = Op,
    /// every opcode the stub must receive through the default branch (DATA included)
    stub_opcodes: &'static [u32] = Op,
    /// stock Do Not Disturb sync opcode that collides with the WP2 DATA opcode
    dnd_opcode: Option<u32> = Op,
    /// `app_set_not_disturb_info`, called from the dispatcher case of `dnd_opcode`
    dnd_handler: Option<u32> = Func,
    decrypt: u32 = Func,
    encrypt: u32 = Func,
    send: u32 = Func,
    stream_space: u32 = Func,
    kv_get: u32 = Func,
    wewear_ctx: u32 = Ram,
    session_key: u32 = Ram,
    authkey: u32 = Ram,

    // --- widget cards (1.10, 2.7)
    cards_store: u32 = Ram,
    cards_save: u32 = Func,
    /// ux2sys property of the card list settings
    cards_prop: u32 = Prop,
    cards_bytes: u32 = Size,
    wgt_reg: u32 = Rom,
    /// a free wgt_reg id the stub's widget takes
    wgt_free_id: Option<u32> = Id,
    /// literal slot of `wgt_reg` in its only reader (relocation, 2.7)
    wgt_reg_literal: Option<u32> = Site,

    // --- app list (1.3, 2.6)
    /// list style's view-ops record: marker word 0xC18487B4, then layout, update
    applist_list_ops: u32 = Rom,
    applist_grid_ops: Option<u32> = Rom,
    applist_layout: u32 = Func,
    applist_update: Option<u32> = Func,
    applist_grid_layout: Option<u32> = Func,
    applist_grid_update: Option<u32> = Func,
    /// slot the stub repoints (WP2: layout, WP3: update)
    applist_hook_slot: u32 = Rom,
    applist_grid_hook_slot: Option<u32> = Rom,
    /// stock entries (rows)
    applist_entries: u32 = Val,
    /// view -> its data block
    applist_view_data: Option<u32> = Off,
    applist_data_cont: u32 = Off,
    applist_data_count: u32 = Off,
    applist_data_label_style: u32 = Off,
    applist_data_line_h: u32 = Off,
    simple_img_create: u32 = Func,
    simple_img_set_src: u32 = Func,
    simple_img_set_zoom: u32 = Func,
    simple_img_set_pivot: Option<u32> = Func,
    vlabel_create: u32 = Func,
    vlabel_set_text: u32 = Func,
    vlabel_set_long_mode: u32 = Func,
    label_set_string_dsc: Option<u32> = Func,
    obj_get_y: u32 = Func,
    obj_get_y2: u32 = Func,
    obj_set_x: u32 = Func,
    obj_set_opa: u32 = Func,
    obj_add_style: u32 = Func,
    event_send: u32 = Func,
    jx_scroll_bar_create: u32 = Func,
    jx_scroll_bar_set_scroll_obj: u32 = Func,
    jx_scroll_bar_set_knob: u32 = Func,

    // --- LVGL (1.5, 2.5)
    lv_obj_size: u32 = Size,
    lv_obj_user_data: u32 = Off,
    lv_obj_coords: u32 = Off,
    /// i32 y1 / y2 of `coords`
    lv_obj_y1: u32 = Off,
    lv_obj_y2: u32 = Off,
    /// tileview's active tile (first field after lv_obj_t)
    tile_act: u32 = Off,
    /// i16 value of an lv_arc
    arc_value: u32 = Off,
    lv_disp_get_scr_act: u32 = Func,
    lv_obj_create: u32 = Func,
    lv_obj_del: u32 = Func,
    lv_obj_add_flag: u32 = Func,
    lv_obj_clear_flag: u32 = Func,
    lv_obj_has_flag: u32 = Func,
    lv_obj_add_state: u32 = Func,
    lv_obj_clear_state: u32 = Func,
    lv_obj_set_pos: u32 = Func,
    lv_obj_set_size: u32 = Func,
    lv_obj_align: u32 = Func,
    lv_obj_align_to: u32 = Func,
    lv_obj_invalidate: u32 = Func,
    lv_obj_invalidate_area: u32 = Func,
    lv_obj_add_event_cb: u32 = Func,
    lv_event_get_code: u32 = Func,
    lv_event_get_param: u32 = Func,
    lv_event_get_user_data: u32 = Func,
    lv_event_get_target: u32 = Func,
    lv_indev_get_point: u32 = Func,
    lv_indev_get_gesture_dir: u32 = Func,
    lv_indev_wait_release: u32 = Func,
    lv_disp_add_knob_obj: u32 = Func,
    lv_obj_set_local_style_prop: u32 = Func,
    lv_obj_get_child: u32 = Func,
    lv_obj_get_child_cnt: u32 = Func,
    lv_obj_get_scroll_y: u32 = Func,
    lv_obj_get_scroll_bottom: u32 = Func,
    lv_obj_scroll_to_y: u32 = Func,
    lv_obj_set_flex_flow: u32 = Func,
    lv_obj_set_flex_align: u32 = Func,
    lv_timer_create: u32 = Func,
    lv_timer_del: u32 = Func,
    lv_img_create: u32 = Func,
    lv_img_set_src: u32 = Func,
    lv_label_create: u32 = Func,
    lv_label_set_text: u32 = Func,
    lv_label_set_long_mode: u32 = Func,
    lv_btn_create: u32 = Func,
    lv_arc_create: u32 = Func,
    lv_arc_set_value: u32 = Func,
    /// not in every image: read `arc_value` instead
    lv_arc_get_value: Option<u32> = Func,
    lv_arc_set_bg_angles: u32 = Func,
    lv_arc_set_rotation: u32 = Func,
    lv_bar_create: u32 = Func,
    lv_bar_set_value: u32 = Func,
    lv_bar_set_range: u32 = Func,
    lv_bar_get_value: u32 = Func,
    lv_slider_create: u32 = Func,
    lv_roller_create: u32 = Func,
    lv_roller_set_options: u32 = Func,
    lv_roller_set_selected: u32 = Func,
    lv_roller_get_selected: u32 = Func,
    lv_tileview_create: u32 = Func,
    lv_tileview_add_tile: u32 = Func,
    lv_obj_set_tile_id: u32 = Func,
    lv_tileview_get_tile_act: u32 = Func,
    lvgl_bitmap_font_open: u32 = Func,
    lvgl_bitmap_font_close: u32 = Func,
    lv_font_default: u32 = Rom,

    // --- scenes, fonts, return to face (1.5, 1.9)
    return_cb: u32 = Ram,
    return_clock: u32 = Func,
    scene_open: u32 = Func,
    scene_close: u32 = Func,
    scene_imgs: u32 = Func,
    scene_imgs_free: u32 = Func,
    scene_texts: u32 = Func,
    scene_texts_free: u32 = Func,
    scene_lang: u32 = Func,
    /// path strings of bt_watch.res / .sty
    scene_res: u32 = Rom,
    scene_sty: u32 = Rom,
    sport1_scene: Option<u32> = Val,
    sport1_imgs: Option<u32> = Rom,
    sport1_texts: Option<u32> = Rom,
    font_open_stock: u32 = Func,
    /// path string of the stock sans font with the emoji fallback (WP2 sans32, WP3 sans36)
    font_sans: u32 = Rom,
    /// path string of font32M
    font_32m: u32 = Rom,

    // --- fs (1.6, 2.10)
    fs_find_mount: u32 = Func,
    fs_open: u32 = Func,
    fs_close: u32 = Func,
    fs_read: u32 = Func,
    fs_write: u32 = Func,
    fs_seek: u32 = Func,
    fs_tell: u32 = Func,
    fs_sync: u32 = Func,
    fs_unlink: u32 = Func,
    fs_rename: u32 = Func,
    fs_stat: u32 = Func,
    fs_statvfs: u32 = Func,
    fs_opendir: u32 = Func,
    fs_readdir: u32 = Func,
    fs_closedir: u32 = Func,
    /// fs_file_t
    fs_file: u32 = Size,
    /// dirent that fs_readdir/fs_stat write
    fs_dirent: u32 = Size,
    /// mount point -> its file system's ops vtable
    fs_mount_ops: u32 = Off,
    fs_ops_unlink: u32 = Off,
    fs_ops_rename: u32 = Off,
    /// there is no fs_mkdir wrapper: `fs.rs` calls this vtable entry
    fs_ops_mkdir: u32 = Off,
    fat_ops: u32 = Rom,
    part_table_ptr: u32 = Ram,

    // --- time, power, wake lock (1.7, 2.8)
    /// (rtc_time *) local time
    rtc_get_time: Option<u32> = Func,
    /// () -> zone offset s
    time_zone: Option<u32> = Func,
    /// () -> UTC seconds
    utc_now: Option<u32> = Func,
    /// (ts, rtc_time *)
    utc_to_rtc: Option<u32> = Func,
    /// (ts, 0 = now) -> zone offset s, DST-aware
    utc_zone_offset: Option<u32> = Func,
    rtc_app: u32 = Ram,
    /// RTC device pointer in rtc_app
    rtc_app_device: u32 = Off,
    power_capacity: u32 = Func,
    power_status: u32 = Func,
    power_dc5v: u32 = Func,
    wake_lock: u32 = Func,
    wake_unlock: u32 = Func,

    // --- motor (1.8, 2.2)
    pwm_device: Option<u32> = Rom,
    pwm_pin_set_cycles: Option<u32> = Func,
    pwm_off: Option<u32> = Func,
    cmu_pwmclk: Option<u32> = Val,
    pwm_regs: Option<u32> = Val,
    /// (strength, effect) on the haptic chip; strength 0 stops
    stock_vibrate: Option<u32> = Func,
    prop_vibrate: u32 = Prop,

    // --- ux2sys, sensors, media, GPS (1.9)
    ux2sys_set: u32 = Func,
    ux2sys_get: u32 = Func,
    ux2sys_set_len: u32 = Func,
    /// {setter, getter, u32 len} per property
    ux2sys_table: u32 = Rom,
    /// properties in the table (ux2sys_set's bound + 1)
    ux2sys_count: u32 = Val,
    prop_hr_mode: u32 = Prop,
    prop_phone_link: u32 = Prop,
    prop_media_available: u32 = Prop,
    prop_media_playing: u32 = Prop,
    prop_media_cmd: u32 = Prop,
    prop_media_info: u32 = Prop,
    prop_volume: u32 = Prop,
    prop_gps_mode: u32 = Prop,
    /// seconds of dark panel before "return to watch face"
    prop_return_timeout: u32 = Prop,
    hr_wear: Option<u32> = Ram,
    hr_owner: Option<u32> = Ram,
    hr_bpm: Option<u32> = Ram,
    gps_state: Option<u32> = Ram,
    gps_info: Option<u32> = Ram,
}

/// CMF Watch Pro 2, JX402 1.0.0.73.
pub const WP2: Model = Model {
    name: "wp2",
    // mirrors firmware.py STOCK_SHA256_PREFIX, STUB_OFFSET
    stock_sha256_prefix: "30e7978c39d8fb79",
    app_size: 0x25_A080,

    // mirrors firmware.py BOOT_CALL, STOCK_HEAP_INIT (| 1, wp2-stub link.ld), HEAP_LITERALS, HEAP_OLD, HEAP_BASE
    boot_call: 0x1018_84FA,
    stock_heap_init: 0x1014_6AA5,
    heap_literals: &[0x1018_8508, 0x1018_8588, 0x1018_85F4],
    heap_size: 0x67_6B80,
    heap_base: 0x380A_9340,
    heap: heap::HEAP,
    // mirrors heap.rs target::free_blocks
    heap_free: 8,
    heap_free_list: 0x60,
    stub_ram: layout::STUB_RAM,
    ui_mem_aligned_alloc: stock::UI_MEM_ALIGNED_ALLOC,
    ui_mem_free: stock::UI_MEM_FREE,
    psram_cache_flush_invalidate: stock::PSRAM_CACHE_FLUSH_INVALIDATE,
    os_thread_create: stock::OS_THREAD_CREATE,
    os_sleep: stock::OS_SLEEP,
    k_thread_abort: stock::K_THREAD_ABORT,
    sys_clock_tick_get: stock::SYS_CLOCK_TICK_GET,
    // mirrors wp2-loader app.rs APP_PRIO
    app_thread_prio: Some(14),
    popup_flags: stock::POPUP_FLAGS,
    aod_msgbox: Some(events::AOD_MSGBOX as u32),

    view_table: stock::VIEW_TABLE,
    view_table_end: stock::VIEW_TABLE_END,
    // mirrors firmware.py add_stub `struct.pack("<IIIIHHHH", name, proc, 0, 0, id, 0x102, 466, 466)`
    view_record: 24,
    view_rec_proc: 4,
    view_rec_id: 0x10,
    // mirrors firmware.py VIEW_START_LITERALS, VIEW_END_LITERALS
    view_start_literals: &[0x1018_BC24, 0x1018_DF3C],
    view_end_literals: &[0x1018_BC28, 0x1018_DF40],
    view_open: stock::VIEW_OPEN,
    view_layout: stock::VIEW_LAYOUT,
    // own: max u16 id of the 162 stock records
    stock_view_id_max: 0xED,
    menu_view: layout::VIEW_ID as u32,
    page_view_base: cards::PAGE_VIEW as u32,
    // mirrors wp2-loader main.rs (view proc messages)
    view_msg_preload: Some(2),
    view_msg_layout: Some(3),
    view_msg_delete: Some(4),
    view_msg_focus: Some(7),
    view_msg_defocus: Some(8),
    view_msg_key: Some(0xD),
    view_msg_suspend: Some(0x82),
    view_msg_wake: Some(0x83),
    key_crown_press: Some(stock::KEY_CROWN_PRESS),
    key_crown_cw: Some(stock::KEY_CROWN_CW),
    key_crown_ccw: Some(stock::KEY_CROWN_CCW),
    key_back: Some(stock::KEY_BACK),

    // mirrors firmware.py BLE_CALL, wp2-stub ble.rs READ_PAYLOAD/DECRYPT/ENCRYPT/SEND and its `mov r2, r5`
    ble_call: 0x100D_34F0,
    read_payload: 0x100C_C285,
    ble_frame_reg: 5,
    // own: 0x100D350E `sub r3, #0x9052; cmp r3, #0x39; bhi default; adr r2; ldr.w pc, [r2, r3, lsl #2]`
    ble_dispatch_default: 0x100D_34EA,
    ble_dispatch_table: 0x100D_3520,
    ble_dispatch_first: 0x9052,
    ble_opcode_max: 0x908B,
    data_opcode: proto::DATA as u32,
    stub_opcodes: &[
        proto::INFO as u32,
        proto::BEGIN as u32,
        proto::DATA as u32,
        proto::END as u32,
        proto::DELETE as u32,
        proto::MSG as u32,
        proto::PING as u32,
        proto::PUSH as u32,
    ],
    dnd_opcode: None,
    dnd_handler: None,
    decrypt: 0x101B_608D,
    encrypt: 0x101B_605B,
    send: 0x100C_C2D5,
    stream_space: stock::STREAM_SPACE,
    kv_get: stock::KV_GET,
    wewear_ctx: stock::WEWEAR_CTX,
    session_key: stock::SESSION_KEY,
    authkey: stock::AUTHKEY,

    cards_store: stock::CARDS_STORE,
    cards_save: stock::CARDS_SAVE,
    // cards.rs: ux2sys 0x2F
    cards_prop: 0x2F,
    cards_bytes: cards::BYTES as u32,
    // mirrors firmware.py WGT_REG, WGT_ID (= cards::APP_WIDGET)
    wgt_reg: 0x101E_FC88,
    wgt_free_id: Some(cards::APP_WIDGET as u32),
    wgt_reg_literal: None,

    // own: marker 0xC18487B4 at 0x101EB15C, layout at +4 = firmware.py APPLIST_LAYOUT
    applist_list_ops: 0x101E_B15C,
    applist_grid_ops: None,
    // mirrors wp2-stub applist.rs STOCK_LAYOUT (firmware.py STOCK_APPLIST_LAYOUT)
    applist_layout: 0x1005_EEE5,
    applist_update: None,
    applist_grid_layout: None,
    applist_grid_update: None,
    applist_hook_slot: 0x101E_B160,
    applist_grid_hook_slot: None,
    // mirrors wp2-stub applist.rs ROWS and its view/data offsets
    applist_entries: 25,
    applist_view_data: Some(0x14),
    applist_data_cont: 4,
    applist_data_count: 8,
    applist_data_label_style: 0x200,
    applist_data_line_h: 0x214,
    simple_img_create: stock::SIMPLE_IMG_CREATE,
    simple_img_set_src: stock::SIMPLE_IMG_SET_SRC,
    // mirrors wp2-stub applist.rs VIMG_SET_ZOOM, VLABEL_*, OBJ_GET_Y/Y2, OBJ_SET_X/OPA, OBJ_ADD_STYLE, EVENT_SEND
    simple_img_set_zoom: 0x1005_A2D1,
    simple_img_set_pivot: None,
    vlabel_create: 0x1005_B579,
    vlabel_set_text: 0x1005_B70D,
    vlabel_set_long_mode: 0x101A_CBA5,
    label_set_string_dsc: None,
    obj_get_y: 0x1012_98B9,
    obj_get_y2: 0x1012_9B05,
    obj_set_x: 0x1012_90CD,
    obj_set_opa: 0x101C_74C5,
    obj_add_style: 0x1012_BB15,
    event_send: 0x1012_59C5,
    jx_scroll_bar_create: stock::JX_SCROLL_BAR_CREATE,
    jx_scroll_bar_set_scroll_obj: stock::JX_SCROLL_BAR_SET_SCROLL_OBJ,
    jx_scroll_bar_set_knob: stock::JX_SCROLL_BAR_SET_KNOB,

    // mirrors wp2-loader lv.rs Y1/Y2/TILE_DIR, wp2-stub card.rs USER_DATA; own: lv_obj_class instance_size 0x2C
    lv_obj_size: 0x2C,
    lv_obj_user_data: 0x10,
    lv_obj_coords: 0x14,
    lv_obj_y1: 0x18,
    lv_obj_y2: 0x20,
    tile_act: 0x2C,
    // own: lv_arc_t value after rotation and 4 angles (lv_obj_size + 0xA)
    arc_value: 0x36,
    lv_disp_get_scr_act: stock::LV_DISP_GET_SCR_ACT,
    lv_obj_create: stock::LV_OBJ_CREATE,
    lv_obj_del: stock::LV_OBJ_DEL,
    lv_obj_add_flag: stock::LV_OBJ_ADD_FLAG,
    lv_obj_clear_flag: stock::LV_OBJ_CLEAR_FLAG,
    lv_obj_has_flag: stock::LV_OBJ_HAS_FLAG,
    lv_obj_add_state: stock::LV_OBJ_ADD_STATE,
    lv_obj_clear_state: stock::LV_OBJ_CLEAR_STATE,
    lv_obj_set_pos: stock::LV_OBJ_SET_POS,
    lv_obj_set_size: stock::LV_OBJ_SET_SIZE,
    lv_obj_align: stock::LV_OBJ_ALIGN,
    lv_obj_align_to: stock::LV_OBJ_ALIGN_TO,
    lv_obj_invalidate: stock::LV_OBJ_INVALIDATE,
    lv_obj_invalidate_area: stock::LV_OBJ_INVALIDATE_AREA,
    lv_obj_add_event_cb: stock::LV_OBJ_ADD_EVENT_CB,
    lv_event_get_code: stock::LV_EVENT_GET_CODE,
    lv_event_get_param: stock::LV_EVENT_GET_PARAM,
    lv_event_get_user_data: stock::LV_EVENT_GET_USER_DATA,
    lv_event_get_target: stock::LV_EVENT_GET_TARGET,
    lv_indev_get_point: stock::LV_INDEV_GET_POINT,
    lv_indev_get_gesture_dir: stock::LV_INDEV_GET_GESTURE_DIR,
    lv_indev_wait_release: stock::LV_INDEV_WAIT_RELEASE,
    lv_disp_add_knob_obj: stock::LV_DISP_ADD_KNOB_OBJ,
    lv_obj_set_local_style_prop: stock::LV_OBJ_SET_LOCAL_STYLE_PROP,
    lv_obj_get_child: stock::LV_OBJ_GET_CHILD,
    lv_obj_get_child_cnt: stock::LV_OBJ_GET_CHILD_CNT,
    lv_obj_get_scroll_y: stock::LV_OBJ_GET_SCROLL_Y,
    lv_obj_get_scroll_bottom: stock::LV_OBJ_GET_SCROLL_BOTTOM,
    lv_obj_scroll_to_y: stock::LV_OBJ_SCROLL_TO_Y,
    lv_obj_set_flex_flow: stock::LV_OBJ_SET_FLEX_FLOW,
    lv_obj_set_flex_align: stock::LV_OBJ_SET_FLEX_ALIGN,
    lv_timer_create: stock::LV_TIMER_CREATE,
    lv_timer_del: stock::LV_TIMER_DEL,
    lv_img_create: stock::LV_IMG_CREATE,
    lv_img_set_src: stock::LV_IMG_SET_SRC,
    lv_label_create: stock::LV_LABEL_CREATE,
    lv_label_set_text: stock::LV_LABEL_SET_TEXT,
    lv_label_set_long_mode: stock::LV_LABEL_SET_LONG_MODE,
    lv_btn_create: stock::LV_BTN_CREATE,
    lv_arc_create: stock::LV_ARC_CREATE,
    lv_arc_set_value: stock::LV_ARC_SET_VALUE,
    lv_arc_get_value: Some(stock::LV_ARC_GET_VALUE),
    lv_arc_set_bg_angles: stock::LV_ARC_SET_BG_ANGLES,
    lv_arc_set_rotation: stock::LV_ARC_SET_ROTATION,
    lv_bar_create: stock::LV_BAR_CREATE,
    lv_bar_set_value: stock::LV_BAR_SET_VALUE,
    lv_bar_set_range: stock::LV_BAR_SET_RANGE,
    lv_bar_get_value: stock::LV_BAR_GET_VALUE,
    lv_slider_create: stock::LV_SLIDER_CREATE,
    lv_roller_create: stock::LV_ROLLER_CREATE,
    lv_roller_set_options: stock::LV_ROLLER_SET_OPTIONS,
    lv_roller_set_selected: stock::LV_ROLLER_SET_SELECTED,
    lv_roller_get_selected: stock::LV_ROLLER_GET_SELECTED,
    lv_tileview_create: stock::LV_TILEVIEW_CREATE,
    lv_tileview_add_tile: stock::LV_TILEVIEW_ADD_TILE,
    lv_obj_set_tile_id: stock::LV_OBJ_SET_TILE_ID,
    lv_tileview_get_tile_act: stock::LV_TILEVIEW_GET_TILE_ACT,
    lvgl_bitmap_font_open: stock::LVGL_BITMAP_FONT_OPEN,
    lvgl_bitmap_font_close: stock::LVGL_BITMAP_FONT_CLOSE,
    lv_font_default: stock::LV_FONT_DEFAULT,

    return_cb: stock::RETURN_CB,
    return_clock: stock::RETURN_CLOCK,
    scene_open: stock::SCENE_OPEN,
    scene_close: stock::SCENE_CLOSE,
    scene_imgs: stock::SCENE_IMGS,
    scene_imgs_free: stock::SCENE_IMGS_FREE,
    scene_texts: stock::SCENE_TEXTS,
    scene_texts_free: stock::SCENE_TEXTS_FREE,
    scene_lang: stock::SCENE_LANG,
    scene_res: stock::SCENE_RES,
    scene_sty: stock::SCENE_STY,
    sport1_scene: Some(stock::SPORT1_SCENE),
    sport1_imgs: Some(stock::SPORT1_IMGS),
    sport1_texts: Some(stock::SPORT1_TEXTS),
    font_open_stock: stock::FONT_OPEN_STOCK,
    font_sans: stock::FONT_SANS32,
    font_32m: stock::FONT_32M,

    fs_find_mount: stock::FS_FIND_MOUNT,
    fs_open: stock::FS_OPEN,
    fs_close: stock::FS_CLOSE,
    fs_read: stock::FS_READ,
    fs_write: stock::FS_WRITE,
    fs_seek: stock::FS_SEEK,
    fs_tell: stock::FS_TELL,
    fs_sync: stock::FS_SYNC,
    fs_unlink: stock::FS_UNLINK,
    fs_rename: stock::FS_RENAME,
    fs_stat: stock::FS_STAT,
    fs_statvfs: stock::FS_STATVFS,
    fs_opendir: stock::FS_OPENDIR,
    fs_readdir: stock::FS_READDIR,
    fs_closedir: stock::FS_CLOSEDIR,
    // mirrors fs.rs File (target-only module); stock.rs: u32 size at +0x104 of the dirent, own: fs_readdir's
    // last store is +0x104
    fs_file: 0x2C,
    fs_dirent: 0x108,
    // mirrors fs.rs mkdir; own: fs_unlink/fs_rename call ops +0x34/+0x38
    fs_mount_ops: 0x1C,
    fs_ops_unlink: 0x34,
    fs_ops_rename: 0x38,
    fs_ops_mkdir: 0x3C,
    // mirrors wp2-loader storage.rs FAT_OPS, TABLE_PTR
    fat_ops: 0x101F_6AC0,
    part_table_ptr: 0x3801_F1E8,

    rtc_get_time: Some(stock::RTC_GET_TIME),
    time_zone: Some(stock::TIME_ZONE),
    utc_now: None,
    utc_to_rtc: None,
    utc_zone_offset: None,
    // mirrors wp2-loader svc.rs RTC_APP (zone bytes at +0/+1, device at +4)
    rtc_app: 0x2FFB_449C,
    rtc_app_device: 4,
    power_capacity: stock::POWER_CAPACITY,
    power_status: stock::POWER_STATUS,
    power_dc5v: stock::POWER_DC5V,
    wake_lock: stock::WAKE_LOCK,
    wake_unlock: stock::WAKE_UNLOCK,

    pwm_device: Some(stock::PWM_DEVICE),
    pwm_pin_set_cycles: Some(stock::PWM_PIN_SET_CYCLES),
    pwm_off: Some(stock::PWM_OFF),
    cmu_pwmclk: Some(stock::CMU_PWMCLK),
    pwm_regs: Some(stock::PWM_REGS),
    stock_vibrate: None,
    prop_vibrate: stock::PROP_VIBRATE,

    ux2sys_set: stock::UX2SYS_SET,
    ux2sys_get: stock::UX2SYS_GET,
    ux2sys_set_len: stock::UX2SYS_SET_LEN,
    // own: literal of ux2sys_set (`cmp r0, #0x9e`, row = 12 * prop)
    ux2sys_table: 0x101F_4274,
    ux2sys_count: 0x9F,
    prop_hr_mode: stock::PROP_HR_MODE,
    prop_phone_link: stock::PROP_PHONE_LINK,
    prop_media_available: stock::PROP_MEDIA_AVAILABLE,
    prop_media_playing: stock::PROP_MEDIA_PLAYING,
    prop_media_cmd: stock::PROP_MEDIA_CMD,
    prop_media_info: stock::PROP_MEDIA_INFO,
    prop_volume: stock::PROP_VOLUME,
    prop_gps_mode: stock::PROP_GPS_MODE,
    // stock.rs RETURN_CB note: ux2sys 0x9C
    prop_return_timeout: 0x9C,
    hr_wear: Some(stock::HR_WEAR),
    hr_owner: Some(stock::HR_OWNER),
    hr_bpm: Some(stock::HR_BPM),
    gps_state: Some(stock::GPS_STATE),
    gps_info: Some(stock::GPS_INFO),
};

/// Watch 3 Pro, jx402_08 1.0.1.77 (PLAN.md section 1; H and M rows only).
pub const WP3: Model = Model {
    name: "wp3",
    // source: upstream firmware.py WP3_STOCK_SHA256_PREFIX, WP3_STUB_OFFSET
    stock_sha256_prefix: "815a21fc80aa3941",
    app_size: 0x2B_D280,

    // source: map + own (caller pushes {r0-r4, lr}, 3 stack args)
    boot_call: 0x1019_C646,
    stock_heap_init: 0x1015_2F19,
    // source: hunt + own (literal scan: exactly these 3 hold 0x636B00)
    heap_literals: &[0x1019_C654, 0x1019_C700, 0x1019_C76C],
    heap_size: 0x63_6B00,
    // source: map + own (literal 0x1019C65C)
    heap_base: 0x380E_91C0,
    // source: map + own (literal passed to heap init)
    heap: 0x3805_CCE0,
    // source: own (heap init `strd r2, r2, [r4, #8]`, `str r5, [r4, #0x60]`); node layout not re-read
    heap_free: 8,
    heap_free_list: 0x60,
    // source: hunt (computed: heap_base + heap_size - 0x1000)
    stub_ram: 0x3871_ECC0,
    // source: map + own
    ui_mem_aligned_alloc: 0x1019_C389,
    // source: map + patch
    ui_mem_free: 0x1019_C445,
    psram_cache_flush_invalidate: 0x101D_C72F,
    // source: map + own
    os_thread_create: 0x101D_BA09,
    // source: map
    os_sleep: 0x101D_BA53,
    k_thread_abort: 0x1011_4B65,
    sys_clock_tick_get: 0x101B_2EE9,
    // TODO(M1 recon): thread priorities not listed for WP3
    app_thread_prio: None,
    // source: hunt (same field offsets)
    popup_flags: 0x3801_5BBC,
    // TODO(M1 recon): AOD msgbox id not re-derived
    aod_msgbox: None,

    // source: map + own
    view_table: 0x1000_050C,
    view_table_end: 0x1000_136C,
    // source: own (184 records of 20 B: name, w1, w2, u16 id, u16 0x102, u16 466, u16 466)
    view_record: 20,
    // source: own (2 stock records use word 1 with word 2 = 0, the form the stub writes)
    view_rec_proc: 4,
    view_rec_id: 0xC,
    // source: hunt + own
    view_start_literals: &[0x101A_1304, 0x101A_3194],
    view_end_literals: &[0x101A_1308, 0x101A_3198],
    // source: map
    view_open: 0x1019_DEB1,
    view_layout: 0x101F_8A01,
    // source: own (table walk: 38 stock ids in 0xF0..0x117)
    stock_view_id_max: 0x117,
    // source: plan 2.3 (proposal above 0x117, clear of the 0x119 and 0x136+ wake-lock ids); M1 recon must confirm
    // the view manager takes ids >= 0x118
    menu_view: 0x120,
    page_view_base: 0x121,
    // TODO(M1 recon): view proc message numbers not re-read (generic view proc 0x100CFDB9)
    view_msg_preload: None,
    view_msg_layout: None,
    view_msg_delete: None,
    view_msg_focus: None,
    view_msg_defocus: None,
    view_msg_key: None,
    view_msg_suspend: None,
    view_msg_wake: None,
    // TODO(M1 recon): key codes not re-read
    key_crown_press: None,
    key_crown_cw: None,
    key_crown_ccw: None,
    key_back: None,

    // source: hunt + own (`ldrh.w r1, [r4, #1]` before it: frame in r4)
    ble_call: 0x100C_253A,
    // source: map + own
    read_payload: 0x100C_0EED,
    ble_frame_reg: 4,
    // source: own (0x100C2558 `sub r3, #0x9052; cmp r3, #0x94; bhi default; adr r2; ldr.w pc, [r2, r3, lsl #2]`)
    ble_dispatch_default: 0x100C_2534,
    ble_dispatch_table: 0x100C_2568,
    ble_dispatch_first: 0x9052,
    ble_opcode_max: 0x90E6,
    // source: plan 2.1 (first free opcode after the stub's; reply 0xB0A9)
    data_opcode: 0x90A9,
    stub_opcodes: &[
        proto::INFO as u32,
        proto::BEGIN as u32,
        0x90A9,
        proto::END as u32,
        proto::DELETE as u32,
        proto::MSG as u32,
        proto::PING as u32,
        proto::PUSH as u32,
    ],
    // source: own (jump table entry -> `bl 0x100CD7EC`, log string app_set_not_disturb_info)
    dnd_opcode: Some(0x90A2),
    dnd_handler: Some(0x100C_D7ED),
    // source: map + own (the stock 0x90A2 handler calls it); takes a k_mutex: thread context only
    decrypt: 0x100C_1B65,
    // source: map + kit review
    encrypt: 0x100C_19D1,
    send: 0x100C_0F3D,
    // source: map
    stream_space: 0x101E_E19D,
    kv_get: 0x1011_EC0D,
    // source: map + kit review (SEND literal)
    wewear_ctx: 0x3802_61B0,
    // source: hunt (corrected; map's 0x380032F1 is off by one)
    session_key: 0x3800_32F2,
    // source: map (data-ref, unaligned)
    authkey: 0x3806_27FB,

    // source: hunt
    cards_store: 0x3800_A404,
    // source: map
    cards_save: 0x100D_9FC9,
    // source: kit property-ids
    cards_prop: 0x32,
    cards_bytes: 290,
    // source: hunt + own (24 dense entries)
    wgt_reg: 0x1021_BEA0,
    // source: own (reader 0x1009498C `cmp r0, #0x17`: no free id)
    wgt_free_id: None,
    wgt_reg_literal: Some(0x1009_499C),

    // source: own (marker 0xC18487B4, then layout, update)
    applist_list_ops: 0x1021_5A60,
    applist_grid_ops: Some(0x1021_5940),
    // source: hunt + kit app-list
    applist_layout: 0x1004_7165,
    applist_update: Some(0x1004_7351),
    applist_grid_layout: Some(0x1004_6B6D),
    applist_grid_update: Some(0x1004_6D99),
    // source: hunt + patch (update-slot hook ran on a real WP3)
    applist_hook_slot: 0x1021_5A68,
    applist_grid_hook_slot: Some(0x1021_5948),
    // source: kit app-list + patch (30 entries, 27..30 shown)
    applist_entries: 30,
    // TODO(M1): view -> data offset not re-read for WP3
    applist_view_data: None,
    // source: kit app-list
    applist_data_cont: 4,
    applist_data_count: 8,
    applist_data_label_style: 0x264,
    applist_data_line_h: 0x274,
    // source: map + patch
    simple_img_create: 0x1004_1959,
    simple_img_set_src: 0x101C_675D,
    // source: map
    simple_img_set_zoom: 0x101C_6801,
    // source: kit + patch
    simple_img_set_pivot: Some(0x101C_67A7),
    // source: map
    vlabel_create: 0x1004_2A95,
    vlabel_set_text: 0x1004_2C29,
    vlabel_set_long_mode: 0x101C_694D,
    // source: kit + patch
    label_set_string_dsc: Some(0x1014_C5D1),
    // source: map
    obj_get_y: 0x101E_4EF9,
    // source: hunt + own (reads y1 +0x1C, y2 +0x24)
    obj_get_y2: 0x101E_4F27,
    // source: map
    obj_set_x: 0x101E_4D55,
    obj_set_opa: 0x101E_664F,
    obj_add_style: 0x1013_AA71,
    event_send: 0x1013_6A3D,
    // source: map + kit review
    jx_scroll_bar_create: 0x100D_17ED,
    jx_scroll_bar_set_scroll_obj: 0x100D_1805,
    jx_scroll_bar_set_knob: 0x101D_2139,

    // source: kit review + own (lv_obj_class instance_size 0x30; every field from 0x14 moves +4)
    lv_obj_size: 0x30,
    lv_obj_user_data: 0x10,
    lv_obj_coords: 0x18,
    lv_obj_y1: 0x1C,
    lv_obj_y2: 0x24,
    // source: kit + own (lv_tileview_get_tile_act `ldr r0, [r0, #0x30]`)
    tile_act: 0x30,
    // source: hunt + own (lv_arc_set_value `ldrsh.w r4, [r0, #0x3a]`)
    arc_value: 0x3A,
    // source: map (+ patch / review where PLAN.md 1.5 says so)
    lv_disp_get_scr_act: 0x1013_66DD,
    lv_obj_create: 0x1013_8B49,
    lv_obj_del: 0x101E_6719,
    lv_obj_add_flag: 0x101E_4367,
    lv_obj_clear_flag: 0x101E_428D,
    lv_obj_has_flag: 0x101E_4335,
    lv_obj_add_state: 0x101E_4311,
    lv_obj_clear_state: 0x101E_4323,
    lv_obj_set_pos: 0x101E_4DB1,
    lv_obj_set_size: 0x101E_4E23,
    lv_obj_align: 0x101E_4E85,
    lv_obj_align_to: 0x1013_9C45,
    lv_obj_invalidate: 0x101E_52A9,
    lv_obj_invalidate_area: 0x101E_526D,
    lv_obj_add_event_cb: 0x1013_6AA1,
    lv_event_get_code: 0x101E_37C7,
    lv_event_get_param: 0x101E_37CF,
    lv_event_get_user_data: 0x101E_37D3,
    lv_event_get_target: 0x101E_37BF,
    lv_indev_get_point: 0x101E_3AE9,
    lv_indev_get_gesture_dir: 0x101E_3B0F,
    lv_indev_wait_release: 0x101E_3B67,
    lv_disp_add_knob_obj: 0x1013_912D,
    lv_obj_set_local_style_prop: 0x101E_609D,
    lv_obj_get_child: 0x101E_66BB,
    lv_obj_get_child_cnt: 0x101E_66D9,
    lv_obj_get_scroll_y: 0x101E_55E7,
    lv_obj_get_scroll_bottom: 0x1013_A029,
    lv_obj_scroll_to_y: 0x101E_573B,
    lv_obj_set_flex_flow: 0x1014_18AD,
    lv_obj_set_flex_align: 0x1014_18FD,
    // source: map + upstream (wfguard::WP3)
    lv_timer_create: 0x1014_8E6D,
    lv_timer_del: 0x1014_8F05,
    // source: map
    lv_img_create: 0x1014_B2DD,
    lv_img_set_src: 0x1014_B2F5,
    lv_label_create: 0x1014_BB49,
    lv_label_set_text: 0x1014_C3D1,
    lv_label_set_long_mode: 0x1014_C611,
    lv_btn_create: 0x1014_A39D,
    // source: hunt
    lv_arc_create: 0x1014_9669,
    lv_arc_set_value: 0x101E_C44F,
    // source: hunt (not in the image; read arc_value)
    lv_arc_get_value: None,
    lv_arc_set_bg_angles: 0x101E_C431,
    lv_arc_set_rotation: 0x101E_C449,
    lv_bar_create: 0x1014_A385,
    lv_bar_set_value: 0x101E_C579,
    lv_bar_set_range: 0x101E_C643,
    lv_bar_get_value: 0x101E_C6B3,
    lv_slider_create: 0x1014_DA09,
    lv_roller_create: 0x1014_CD05,
    lv_roller_set_options: 0x1014_CD1D,
    // source: map
    lv_roller_set_selected: 0x101E_D02B,
    lv_roller_get_selected: 0x101E_D0B7,
    // source: hunt
    lv_tileview_create: 0x1014_60B9,
    // source: map
    lv_tileview_add_tile: 0x1014_60D1,
    lv_obj_set_tile_id: 0x1014_6101,
    // source: hunt
    lv_tileview_get_tile_act: 0x101E_AB1D,
    // source: map
    lvgl_bitmap_font_open: 0x1019_96E9,
    lvgl_bitmap_font_close: 0x1019_97F5,
    // source: map + patch
    lv_font_default: 0x1023_04B8,

    // source: map
    return_cb: 0x3802_55C0,
    // source: hunt
    return_clock: 0x1004_4F95,
    // source: map
    scene_open: 0x1019_82C9,
    scene_close: 0x101F_8E01,
    scene_imgs: 0x1019_8399,
    scene_imgs_free: 0x101F_8E33,
    scene_texts: 0x1019_8521,
    scene_texts_free: 0x101F_8E53,
    scene_lang: 0x100D_93AD,
    scene_res: 0x1023_B78B,
    scene_sty: 0x1023_B7A0,
    // TODO(M3): SPORT1 scene id not checked; hunt candidates imgs 0x102188C0 (ids diverge after the 4th) and
    // texts 0x102189A0 are low confidence: music tile layout must be re-read
    sport1_scene: None,
    sport1_imgs: None,
    sport1_texts: None,
    // source: map
    font_open_stock: 0x100D_9151,
    // source: map + own (the only /NAND:C/sans string is sans36: WP3 has no sans32)
    font_sans: 0x1023_8A8D,
    // source: hunt + own (/NAND:B/font32M.font)
    font_32m: 0x1023_8A39,

    // source: map + upstream (wfguard::WP3)
    fs_find_mount: 0x1011_5E89,
    fs_open: 0x1011_5F19,
    fs_close: 0x1011_60C9,
    fs_read: 0x1011_6121,
    fs_write: 0x1011_617D,
    // source: map
    fs_seek: 0x1011_61D9,
    fs_tell: 0x1011_6239,
    fs_sync: 0x1011_62D9,
    // source: map + upstream
    fs_unlink: 0x1011_6639,
    fs_rename: 0x1011_66F1,
    // source: hunt (emulator-called)
    fs_stat: 0x1011_7039,
    fs_statvfs: 0x101D_DEB7,
    fs_opendir: 0x1011_6FE1,
    fs_readdir: 0x1011_7081,
    fs_closedir: 0x1011_6D71,
    // source: upstream (wfguard's FILE_SIZE 0x2C runs on WP3); not re-read
    fs_file: 0x2C,
    // source: hunt + own (fs_readdir's last store is +0x108)
    fs_dirent: 0x10C,
    // source: hunt + own (fs_unlink/fs_rename call ops +0x38/+0x3C: an entry was inserted before them)
    fs_mount_ops: 0x1C,
    fs_ops_unlink: 0x38,
    fs_ops_rename: 0x3C,
    fs_ops_mkdir: 0x40,
    // source: hunt + own
    fat_ops: 0x1022_F18C,
    // source: hunt
    part_table_ptr: 0x3802_BB04,

    // source: hunt (replaced by utc_now + utc_to_rtc and utc_zone_offset, different signatures)
    rtc_get_time: None,
    time_zone: None,
    // source: hunt + patch
    utc_now: Some(0x100B_3CCD),
    // source: hunt
    utc_to_rtc: Some(0x101D_0CBF),
    utc_zone_offset: Some(0x100B_3CED),
    // source: hunt (no zone bytes; device pointer at +0x24)
    rtc_app: 0x2FF9_C180,
    rtc_app_device: 0x24,
    // source: map
    power_capacity: 0x101E_E471,
    power_status: 0x101E_E477,
    power_dc5v: 0x101E_E483,
    wake_lock: 0x101D_0DE1,
    // source: hunt
    wake_unlock: 0x100B_4C45,

    // source: hunt + kit: the PWM functions exist but never drive the motor (haptic chip): do not use
    pwm_device: None,
    pwm_pin_set_cycles: None,
    pwm_off: None,
    cmu_pwmclk: None,
    pwm_regs: None,
    // source: kit + patch (real hardware buzz)
    stock_vibrate: Some(0x100D_A77D),
    // source: kit property-ids
    prop_vibrate: 0x29,

    // source: map + kit review
    ux2sys_set: 0x100D_A1A1,
    ux2sys_get: 0x100D_A21D,
    ux2sys_set_len: 0x100D_A2A9,
    // source: kit review + own (`cmp r0, #0xd3`)
    ux2sys_table: 0x1022_0DE4,
    ux2sys_count: 0xD4,
    // source: kit property-ids (0x00..0x23 unchanged, then shifted)
    prop_hr_mode: 0x02,
    prop_phone_link: 0x34,
    prop_media_available: 0x53,
    prop_media_playing: 0x54,
    prop_media_cmd: 0x55,
    prop_media_info: 0x56,
    prop_volume: 0x57,
    prop_gps_mode: 0x6A,
    prop_return_timeout: 0xA7,
    // TODO(M3): HR cells are map-derived guesses (0x38064D9E / A1 / A2); verify in the emulator
    hr_wear: None,
    hr_owner: None,
    hr_bpm: None,
    // source: hunt: not found (GPS service rewritten)
    gps_state: None,
    // TODO(M3): hunt candidate 0x38020C68 with a different layout (0xA8 B, lat +0x68)
    gps_info: None,
};

pub const MODELS: [&Model; 2] = [&WP2, &WP3];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const VBASE: u32 = 0x1000_0000;

    fn fields(m: &Model) -> HashMap<&'static str, (Kind, Val)> {
        m.fields().map(|(n, k, v)| (n, (k, v))).collect()
    }

    fn nums(v: Val) -> Vec<u32> {
        match v {
            Val::Num(n) => vec![n],
            Val::List(l) => l.to_vec(),
            Val::Unknown | Val::Text(_) => vec![],
        }
    }

    fn stub_views(m: &Model) -> Vec<u32> {
        let pages = m.page_view_base..m.page_view_base + layout::SLOTS as u32;
        core::iter::once(m.menu_view).chain(pages).collect()
    }

    #[test]
    fn both_models_have_the_same_fields() {
        let a: Vec<_> = WP2.fields().map(|(n, k, _)| (n, k)).collect();
        let b: Vec<_> = WP3.fields().map(|(n, k, _)| (n, k)).collect();
        assert_eq!(a, b);
        assert!(a.len() > 200);
    }

    #[test]
    fn addresses_are_in_range_and_thumb_where_code() {
        for m in MODELS {
            let end = VBASE + m.app_size;
            for (name, kind, v) in m.fields() {
                for a in nums(v) {
                    match kind {
                        Kind::Func => assert!(a & 1 == 1 && (VBASE..end).contains(&a), "{} {name} {a:#x}", m.name),
                        Kind::Site => assert!(a & 1 == 0 && (VBASE..end).contains(&a), "{} {name} {a:#x}", m.name),
                        Kind::Rom => assert!((VBASE..end).contains(&a), "{} {name} {a:#x}", m.name),
                        Kind::Ram => assert!((0x2FF0_0000..0x3880_0000).contains(&a), "{} {name} {a:#x}", m.name),
                        _ => {}
                    }
                }
            }
        }
    }

    #[test]
    fn functions_and_sites_are_distinct() {
        for m in MODELS {
            let mut seen: HashMap<u32, &str> = HashMap::new();
            for (name, kind, v) in m.fields() {
                if !matches!(kind, Kind::Func | Kind::Site) {
                    continue;
                }
                for a in nums(v) {
                    if let Some(other) = seen.insert(a & !1, name) {
                        panic!("{}: {name} and {other} share {a:#x}", m.name);
                    }
                }
            }
        }
    }

    #[test]
    fn heap_split_gives_stub_ram() {
        for m in MODELS {
            assert_eq!(m.stub_ram, m.heap_base + m.heap_size - layout::STUB_RAM_SIZE, "{}", m.name);
            assert_eq!(m.heap_literals.len(), 3);
        }
        assert_eq!(WP2.heap_size, 0x67_6B80);
        assert_eq!(WP3.heap_size, 0x63_6B00);
    }

    #[test]
    fn stub_view_ids_clear_the_stock_ones() {
        for m in MODELS {
            for id in stub_views(m) {
                assert!(id > m.stock_view_id_max && id <= 0xFFFF, "{} view {id:#x}", m.name);
            }
        }
        // 2.3: stock WP3 uses 0xF0..0x117; the wake-lock ids 0x119 and 0x136+ are taken
        for id in stub_views(&WP3) {
            assert!(!(0xF0..=0x117).contains(&id) && id != 0x119 && id < 0x136, "{id:#x}");
        }
        assert_eq!(stub_views(&WP2), [0xF0, 0xF8, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD, 0xFE, 0xFF, 0x100, 0x101, 0x102,
            0x103, 0x104, 0x105, 0x106, 0x107]);
        assert_eq!(cards::page_slot(WP2.page_view_base + 3), Some(3));
    }

    #[test]
    fn view_records() {
        assert_eq!((WP2.view_record, WP2.view_rec_id), (24, 0x10));
        assert_eq!((WP3.view_record, WP3.view_rec_id), (20, 0xC));
        for m in MODELS {
            assert_eq!((m.view_table_end - m.view_table) % m.view_record, 0, "{}", m.name);
            // id, 0x102, width, height close the record
            assert_eq!(m.view_rec_id + 8, m.view_record, "{}", m.name);
            assert!(m.view_rec_proc >= 4 && m.view_rec_proc < m.view_rec_id);
            assert_eq!(m.view_start_literals.len(), m.view_end_literals.len());
        }
    }

    #[test]
    fn ble_channel() {
        assert_eq!(WP2.data_opcode, proto::DATA as u32);
        assert_eq!(WP3.data_opcode, 0x90A9);
        assert_ne!(WP3.data_opcode, 0x90A2);
        assert_eq!(WP3.dnd_opcode, Some(0x90A2));
        assert_eq!((WP2.ble_frame_reg, WP3.ble_frame_reg), (5, 4));
        for m in MODELS {
            assert!(m.stub_opcodes.contains(&m.data_opcode));
            assert!(m.dnd_opcode.is_none_or(|d| !m.stub_opcodes.contains(&d)), "{}", m.name);
            let mut ops = m.stub_opcodes.to_vec();
            ops.sort();
            ops.dedup();
            assert_eq!(ops.len(), m.stub_opcodes.len(), "{}", m.name);
            assert!(m.ble_dispatch_first <= 0x90A0 && m.ble_opcode_max >= m.ble_dispatch_first);
            assert!(m.ble_dispatch_table > m.ble_dispatch_default && m.ble_call > m.ble_dispatch_default);
            assert_eq!(m.dnd_opcode.is_some(), m.dnd_handler.is_some());
        }
    }

    #[test]
    fn lv_obj_layout() {
        assert_eq!((WP2.lv_obj_size, WP3.lv_obj_size), (0x2C, 0x30));
        for m in MODELS {
            assert_eq!(m.lv_obj_y1, m.lv_obj_coords + 4);
            assert_eq!(m.lv_obj_y2, m.lv_obj_coords + 0xC);
            assert_eq!(m.tile_act, m.lv_obj_size);
            assert_eq!(m.arc_value, m.lv_obj_size + 0xA);
            assert!(m.lv_obj_user_data + 4 <= m.lv_obj_coords);
        }
    }

    #[test]
    fn fs_vtable() {
        assert_eq!((WP2.fs_ops_mkdir, WP3.fs_ops_mkdir), (0x3C, 0x40));
        assert_eq!((WP2.fs_dirent, WP3.fs_dirent), (0x108, 0x10C));
        for m in MODELS {
            assert_eq!(m.fs_ops_rename, m.fs_ops_unlink + 4);
            assert_eq!(m.fs_ops_mkdir, m.fs_ops_rename + 4);
            assert_eq!(m.fs_file, 0x2C);
        }
    }

    #[test]
    fn app_list_hooks() {
        assert_eq!(WP2.applist_hook_slot, WP2.applist_list_ops + 4);
        assert_eq!(WP3.applist_hook_slot, WP3.applist_list_ops + 8);
        assert_eq!(WP3.applist_grid_hook_slot, WP3.applist_grid_ops.map(|o| o + 8));
        assert_ne!(Some(WP3.applist_hook_slot), WP3.applist_grid_hook_slot);
        assert_eq!(WP3.wgt_free_id, None);
    }

    #[test]
    fn wp3_shares_the_values_wfguard_already_uses() {
        use crate::wfguard;
        for (m, d) in [(&WP2, &wfguard::WP2), (&WP3, &wfguard::WP3)] {
            let pairs = [
                (m.lv_timer_create, d.timer_create),
                (m.lv_timer_del, d.timer_del),
                (m.fs_open, d.fs_open),
                (m.fs_close, d.fs_close),
                (m.fs_read, d.fs_read),
                (m.fs_write, d.fs_write),
                (m.fs_unlink, d.fs_unlink),
            ];
            for (a, b) in pairs {
                assert_eq!(a, b, "{}", m.name);
            }
        }
    }

    #[test]
    fn unknowns_are_explicit() {
        let f = fields(&WP3);
        for name in ["lv_arc_get_value", "aod_msgbox", "key_back", "view_msg_key", "rtc_get_time", "gps_state"] {
            assert_eq!(f[name].1, Val::Unknown, "{name}");
        }
    }

    pub(super) fn json() -> String {
        let mut out = String::from("{\n");
        for (i, m) in MODELS.iter().enumerate() {
            out += &format!("  \"{}\": {{\n", m.name);
            let rows: Vec<_> = m.fields().collect();
            for (j, (name, kind, v)) in rows.iter().enumerate() {
                let value = match v {
                    Val::Num(n) => format!("\"{n:#x}\""),
                    Val::Unknown => "null".into(),
                    Val::List(l) => {
                        let items: Vec<_> = l.iter().map(|n| format!("\"{n:#x}\"")).collect();
                        format!("[{}]", items.join(", "))
                    }
                    Val::Text(s) => format!("\"{s}\""),
                };
                let kind = format!("{kind:?}").to_lowercase();
                let comma = if j + 1 < rows.len() { "," } else { "" };
                out += &format!("    \"{name}\": {{\"kind\": \"{kind}\", \"value\": {value}}}{comma}\n");
            }
            out += if i + 1 < MODELS.len() { "  },\n" } else { "  }\n" };
        }
        out + "}\n"
    }

    #[test]
    fn json_is_current() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/model.json");
        let want = json();
        if std::env::var_os("UPDATE_MODEL_JSON").is_some() {
            std::fs::write(path, &want).unwrap();
        }
        let have = std::fs::read_to_string(path).unwrap_or_default().replace("\r\n", "\n");
        assert!(have == want, "model.json is stale: run `UPDATE_MODEL_JSON=1 cargo test -p wp2-core model`");
    }
}
