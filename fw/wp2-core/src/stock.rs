// JX402 1.0.0.73 only (docs/recon.md). Functions carry the Thumb bit, data does not.
pub const UI_MEM_ALIGNED_ALLOC: u32 = 0x101882C5;
pub const UI_MEM_FREE: u32 = 0x10188381;
// (addr, len): SPI1 (PSRAM) cache write-buffer flush + clean/invalidate range, no-op outside
// 0x34000000..0x3C000000; takes a k_mutex, thread context only
pub const PSRAM_CACHE_FLUSH_INVALIDATE: u32 = 0x101BEE53;

pub const FS_FIND_MOUNT: u32 = 0x10106FA1;
pub const FS_OPEN: u32 = 0x10107031;
pub const FS_CLOSE: u32 = 0x101071E1;
pub const FS_READ: u32 = 0x10107239;
pub const FS_WRITE: u32 = 0x10107295;
pub const FS_SEEK: u32 = 0x101072F1;
pub const FS_TELL: u32 = 0x10107351;
pub const FS_SYNC: u32 = 0x101073A5;
pub const FS_UNLINK: u32 = 0x1010764D;
pub const FS_RENAME: u32 = 0x10107705;
// Read-only metadata for storage inspection (docs/storage-inspection.md). fs_stat/fs_readdir fill the
// production dirent: u8 type, name[256], u32 size at +0x104 (not the SDK's enum-sized fs_dirent).
pub const FS_STAT: u32 = 0x10107DC1;
pub const FS_STATVFS: u32 = 0x101BF769;
pub const FS_OPENDIR: u32 = 0x10107D09;
pub const FS_READDIR: u32 = 0x10107E09;
pub const FS_CLOSEDIR: u32 = 0x10107AE9;

pub const OS_THREAD_CREATE: u32 = 0x101BE1A3;
pub const OS_SLEEP: u32 = 0x101BE1ED;
// z_thread_entry calls it with k_current_get() after the entry returns
pub const K_THREAD_ABORT: u32 = 0x10105C91;
// u64 ticks, CONFIG_SYS_CLOCK_TICKS_PER_SEC 1000 (os_sleep passes ms to k_sleep unscaled)
pub const SYS_CLOCK_TICK_GET: u32 = 0x1019B7E1;
pub const VIEW_OPEN: u32 = 0x101898BD;
// posts "layout" for a view id; a preload handler returns it, else msg 3 never comes
pub const VIEW_LAYOUT: u32 = 0x101D72FF;
pub const VIEW_TABLE: u32 = 0x100004B8;
pub const VIEW_TABLE_END: u32 = 0x100013E8;

pub const LV_DISP_GET_SCR_ACT: u32 = 0x101255E1;
pub const LV_IMG_CREATE: u32 = 0x1013ED39;
pub const LV_IMG_SET_SRC: u32 = 0x1013ED51;
pub const LV_OBJ_SET_POS: u32 = 0x1012926D;
pub const LV_OBJ_SET_SIZE: u32 = 0x101294C9;
pub const LV_OBJ_INVALIDATE: u32 = 0x10129FD9;
// (obj, &lv_area_t): absolute i32 x1, y1, x2, y2 (inclusive), clipped to the object (docs/recon-lvgl.md)
pub const LV_OBJ_INVALIDATE_AREA: u32 = 0x10129EF1;
pub const LV_OBJ_DEL: u32 = 0x1012C775;
pub const LV_TIMER_CREATE: u32 = 0x1013B10D;
pub const LV_TIMER_DEL: u32 = 0x1013B1A5;
pub const LV_OBJ_ADD_FLAG: u32 = 0x10128D39;
pub const LV_OBJ_ADD_EVENT_CB: u32 = 0x10125A95;
pub const LV_EVENT_GET_CODE: u32 = 0x101C4C9B;
pub const LV_EVENT_GET_PARAM: u32 = 0x101C4CA3;
// lv_point_t is two i32 here (LV_USE_LARGE_COORD)
pub const LV_INDEV_GET_POINT: u32 = 0x101C4F57;
pub const LV_EVENT_GET_USER_DATA: u32 = 0x101C4CA7;
pub const LV_OBJ_CREATE: u32 = 0x10127CFD;
pub const LV_OBJ_CLEAR_FLAG: u32 = 0x10127E7D;
// (obj): makes an already registered knob object the display's current one (docs/recon-lvgl.md)
pub const LV_DISP_ADD_KNOB_OBJ: u32 = 0x10128C51;
// (obj, prop, value, selector); value is lv_style_value_t (i32, pointer or RGB565 in the low half)
pub const LV_OBJ_SET_LOCAL_STYLE_PROP: u32 = 0x101C6F9B;
pub const LV_OBJ_ALIGN: u32 = 0x101C5DE5;
// jx_scroll_bar (lv_arc): round indicator; stock lists create it on the screen, size 462, align CENTER (0x1005EFA6)
pub const JX_SCROLL_BAR_CREATE: u32 = 0x100D93CD;
// (bar, obj): follows SCROLL_BEGIN/END/SCROLL of obj
pub const JX_SCROLL_BAR_SET_SCROLL_OBJ: u32 = 0x100D93E5;
// (bar, on): sets or clears the knob flag (1 << 30) on the scroll obj, so the crown scrolls it
pub const JX_SCROLL_BAR_SET_KNOB: u32 = 0x100D94ED;
pub const LV_LABEL_CREATE: u32 = 0x1013F5F1;
// copies the text into lv_mem
pub const LV_LABEL_SET_TEXT: u32 = 0x101400A5;
pub const LV_LABEL_SET_LONG_MODE: u32 = 0x1014051D;
// (lv_font_t *, path) -> 0 / -1; the 0x20-byte lv_font_t has line_height (i32) at +8
pub const LVGL_BITMAP_FONT_OPEN: u32 = 0x10185C31;
pub const LVGL_BITMAP_FONT_CLOSE: u32 = 0x10185D3D;
pub const LV_FONT_DEFAULT: u32 = 0x101FB6BC;
// style property ids of this build (8.3.10 numbering, read from the lv_obj_set_style_* setters)
pub const LV_STYLE_WIDTH: u32 = 0x01;
pub const LV_STYLE_RADIUS: u32 = 0x0B;
pub const LV_STYLE_TRANSLATE_X: u32 = 0x6B;
pub const LV_STYLE_PAD_BOTTOM: u32 = 0x11;
pub const LV_STYLE_PAD_LEFT: u32 = 0x12;
pub const LV_STYLE_BG_COLOR: u32 = 0x20;
pub const LV_STYLE_BG_OPA: u32 = 0x21;
pub const LV_STYLE_TEXT_COLOR: u32 = 0x55;
pub const LV_STYLE_TEXT_OPA: u32 = 0x56;
pub const LV_STYLE_TEXT_FONT: u32 = 0x57;
pub const LV_STYLE_TEXT_ALIGN: u32 = 0x5B;

// ABI 1.2 widgets, each run in the emulator by scripts/recon/lv_entries.py (docs/recon-lvgl.md "Verified for ABI 1.2").
// lv_arc_set_range is not in the image: arcs keep the default range 0..100; arc value/min/max are i16.
pub const LV_BTN_CREATE: u32 = 0x1013D389;
pub const LV_ARC_CREATE: u32 = 0x1013BA55;
// (obj, i16): clamps to the range, returns early when unchanged
pub const LV_ARC_SET_VALUE: u32 = 0x1013C109;
pub const LV_ARC_GET_VALUE: u32 = 0x1013C2ED;
// (obj, u16 start, u16 end)
pub const LV_ARC_SET_BG_ANGLES: u32 = 0x101CCC83;
pub const LV_ARC_SET_ROTATION: u32 = 0x1013C055;
pub const LV_BAR_CREATE: u32 = 0x1013CEA1;
// (obj, i32, anim); slider values go through the bar functions (lv_slider is an lv_bar)
pub const LV_BAR_SET_VALUE: u32 = 0x1013CEB9;
pub const LV_BAR_SET_RANGE: u32 = 0x1013D295;
pub const LV_BAR_GET_VALUE: u32 = 0x1013D079;
pub const LV_SLIDER_CREATE: u32 = 0x10141EB9;
pub const LV_ROLLER_CREATE: u32 = 0x10140E01;
// (obj, options, mode); copies the options
pub const LV_ROLLER_SET_OPTIONS: u32 = 0x10140E19;
// (obj, u16, anim)
pub const LV_ROLLER_SET_SELECTED: u32 = 0x10141045;
pub const LV_ROLLER_GET_SELECTED: u32 = 0x10141691;
// also sets the layout to flex
pub const LV_OBJ_SET_FLEX_FLOW: u32 = 0x10133545;
// (obj, main, cross, track)
pub const LV_OBJ_SET_FLEX_ALIGN: u32 = 0x10133595;
pub const LV_EVENT_GET_TARGET: u32 = 0x101C4C93;
// (obj, flag) -> bool; the checker of set_knob in lv_entries.py
pub const LV_OBJ_HAS_FLAG: u32 = 0x101283DD;
// FOCUS (docs/recon-lvgl.md, `__func__`, high): (obj, i32 index) -> child, negative from the end; child count;
// (obj, state)
pub const LV_OBJ_GET_CHILD: u32 = 0x1012C3DD;
pub const LV_OBJ_GET_CHILD_CNT: u32 = 0x1012C4AD;
pub const LV_OBJ_ADD_STATE: u32 = 0x10128245;
pub const LV_OBJ_CLEAR_STATE: u32 = 0x10128311;
// Pager (docs/recon-lvgl.md, "lv_tileview"; upstream 8.3 shapes read with thumb_dis): create(parent);
// add_tile(tv, col u8, row u8, dir u8) stores col/row/dir for the tile constructor; set_tile_id(tv, col, row, anim);
// get_tile_act(tv) reads tv+0x2C. Scroll: get_scroll_y = -spec_attr.scroll.y, get_scroll_bottom (`__func__`),
// scroll_to_y(obj, y, anim) (the second call of lv_obj_scroll_to 0x101C6650, which lv_obj_set_tile ends in).
pub const LV_TILEVIEW_CREATE: u32 = 0x101380A1;
pub const LV_TILEVIEW_ADD_TILE: u32 = 0x101380B9;
pub const LV_OBJ_SET_TILE_ID: u32 = 0x101380E9;
pub const LV_TILEVIEW_GET_TILE_ACT: u32 = 0x101CB681;
pub const LV_OBJ_GET_SCROLL_Y: u32 = 0x101C6515;
pub const LV_OBJ_GET_SCROLL_BOTTOM: u32 = 0x1012AE09;
pub const LV_OBJ_SCROLL_TO_Y: u32 = 0x1012B381;
// Ids of the 48 stock lv_obj_set_style_* setters (44 tail-call set_local_style_prop with `movs r1, #id`,
// 0x101C72CC..0x101C751A; the 4 flex setters load ids that lv_flex_init registers at boot: flow 113, main_place
// 114, cross_place 115, track_place 116, read in the emulator). 45, 92, 93 are Actions additions. Sorted.
pub const STYLE_PROPS: [u16; 48] = [
    1, 4, 6, 7, 8, 9, 10, 11, 16, 17, 18, 19, 20, 21, 22, 32, 33, 34, 40, 45, 48, 50, 53, 54, 55, 70, 71, 72, 80, 81,
    82, 83, 84, 85, 86, 87, 89, 91, 92, 93, 96, 102, 107, 108, 113, 114, 115, 116,
];

pub const PWM_DEVICE: u32 = 0x101DF1C0;
pub const PWM_PIN_SET_CYCLES: u32 = 0x1012197D;
pub const PWM_OFF: u32 = 0x101214AD;
pub const CMU_PWMCLK: u32 = 0x4000_1068;
pub const PWM_REGS: u32 = 0x4006_0000;

pub const POPUP_FLAGS: u32 = 0x3800FAF4;
// The card list left of the face (wp2-core cards.rs): the settings copy the phone's 0x906D set writes, then its
// `0x100E05EC(0xE4, 0)` (store and lay the cards out again, docs/recon.md "Widget cards")
pub const CARDS_STORE: u32 = 0x3800_7880;
pub const CARDS_SAVE: u32 = 0x100E_05ED;

pub const KEY_CROWN_PRESS: u32 = 0x0400_0001;
pub const KEY_CROWN_CW: u32 = 0xFFFF_006F;
pub const KEY_CROWN_CCW: u32 = 0xFFFF_006E;
// right swipe; the launcher (0x1005DB08) matches the low half only and pops the view stack (docs/recon.md)
pub const KEY_BACK: u32 = 0x71;

// ABI 1.1 (docs/abi-1.1.md, docs/recon.md "ABI 1.1"): wewear_read's context (+0 the wewear_read thread, bit 0 of
// byte +4 stream connected, +0x18 the BLE stream) and the stock key buffers.
pub const WEWEAR_CTX: u32 = 0x3801_A658;
// (stream) -> free bytes, negative when closed (0x100CC2D4 writes all or nothing)
pub const STREAM_SPACE: u32 = 0x101CE411;
pub const SESSION_KEY: u32 = 0x3800_29A6;
pub const AUTHKEY: u32 = 0x3804_8E08;
// (struct rtc_time *): {u8 sec, min, hour, mday, mon 1..12, wday 0 = Sunday, u16 year, u16 ms}, local time
pub const RTC_GET_TIME: u32 = 0x100C14B5;
// () -> i32: zone offset in seconds (signed bytes h, m at 0x2FFB449C)
pub const TIME_ZONE: u32 = 0x100C1459;
// power supply properties: capacity %, status (3 charging, 4 full), DC5V present; negative on error
pub const POWER_CAPACITY: u32 = 0x101CE6FD;
pub const POWER_STATUS: u32 = 0x101CE703;
pub const POWER_DC5V: u32 = 0x101CE70F;
// per-id screen wake lock (stock views lock their own view id). The last unlock restarts the auto-sleep timeout
// (sys_wake_unlock stores the time); AT SETWAKELOCK 0 also calls 0x100C1739, which blanks at once if it ran out.
pub const WAKE_LOCK: u32 = 0x101B59A9;
pub const WAKE_UNLOCK: u32 = 0x100C1859;
// NVRAM read (name, buf, len) -> bytes read or < 0; takes the NVRAM lock, safe from any thread. The AT GETSECRET
// handler 0x100E9DD0 reads the pairing secret with it (docs/recon.md, "Pairing secret").
pub const KV_GET: u32 = 0x1010_E215;
pub const APP_SECRET: &[u8] = b"APP_SECRET\0";
// Heart rate (docs/recon.md, "Heart rate"). (prop, *in): the ux2sys setter, synchronous; property 2 =
// change_hr_mode (u32 1 on, 0 off), as the stock heart view (0x1006F20C) sets it.
pub const UX2SYS_SET: u32 = 0x100E_07B9;
pub const PROP_HR_MODE: u32 = 0x02;
// (prop, *out): the ux2sys getter (0x100E082C), as the stock music view and workout screen read media state
// (docs/abi-1.4.md, "Music"). Values are u32 but 0x4F (0x82 B: title [0..0x40], artist [0x41..0x81]).
pub const UX2SYS_GET: u32 = 0x100E_082D;
pub const PROP_PHONE_LINK: u32 = 0x31;
pub const PROP_MEDIA_AVAILABLE: u32 = 0x4C;
pub const PROP_MEDIA_PLAYING: u32 = 0x4D;
pub const PROP_MEDIA_CMD: u32 = 0x4E;
pub const PROP_MEDIA_INFO: u32 = 0x4F;
// u8: non-zero = on the wrist (ux2sys 0x13; its getter logs every call, so the loader reads the byte)
pub const HR_WEAR: u32 = 0x3804_B30C;
// u8 owner of the HR mode: 0 none, 1 property 2 (heart view, an app), 2 a stock workout (0x100C9434)
pub const HR_OWNER: u32 = 0x3804_B30F;
// u8 latest bpm, 0 = none; hr_callback type 5 (0x100E337A) writes it, the heart view clears it at start
pub const HR_BPM: u32 = 0x3804_B310;
// GPS (docs/recon.md, "GPS"). ux2sys 0x63 = GPS mode (u32: 0 off, 1 the workouts' normal mode, 4 and 5 other
// constellation sets); its setter 0x101B9B4E posts to gps_service, which powers the AG3352 on or off.
pub const PROP_GPS_MODE: u32 = 0x63;
// gps_service state: u8 [1] = mode in force (0 = off). Non-zero before the app's start = the stock owns the GPS.
pub const GPS_STATE: u32 = 0x3804_43F4;
// the NMEA parser's info block (f64 lat +0x38, f64 lon +0x40, f32 altitude m +0x48, u32 fix quality +0x4C,
// u32 satellites +0x50, u32 time to first fix s +0x54); cleared and refilled at every GGA, zeroed at mode 0.
pub const GPS_INFO: u32 = 0x3801_6C40;
// The launcher's "return to watch face" (docs/recon.md, "Return to watch face"): u32 callback at RETURN_CB, set to
// RETURN_CLOCK at launch; late-resume calls it when the panel was dark for ux2sys 0x9C seconds, unless it is 0.
pub const RETURN_CB: u32 = 0x3801_9B10;
pub const RETURN_CLOCK: u32 = 0x1005_D969;
// The stock workout screen's music tile (sport1_view 0x64, its third tile; docs/recon.md, "Music tile"). Scene
// resources: SCENE_OPEN(scene id, &Scene {id, node, res}, sty path, res path, language path) opens a scene of
// bt_watch.res (refcounted); SCENE_IMGS(&scene, ids, out lv_img_dsc_t[n], 0, n) / SCENE_TEXTS(&scene, ids,
// out {char *, u32}[n], n) load n resources by hash id; the _FREE calls (out, n) and SCENE_CLOSE(&scene) undo them.
pub const SCENE_OPEN: u32 = 0x1018_4805;
pub const SCENE_CLOSE: u32 = 0x101D_7715;
pub const SCENE_IMGS: u32 = 0x1018_48D5;
pub const SCENE_IMGS_FREE: u32 = 0x101D_775B;
pub const SCENE_TEXTS: u32 = 0x1018_4A5D;
pub const SCENE_TEXTS_FREE: u32 = 0x101D_777B;
// () -> the language file path (/NAND:A/bt_watch.<lang>)
pub const SCENE_LANG: u32 = 0x100D_FB89;
pub const SCENE_RES: u32 = 0x1020_3E91;
pub const SCENE_STY: u32 = 0x1020_3EA6;
pub const SPORT1_SCENE: u32 = 0x6880_AE34;
// sport1_view's 22 image ids (its preload 0x10078044) and its texts' ids
pub const SPORT1_IMGS: u32 = 0x101E_D460;
pub const SPORT1_TEXTS: u32 = 0x101E_D4FC;
// (lv_font_t *, path): the stock font open with its per-file tweaks: font32M +6 line height, sans32 gets the emoji
// fallback (0x100DF9A0). LVGL_BITMAP_FONT_CLOSE frees the fallback too.
pub const FONT_OPEN_STOCK: u32 = 0x100D_F9A1;
pub const FONT_SANS32: u32 = 0x1020_1DEE;
pub const FONT_32M: u32 = 0x1020_1DC4;
// vendor simple_img (create, set_src with an lv_img_dsc_t), as the stock tiles draw their icons
pub const SIMPLE_IMG_CREATE: u32 = 0x1005_A015;
pub const SIMPLE_IMG_SET_SRC: u32 = 0x1005_A02D;
pub const LV_OBJ_ALIGN_TO: u32 = 0x1012_A87D;
pub const LV_STYLE_ANIM_SPEED: u32 = 0x66;
// sport1_view's volume popup (0x100786DC): ux2sys 0x50 reads {max u8, current u8}; 0x26 vibrates ({8}, 4 B)
pub const PROP_VOLUME: u32 = 0x50;
pub const PROP_VIBRATE: u32 = 0x26;
pub const UX2SYS_SET_LEN: u32 = 0x100E_08B9;
// a pointer value (lv_img_dsc_t *): local style only through the loader, never STYLE
pub const LV_STYLE_BG_IMG_SRC: u32 = 0x28;
// (indev) -> gesture direction; (indev): wait for release, as the stock popup's gesture close does
pub const LV_INDEV_GET_GESTURE_DIR: u32 = 0x101C_4F7D;
pub const LV_INDEV_WAIT_RELEASE: u32 = 0x101C_4FD5;
// Watch face manager and data_storage: wfguard.rs (Device)
