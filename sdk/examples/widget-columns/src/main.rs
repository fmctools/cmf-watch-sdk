//! A `columns` widget card with the app's own icons (stub 10, docs/app-guide.md "Widget"): Now and the next two hours
//! of a made-up forecast. The crown shifts the hours; back exits. On an older stub the text card is the fallback.
#![no_std]
#![no_main]

use cmf_wp2_app::{Api, Column, BUTTON, CROWN};

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_features!(cmf_wp2_app::WIDGET);

/// icon indices = widget-icons/ in file name order
const SUN: u8 = 0;
const CLOUD: u8 = 1;
const RAIN: u8 = 2;
const HOURS: [(&str, u8, &str); 6] =
    [("09:00", SUN, "21°"), ("10:00", CLOUD, "19°"), ("11:00", RAIN, "-3°"), ("12:00", SUN, "24°"), ("13:00", CLOUD, "18°"), ("14:00", RAIN, "15°")];

/// the same values for the text card: its font has only `º` (U+00BA) and the stub replaces `°` in `columns` alone
const TEXT: [&str; 6] = ["21º", "19º", "-3º", "24º", "18º", "15º"];

fn card(api: &Api, at: usize) -> i32 {
    let col = |i: usize, caption| {
        let (_, icon, value) = HOURS[(at + i) % HOURS.len()];
        Column { caption, icon: Some(icon), value }
    };
    let rc = api.widget_columns(&[col(0, "Now"), col(1, HOURS[(at + 1) % HOURS.len()].0), col(2, HOURS[(at + 2) % HOURS.len()].0)]);
    if rc == -22 {
        api.widget("Forecast", TEXT[at]);
    }
    rc
}

fn main(api: &'static Api) -> i32 {
    let mut at = 0;
    let mut rc = card(api, at);
    loop {
        api.rect(0, 0, api.width, api.height, 0);
        let text = if rc == 0 { "columns card set" } else { "text card (old stub)" };
        api.text(api.width as i16 / 2 - 8 * text.len() as i16, api.height as i16 / 2 - 8, text, 0xFFFF);
        api.show();
        let Some(event) = api.event(u32::MAX) else { continue };
        match event.kind {
            BUTTON => return 0,
            CROWN => {
                at = (at + 1) % HOURS.len();
                rc = card(api, at);
            }
            _ => {}
        }
    }
}
