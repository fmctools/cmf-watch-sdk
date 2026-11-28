# Keyboards and voice typing

Text input for any app, from the UI kit (`cmf-wp2-ui`). The demo is `sdk/examples/keyboards`:

```bash
scripts/sim keyboards                                  # on the Mac
scripts/build-app keyboards 1 && uv run wp2apps install target/keyboards.wp2app   # on a watch
```

| Module | What | Needs |
|---|---|---|
| `keyboard` | QWERTY laid out for the round panel (Wear OS style): letters, `?12` symbols, shift, delete (held: repeats), space, the action key; a capital after the end of a sentence; the typed text with a caret, scrolled by the crown. Number fields get a number pad; multiline fields a new-line key | nothing |
| `emoji` | the watch's own 52 emoji (the stock `emoji28.font`), in text and as the keyboard's second page | loader 6 (`emoji_read`); older: no emoji key, emoji draw as boxes |
| `dial` | the rotary keyboard: letters round the rim, the crown picks, a tap types, held a capital, delete held repeats | nothing; hiding the status bar over it needs the loader of 2026-11-28 |
| `dictate` | voice typing: the mic as IMA ADPCM to the phone, the words back as they come | loader 6 (mic) and a phone app that answers (below) |

## In an app

The app keeps its loop; the modules draw and say what to do next.

```rust
use cmf_wp2_ui::keyboard::{Act, Keyboard};
use cmf_wp2_ui::dictate::{Dictate, Out};

static mut EMOJI: [u8; cmf_wp2_ui::emoji::ram_for(18)] = [0; cmf_wp2_ui::emoji::ram_for(18)];

cmf_wp2_ui::emoji::init(api, unsafe { &mut *core::ptr::addr_of_mut!(EMOJI) });
let mut kb = Keyboard::new(ACCENT);
kb.input = Input::Text;        // or Input::Integer { signed }, Input::Decimal { signed }: a number pad
kb.action = Action::Send;      // the action key's label: Send (>), Done (ok), Next (next)
kb.multiline = false;          // true: the action key types a new line, a button left of the text submits
api.own_back(true);            // the button is the app's (back from the rotary keyboard), not the loader's escape
kb.draw(&mut surface, "Title");
// TOUCH_DOWN (TOUCH_UP: kb.release(), so a held delete stops)
match kb.tap(x, y, api.now()) {
    Act::Typed(k) => { /* kb.draw_text, kb.draw_key(k, true), plain again after FLASH_MS */ }
    Act::Text => kb.draw_text(&mut surface),
    Act::Redraw => kb.draw(&mut surface, "Title"),
    Act::Submit => use_the_text(kb.text()),  // the action key or the submit button: send, done, next field
    Act::Voice => dict.start(api, false),   // then dict.draw
    Act::Dial => { /* the rotary keyboard */ }
    _ => {}
}
```

- **Delete held**: wait at most `kb.hold_wait(now)` for events and call `kb.tick(now)` on every pass; it answers
  like a tap (`Typed(Del)`). A tap deletes one char (a whole emoji), held it repeats every 90 ms after 450 ms, the
  lift stops it.
- **Crown**: `kb.crown(d)` turns the emoji pages, or else scrolls the text a line (`Act::Text`); typing goes back
  to the caret.
- **Number fields**: digits, a minus first when `signed`, one point for decimals; anything else is ignored, voice
  typing included. No emoji, symbols, mic or rotary keyboard there.

- **Voice typing**: while `dict.active()`, wait at most `dict.wait_ms(...)` for events and call `dict.tick(api)`.
  Give every message to `dict.message(m)` first, touches to `dict.tap`, the button to `dict.cancel`. On
  `Out::Done`, `kb.add_words(dict.text())`.
- **Rotary keyboard**: `dial.down` on TOUCH_DOWN, `dial.up` on TOUCH_UP, `dial.crown` on CROWN, `dial.tick` with
  `dial.hold_wait` as a timeout (held letters become capitals, held delete repeats). Its letters come in their case:
  set `kb.shift = false` before `kb.type_key`, and `dial.shift = kb.shift` after. Draw the text inside its letters
  with `kb.draw_text_in(s, dial::TEXT_Y, dial::TEXT_ROWS, dial::TEXT_PX)`: that box only is cleared, so typing
  leaves the letters alone. Hide the status bar while it is up (`api.status_bar(false)`, `true` when leaving), and
  let the button go back to QWERTY first (with `own_back(true)` the app gets the press; without it the loader
  stops the app a second later).
- **Emoji memory**: `ram_for(n)` keeps `n` emoji at once (18 = a whole page, about 20 KB). An app short of room
  can give the icon's memory once it runs (`app_icon_ram()`), as Chats does.
- **PAUSE**: the loader stops the mic, so cancel dictation (`dict.cancel`).

## The phone side of voice typing

The watch sends `~` messages on the app's own channel, so one phone feature serves every app:

| Direction | Message | Meaning |
|---|---|---|
| watch → phone | `~1` | start dictating (`~3`: a recording the phone keeps) |
| watch → phone | `~a seq:u16 block` | 240 samples at 8 kHz as one IMA ADPCM block (`cmf_wp2_app::adpcm`) |
| watch → phone | `~0` / `~2` | the end / cancel |
| phone → watch | `~ kind:u8 text`, kind 0 | the words so far (kind is a byte 0, 1 or 2, not a digit) |
| phone → watch | kind 1 | the final words |
| phone → watch | kind 2 | a problem, shown on the watch |

The mic is `cmf_wp2_app::mic::Voice`: 1 or 2 channels detected, 8 kHz, an automatic gain (the real watch's mic is
quiet). With no phone connected the screen says so; with no answer for 10 s it gives up.

Phone apps: CMFX (iOS speech recognition, on the iPhone when it can) answers today. The FMC Companion does not yet.
On the Mac, `hosts/voicehost.py` stands in for the phone in the simulator:

```bash
scripts/sim keyboards --tcp 7798 &
uv run python hosts/voicehost.py --sim 7798 --words "coffee at ten" heard.wav
```

## Limits

- At most `TEXT_MAX` (100) bytes of text; emoji take 4 each.
- Before the loader of 2026-11-28 the status bar can't be hidden: the rotary keyboard's top letter sits under it.
