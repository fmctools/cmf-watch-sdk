#![no_std]
#![no_main]

use bitmap_lv::build;
use cmf_wp2_app::lv::*;
use cmf_wp2_app::{stock, Api, BUTTON, PAUSE, RESUME};

cmf_wp2_app::entry!(main);

fn main(api: &'static Api) -> i32 {
    let Some(lv) = api.lv() else { return -95 };
    let Some(mut b) = Batch::new() else { return -1 };
    if stock::font(&mut b).and_then(|_| b.submit(&lv)).is_err() {
        return 1;
    }
    if build(&mut b).is_err() {
        // an older loader refuses IMAGE_SRC (-22) or a budget ran out: say so instead of a bare screen
        b.clear();
        if let Ok((page, _bar)) = stock::page(&mut b) {
            let _ = stock::title(&mut b, page, "Bitmap");
            if let Ok(label) = b.create(Class::Label, page) {
                let _ = b.text(label, "needs a bitmap loader");
            }
            let _ = b.submit(&lv);
        }
        loop {
            match api.event(u32::MAX) {
                Some(e) if e.kind == BUTTON => return 0,
                Some(e) if e.kind == RESUME => {}
                Some(e) if e.kind == PAUSE => continue,
                _ => {}
            }
        }
    }
    if b.submit(&lv).is_err() {
        return 3;
    }
    loop {
        match api.event(u32::MAX) {
            Some(e) if e.kind == BUTTON => return 0,
            _ => {}
        }
    }
}
