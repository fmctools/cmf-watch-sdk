#![no_std]
#![no_main]

use bitmap_stream_protocol::{parse, segments, Msg};
use cmf_wp2_app::{Api, BUTTON, MESSAGE, PAUSE, RESUME};
use core::ptr::addr_of_mut;

cmf_wp2_app::entry!(main);

#[repr(align(2))]
struct Aligned([u8; 128]);

// The receive buffer: one host message (MSG_MAX 128), 2-aligned so DATA payloads read as RGB565 words
static mut BUF: Aligned = Aligned([0; 128]);

fn centre(text: &str) -> i16 {
    233 - 8 * text.len() as i16
}

fn status(api: &Api, text: &str, colour: u16) {
    api.rect(0, 0, api.width, api.height, 0);
    api.text(centre(text), 224, text, colour);
    api.show();
}

fn main(api: &'static Api) -> i32 {
    let mut receiving: Option<(u16, u16)> = None;
    status(api, "send an image", 0x8410);
    loop {
        let Some(event) = api.event(u32::MAX) else { continue };
        match event.kind {
            BUTTON => return 0,
            MESSAGE => {
                let buf = unsafe { &mut *addr_of_mut!(BUF.0) };
                while let Some(m) = api.recv(buf) {
                    match parse(m) {
                        Some(Msg::Start { w, h }) => {
                            receiving = Some((w, h));
                            status(api, "receiving", 0x07FF);
                        }
                        Some(Msg::Data { offset, payload }) => {
                            let Some((w, h)) = receiving else { continue };
                            // centre it: the panel is a disc, so a picture tucked into the corner (0,0) would be
                            // cut away entirely (app-guide "Safe area")
                            let (ox, oy) = ((api.width - w) / 2, (api.height - h) / 2);
                            // payload starts at byte 6 of a 2-aligned buffer: aligned RGB565 words
                            let words = unsafe {
                                core::slice::from_raw_parts(payload.as_ptr() as *const u16, payload.len() / 2)
                            };
                            // Progressive reveal: every chunk goes straight into the framebuffer (no image buffer),
                            // so the whole streamed picture costs one 466x466 screen at most.
                            for (x, y, n, src) in segments(offset, payload.len(), w as usize, h as usize) {
                                api.pixels(&words[src..src + n], (ox + x as u16) as i16, (oy + y as u16) as i16,
                                           n as u16, 1);
                            }
                            api.show();
                        }
                        Some(Msg::Done) if receiving.is_some() => {
                            // keep the picture: only the status line at the bottom changes
                            api.rect(0, 216, api.width, 24, 0);
                            status_line(api, "done", 0x3B69);
                            api.show();
                        }
                        _ => {}
                    }
                }
            }
            RESUME => {}  // the loader repainted the framebuffer itself
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

fn status_line(api: &Api, text: &str, colour: u16) {
    api.text(centre(text), 224, text, colour);
}
