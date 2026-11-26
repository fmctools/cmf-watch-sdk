#![no_std]
#![no_main]

use cmf_wp2_app::{Api, BUTTON, CROWN, PAUSE, RESUME};

cmf_wp2_app::entry!(main);
cmf_wp2_app::bitmaps!();

use bitmaps::{GRADIENT, RING};

const PAGES: usize = 2;

fn centre(text: &str) -> i16 {
    233 - 8 * text.len() as i16
}

fn draw(api: &Api, page: usize) {
    api.rect(0, 0, api.width, api.height, 0);
    api.text(centre("Bitmap assets"), 60, "Bitmap assets", 0xFFFF);
    let (b, caption) = match page {
        0 => (RING, "ring.png"),
        _ => (GRADIENT, "gradient.png"),
    };
    if b.ok() {
        // pixels: one copy on the watch, spans of rect on an older loader or in the simulator
        api.pixels(&b.px, 233 - b.w as i16 / 2, 160 - b.h as i16 / 2, b.w, b.h);
        api.text(centre(caption), 240, caption, 0x8410);
    } else {
        api.text(centre("bad bitmap"), 160, "bad bitmap", 0xF800);
    }
    api.show();
}

fn main(api: &'static Api) -> i32 {
    let mut page = 0;
    draw(api, page);
    loop {
        let Some(event) = api.event(u32::MAX) else { continue };
        match event.kind {
            BUTTON => return 0,
            CROWN => {
                page = (page + 1) % PAGES;
                draw(api, page);
            }
            RESUME => draw(api, page),
            PAUSE => loop {
                match api.event(u32::MAX) {
                    Some(e) if e.kind == RESUME => break,
                    Some(e) if e.kind == BUTTON => return 0,
                    _ => {}
                }
            },
            _ => {}
        }
    }
}
