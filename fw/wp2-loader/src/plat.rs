//! The model this loader is built for: the Watch Pro 2 by default, the Watch 3 Pro with `--features wp3` (a feature
//! of this crate only, so building the stubs in the same invocation never unifies a model into wp2-core). Every stock
//! address, struct offset and id the loader uses comes through here: for WP2 the existing wp2-core modules, re-exported
//! unchanged (the WP2 build stays byte-identical); for WP3 the same names from `model::WP3` and wp2-core `port`.
//! Stock facts with no Watch 3 Pro value (`None` in the model) are 0 here and every user checks `HAS_*` first.
#![allow(dead_code)]

#[cfg(not(feature = "wp3"))]
pub use wp2_core::layout::stub_ram;
#[cfg(not(feature = "wp3"))]
pub use wp2_core::{fs, heap, load, stock};
#[cfg(not(feature = "wp3"))]
pub use wp2_core::chan::{listening, session_word};
#[cfg(not(feature = "wp3"))]
pub use wp2_core::events::{map_key, popup_covers};

#[cfg(not(feature = "wp3"))]
pub mod view {
    // the launcher's early-suspend / late-resume to the top view (docs/recon.md, "Screen-off and the app view")
    pub const SUSPEND: u32 = 0x82;
    pub const WAKE: u32 = 0x83;
    // the loader's wake lock: its view id
    pub const WAKE_ID: u32 = 0xF0;
    pub use wp2_core::cards::page_slot;
}

#[cfg(not(feature = "wp3"))]
pub mod obj {
    // the indev's lv_obj_t coords y1 / y2, tileview's scroll dir (lv.rs)
    pub const Y1: usize = 0x18;
    pub const Y2: usize = 0x20;
    pub const TILE_DIR: usize = 0x2C;
    pub const POPUP_FLAGS: usize = 0x3800_FAF4;
    // storage.rs: partition table pointer and the FAT ops vtable (docs/storage-inspection.md)
    pub const TABLE_PTR: usize = 0x3801_f1e8;
    pub const FAT_OPS: u32 = 0x101f_6ac0;
}

#[cfg(feature = "wp3")]
pub use wp3::*;

#[cfg(feature = "wp3")]
mod wp3 {
    use wp2_core::events::Event;
    use wp2_core::model::{Model, WP3};
    use wp2_core::port;

    pub const M: &Model = &WP3;

    const fn opt(v: Option<u32>) -> u32 {
        match v {
            Some(v) => v,
            None => 0,
        }
    }

    const fn req(v: Option<u32>) -> u32 {
        match v {
            Some(v) => v,
            None => panic!("WP3 model value missing"),
        }
    }

    /// wp2_core::stock's names with the Watch 3 Pro values (fw/wp2-core model.rs)
    pub mod stock {
        use super::{opt, M};
        pub use wp2_core::stock::{LV_STYLE_ANIM_SPEED, LV_STYLE_BG_COLOR, LV_STYLE_BG_IMG_SRC, LV_STYLE_BG_OPA, LV_STYLE_PAD_BOTTOM, LV_STYLE_PAD_LEFT, LV_STYLE_RADIUS, LV_STYLE_TEXT_ALIGN, LV_STYLE_TEXT_COLOR, LV_STYLE_TEXT_FONT, LV_STYLE_TEXT_OPA, LV_STYLE_TRANSLATE_X, LV_STYLE_WIDTH};
        pub const CARDS_SAVE: u32 = M.cards_save;
        pub const CARDS_STORE: u32 = M.cards_store;
        pub const CMU_PWMCLK: u32 = opt(M.cmu_pwmclk); // not on this model: 0, callers check
        pub const FONT_32M: u32 = M.font_32m;
        pub const FONT_OPEN_STOCK: u32 = M.font_open_stock;
        pub const FONT_SANS32: u32 = M.font_sans; // sans36 on this model
        pub const FS_CLOSEDIR: u32 = M.fs_closedir;
        pub const FS_FIND_MOUNT: u32 = M.fs_find_mount;
        pub const FS_OPENDIR: u32 = M.fs_opendir;
        pub const FS_READDIR: u32 = M.fs_readdir;
        pub const FS_STAT: u32 = M.fs_stat;
        pub const FS_STATVFS: u32 = M.fs_statvfs;
        pub const GPS_INFO: u32 = opt(M.gps_info); // not on this model: 0, callers check
        pub const GPS_STATE: u32 = opt(M.gps_state); // not on this model: 0, callers check
        pub const HR_BPM: u32 = opt(M.hr_bpm);
        pub const HR_OWNER: u32 = opt(M.hr_owner);
        pub const HR_WEAR: u32 = opt(M.hr_wear);
        pub const JX_SCROLL_BAR_CREATE: u32 = M.jx_scroll_bar_create;
        pub const JX_SCROLL_BAR_SET_KNOB: u32 = M.jx_scroll_bar_set_knob;
        pub const JX_SCROLL_BAR_SET_SCROLL_OBJ: u32 = M.jx_scroll_bar_set_scroll_obj;
        pub const KEY_BACK: u32 = opt(M.key_back);
        pub const K_THREAD_ABORT: u32 = M.k_thread_abort;
        pub const LVGL_BITMAP_FONT_CLOSE: u32 = M.lvgl_bitmap_font_close;
        pub const LVGL_BITMAP_FONT_OPEN: u32 = M.lvgl_bitmap_font_open;
        pub const LV_ARC_CREATE: u32 = M.lv_arc_create;
        pub const LV_ARC_GET_VALUE: u32 = opt(M.lv_arc_get_value); // not on this model: 0, callers check
        pub const LV_ARC_SET_BG_ANGLES: u32 = M.lv_arc_set_bg_angles;
        pub const LV_ARC_SET_ROTATION: u32 = M.lv_arc_set_rotation;
        pub const LV_ARC_SET_VALUE: u32 = M.lv_arc_set_value;
        pub const LV_BAR_CREATE: u32 = M.lv_bar_create;
        pub const LV_BAR_GET_VALUE: u32 = M.lv_bar_get_value;
        pub const LV_BAR_SET_RANGE: u32 = M.lv_bar_set_range;
        pub const LV_BAR_SET_VALUE: u32 = M.lv_bar_set_value;
        pub const LV_BTN_CREATE: u32 = M.lv_btn_create;
        pub const LV_DISP_ADD_KNOB_OBJ: u32 = M.lv_disp_add_knob_obj;
        pub const LV_DISP_GET_SCR_ACT: u32 = M.lv_disp_get_scr_act;
        pub const LV_EVENT_GET_CODE: u32 = M.lv_event_get_code;
        pub const LV_EVENT_GET_PARAM: u32 = M.lv_event_get_param;
        pub const LV_EVENT_GET_USER_DATA: u32 = M.lv_event_get_user_data;
        pub const LV_FONT_DEFAULT: u32 = M.lv_font_default;
        pub const LV_IMG_CREATE: u32 = M.lv_img_create;
        pub const LV_IMG_SET_SRC: u32 = M.lv_img_set_src;
        pub const LV_INDEV_GET_GESTURE_DIR: u32 = M.lv_indev_get_gesture_dir;
        pub const LV_INDEV_GET_POINT: u32 = M.lv_indev_get_point;
        pub const LV_INDEV_WAIT_RELEASE: u32 = M.lv_indev_wait_release;
        pub const LV_LABEL_CREATE: u32 = M.lv_label_create;
        pub const LV_LABEL_SET_LONG_MODE: u32 = M.lv_label_set_long_mode;
        pub const LV_LABEL_SET_TEXT: u32 = M.lv_label_set_text;
        pub const LV_OBJ_ADD_EVENT_CB: u32 = M.lv_obj_add_event_cb;
        pub const LV_OBJ_ADD_FLAG: u32 = M.lv_obj_add_flag;
        pub const LV_OBJ_ADD_STATE: u32 = M.lv_obj_add_state;
        pub const LV_OBJ_ALIGN: u32 = M.lv_obj_align;
        pub const LV_OBJ_ALIGN_TO: u32 = M.lv_obj_align_to;
        pub const LV_OBJ_CLEAR_FLAG: u32 = M.lv_obj_clear_flag;
        pub const LV_OBJ_CLEAR_STATE: u32 = M.lv_obj_clear_state;
        pub const LV_OBJ_CREATE: u32 = M.lv_obj_create;
        pub const LV_OBJ_DEL: u32 = M.lv_obj_del;
        pub const LV_OBJ_GET_CHILD: u32 = M.lv_obj_get_child;
        pub const LV_OBJ_GET_CHILD_CNT: u32 = M.lv_obj_get_child_cnt;
        pub const LV_OBJ_GET_SCROLL_BOTTOM: u32 = M.lv_obj_get_scroll_bottom;
        pub const LV_OBJ_GET_SCROLL_Y: u32 = M.lv_obj_get_scroll_y;
        pub const LV_OBJ_HAS_FLAG: u32 = M.lv_obj_has_flag;
        pub const LV_OBJ_INVALIDATE_AREA: u32 = M.lv_obj_invalidate_area;
        pub const LV_OBJ_SCROLL_TO_Y: u32 = M.lv_obj_scroll_to_y;
        pub const LV_OBJ_SET_FLEX_ALIGN: u32 = M.lv_obj_set_flex_align;
        pub const LV_OBJ_SET_FLEX_FLOW: u32 = M.lv_obj_set_flex_flow;
        pub const LV_OBJ_SET_LOCAL_STYLE_PROP: u32 = M.lv_obj_set_local_style_prop;
        pub const LV_OBJ_SET_POS: u32 = M.lv_obj_set_pos;
        pub const LV_OBJ_SET_SIZE: u32 = M.lv_obj_set_size;
        pub const LV_OBJ_SET_TILE_ID: u32 = M.lv_obj_set_tile_id;
        pub const LV_ROLLER_CREATE: u32 = M.lv_roller_create;
        pub const LV_ROLLER_GET_SELECTED: u32 = M.lv_roller_get_selected;
        pub const LV_ROLLER_SET_OPTIONS: u32 = M.lv_roller_set_options;
        pub const LV_ROLLER_SET_SELECTED: u32 = M.lv_roller_set_selected;
        pub const LV_SLIDER_CREATE: u32 = M.lv_slider_create;
        pub const LV_TILEVIEW_ADD_TILE: u32 = M.lv_tileview_add_tile;
        pub const LV_TILEVIEW_CREATE: u32 = M.lv_tileview_create;
        pub const LV_TILEVIEW_GET_TILE_ACT: u32 = M.lv_tileview_get_tile_act;
        pub const LV_TIMER_CREATE: u32 = M.lv_timer_create;
        pub const LV_TIMER_DEL: u32 = M.lv_timer_del;
        pub const OS_SLEEP: u32 = M.os_sleep;
        pub const OS_THREAD_CREATE: u32 = M.os_thread_create;
        pub const POPUP_FLAGS: u32 = M.popup_flags;
        pub const POWER_CAPACITY: u32 = M.power_capacity;
        pub const POWER_DC5V: u32 = M.power_dc5v;
        pub const POWER_STATUS: u32 = M.power_status;
        pub const PROP_GPS_MODE: u32 = M.prop_gps_mode;
        pub const PROP_HR_MODE: u32 = M.prop_hr_mode;
        pub const PROP_MEDIA_AVAILABLE: u32 = M.prop_media_available;
        pub const PROP_MEDIA_CMD: u32 = M.prop_media_cmd;
        pub const PROP_MEDIA_INFO: u32 = M.prop_media_info;
        pub const PROP_MEDIA_PLAYING: u32 = M.prop_media_playing;
        pub const PROP_PHONE_LINK: u32 = M.prop_phone_link;
        pub const PROP_VIBRATE: u32 = M.prop_vibrate;
        pub const PROP_VOLUME: u32 = M.prop_volume;
        pub const PWM_DEVICE: u32 = opt(M.pwm_device); // not on this model: 0, callers check
        pub const RETURN_CB: u32 = M.return_cb;
        pub const RETURN_CLOCK: u32 = M.return_clock;
        pub const RTC_GET_TIME: u32 = opt(M.rtc_get_time); // not on this model: 0, callers check
        pub const SCENE_CLOSE: u32 = M.scene_close;
        pub const SCENE_IMGS: u32 = M.scene_imgs;
        pub const SCENE_IMGS_FREE: u32 = M.scene_imgs_free;
        pub const SCENE_LANG: u32 = M.scene_lang;
        pub const SCENE_OPEN: u32 = M.scene_open;
        pub const SCENE_RES: u32 = M.scene_res;
        pub const SCENE_STY: u32 = M.scene_sty;
        pub const SCENE_TEXTS: u32 = M.scene_texts;
        pub const SCENE_TEXTS_FREE: u32 = M.scene_texts_free;
        pub const SIMPLE_IMG_CREATE: u32 = M.simple_img_create;
        pub const SIMPLE_IMG_SET_SRC: u32 = M.simple_img_set_src;
        pub const SPORT1_IMGS: u32 = opt(M.sport1_imgs); // not on this model: 0, callers check
        pub const SPORT1_SCENE: u32 = opt(M.sport1_scene); // not on this model: 0, callers check
        pub const SPORT1_TEXTS: u32 = opt(M.sport1_texts); // not on this model: 0, callers check
        pub const SYS_CLOCK_TICK_GET: u32 = M.sys_clock_tick_get;
        pub const TIME_ZONE: u32 = opt(M.time_zone); // not on this model: 0, callers check
        pub const UX2SYS_GET: u32 = M.ux2sys_get;
        pub const UX2SYS_SET: u32 = M.ux2sys_set;
        pub const UX2SYS_SET_LEN: u32 = M.ux2sys_set_len;
        pub const VIEW_LAYOUT: u32 = M.view_layout;
        pub const WAKE_LOCK: u32 = M.wake_lock;
        pub const WAKE_UNLOCK: u32 = M.wake_unlock;
    }

    pub mod view {
        use super::{req, M};
        pub const SUSPEND: u32 = req(M.view_msg_suspend);
        pub const WAKE: u32 = req(M.view_msg_wake);
        // the menu view id, a free wake-lock id (model.rs menu_view)
        pub const WAKE_ID: u32 = M.menu_view;

        /// App pages (views from `page_view_base`) need view-table records the M2 stub does not add yet (M3).
        pub fn page_slot(_view_id: u32) -> Option<u8> {
            None
        }
    }

    pub mod obj {
        use super::M;
        pub const Y1: usize = M.lv_obj_y1 as usize;
        pub const Y2: usize = M.lv_obj_y2 as usize;
        pub const TILE_DIR: usize = M.tile_act as usize;
        pub const POPUP_FLAGS: usize = M.popup_flags as usize;
        pub const TABLE_PTR: usize = M.part_table_ptr as usize;
        pub const FAT_OPS: u32 = M.fat_ops;
        /// i16 value of an lv_arc (this image has no lv_arc_get_value)
        pub const ARC_VALUE: usize = M.arc_value as usize;
    }

    /// What an ABI call with no Watch 3 Pro implementation yet returns (motor, GPS, heart rate, phone media, the
    /// music tile): -19 (ENODEV). Each is a milestone M3 item.
    pub const E_NODEV: i32 = -19;
    const E_IO: i32 = -5;

    /// ABI `time` on the UTC API (`utc_now`, `utc_zone_offset`): this image has no local-time RTC getter. The local
    /// date is computed here from UTC seconds + the zone offset (weekday 0 = Sunday, millis 0); the offset is real.
    pub fn time(out: *mut wp2_core::events::Time) -> i32 {
        if out.is_null() {
            return E_IO;
        }
        let now: extern "C" fn() -> u32 = unsafe { core::mem::transmute(req(M.utc_now) as usize) };
        let zone: extern "C" fn(u32, u32) -> i32 = unsafe { core::mem::transmute(req(M.utc_zone_offset) as usize) };
        let ts = now();
        // before 2020-01-01 the clock was never set
        if ts < 1_577_836_800 {
            return E_IO;
        }
        let offset = zone(ts, 0);
        let offset = if offset.abs() <= 14 * 3600 { offset } else { 0 };
        let t = wp2_core::port::civil(i64::from(ts) + i64::from(offset));
        let utc_offset_min = (offset / 60) as i16;
        unsafe { out.write(wp2_core::events::Time { utc_offset_min, ..t }) };
        0
    }

    pub fn stub_ram() -> &'static mut wp2_core::layout::StubRam {
        port::stub_ram(M)
    }

    pub fn session_word() -> u32 {
        unsafe { (M.session_key as *const u32).read_unaligned() }
    }

    // As wp2_core::chan::listening: the stream is connected (wewear ctx +4 bit 0) and the session word is the host's.
    pub fn listening(host_key: u32) -> bool {
        let ctx = unsafe { (M.wewear_ctx as *const u32).read_volatile() };
        host_key != 0 && ctx != 0 && unsafe { ((ctx + 4) as *const u8).read_volatile() } & 1 != 0
            && session_word() == host_key
    }

    /// As wp2_core::events::map_key with this model's codes: knob 0x1E clockwise (next), 0x1F anticlockwise.
    pub fn map_key(value: u32) -> Option<Event> {
        use wp2_core::events::{BUTTON, CROWN};
        let (kind, delta) = match value {
            v if v == req(M.key_crown_cw) => (CROWN, 1),
            v if v == req(M.key_crown_ccw) => (CROWN, -1),
            v if v == req(M.key_crown_press) => (BUTTON, 0),
            v if v & 0xFFFF == req(M.key_back) => (BUTTON, 0),
            _ => return None,
        };
        Some(Event { kind, delta, ..Event::default() })
    }

    /// As wp2_core::events::popup_covers with this model's AOD msgbox id.
    pub fn popup_covers(flags: u8, ids: [u16; 2]) -> bool {
        let aod = req(M.aod_msgbox) as u16;
        (flags & 1 != 0 && ids[0] != aod) || (flags & 2 != 0 && ids[1] != aod)
    }

    pub mod fs {
        use super::M;
        use wp2_core::port;
        pub use wp2_core::port::{File, CREATE, READ, WRITE};

        pub fn open(file: &mut File, path: &[u8], flags: u8) -> i32 {
            port::open(M, file, path, flags)
        }
        pub fn read(file: &mut File, buf: &mut [u8]) -> i32 {
            port::read(M, file, buf)
        }
        pub fn write(file: &mut File, buf: &[u8]) -> i32 {
            port::write(M, file, buf)
        }
        pub fn seek(file: &mut File, offset: u32) -> i32 {
            port::seek(M, file, offset)
        }
        pub fn close(file: &mut File) -> i32 {
            port::close(M, file)
        }
        pub fn unlink(path: &[u8]) -> i32 {
            port::unlink(M, path)
        }
        pub fn rename(from: &[u8], to: &[u8]) -> i32 {
            port::rename(M, from, to)
        }
        pub fn mkdir(path: &[u8]) -> i32 {
            port::mkdir(M, path)
        }
        pub fn read_exact(file: &mut File, buf: &mut [u8]) -> bool {
            port::read_exact(M, file, buf)
        }
    }

    pub mod heap {
        use super::M;
        pub use wp2_core::heap::{FB, RES};

        pub fn alloc(ty: u32, size: u32, align: u32) -> *mut u8 {
            wp2_core::port::alloc(M, ty, size, align)
        }

        /// # Safety
        /// `p` is NULL or a block of type `ty` from `alloc`.
        pub unsafe fn free(ty: u32, p: *mut u8) {
            unsafe { wp2_core::port::free(M, ty, p) }
        }
    }

    pub mod load {
        use super::M;
        use wp2_core::pkg::Header;
        use wp2_core::port::File;

        pub fn load(slot: u8, file: &mut File, scratch: &mut [u8]) -> Result<(*mut u8, Header), i32> {
            wp2_core::port::load(M, slot, file, scratch)
        }

        pub fn sync_psram(block: *mut u8, len: u32) {
            wp2_core::port::sync_psram(M, block, len)
        }
    }
}
