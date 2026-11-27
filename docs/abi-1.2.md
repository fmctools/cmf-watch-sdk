# App ABI 1.2: LVGL apps

Status: implemented, 2026-11-11. JX402 `1.0.0.73` only. Design: `docs/specs/2026-11-11-lvgl-apps-design.md`; every
stock entry used here is in `docs/recon-lvgl.md`, "Verified for ABI 1.2". The emulator proof is
`tests/test_lv_e2e.py` (app `sdk/examples/lv-demo`). Not yet run on a real watch: installing the 1.2 firmware or
loader on a physical watch needs the owner's yes (`AGENTS.md`, hard rule 1). LVGL apps do not run in the simulator
yet (`lv_submit` returns `-95` there).

An app keeps its own thread and event loop. It never calls LVGL: it fills a command buffer, calls
`api.lv_submit(buf, len)`, and the loader runs the buffer on `ui_service`, the only thread that may touch LVGL.
Everything below is the contract of that call.

## Compatibility

- `Api.abi` stays 1, `Api.abi_minor` becomes 2. `lv_submit` follows `screen_on`; the ABI 1.0 and 1.1 fields keep
  their offsets (`sdk/cmf-wp2-app/src/lib.rs`). An older app never reads it.
- `LOADER_ABI_MINOR = 2` in the stub and the loader (`fw/wp2-core/src/layout.rs`). The stub refuses packages whose
  `abi_minor` is above what it knows, so the firmware is rebuilt (`scripts/build-fw`). An app built with the 1.2
  SDK needs a 1.2 loader.
- `Api::lv() -> Option<Lv>` is `None` when `abi_minor < 2`; the SDK never reads the `lv_submit` field then. The
  simulator fills the 1.2 table with a function that returns `-95`.
- Pixel apps (`cmf-wp2-ui`, `rect`/`text`/`show`) are unchanged and still the kit for the simulator.
- The event queue's `head` is now a word with a generation (`fw/wp2-core/src/events.rs`); not visible to apps.

## The call

`lv_submit(buf: *const u8, len: usize) -> i32`, from the app thread only. It blocks until the UI thread has run the
batch. The UI thread polls every 10 ms, so **each submit waits up to one tick**: build a screen in one batch, not
one record per call. A submit from a stack buffer is refused (`-1`): `buf..buf+len` must lie inside the app's own
block, so use a `static` (the SDK's `Batch` does).

| Return | Meaning |
|---|---|
| `0` | the whole batch ran |
| `-1` | `buf` null, `len > 2048`, or the range is outside the app block |
| `-22` | malformed or invalid batch. The whole batch is checked first (structure, lengths, handle states, class of each target, whitelisted style ids, selectors and flags, open font slots); **nothing is executed** |
| `-12` | the batch would create the 193rd live object. Nothing is executed |
| `-2` | a font file failed to open. The batch stops there and earlier records stay applied; the slot holds the built-in font. The SDK opens fonts in a batch of their own |
| `-95` | the simulator's `lv_submit` (never the loader). With a loader older than 1.2, `api.lv()` is `None` and nothing is called; lv-demo then returns `-95` from `main` on its own |

Sources: `fw/wp2-core/src/lvcmd.rs` (`Error::code`, `validate`), `fw/wp2-loader/src/lv.rs` (`submit`, `BATCH_MAX`).

## Command records

Little-endian, back to back: `op: u8, len: u8, handle: u16, payload[len]`. The `len` must match the op's payload
exactly (TEXT and OPTIONS take their string). Handles are `u16`, chosen by the app: `1..=192`, handle `0` is the
root. A handle must be free to be created and live to be used. The SDK (`sdk/cmf-wp2-app/src/lv.rs`, `Batch`)
picks the lowest free handle and keeps the same table as the loader.

| op | name | payload | LVGL call | Checks |
|---|---|---|---|---|
| 0x01 | CREATE | `class u8, parent u16`; classes 0 obj, 1 label, 2 btn, 3 arc, 4 bar, 5 slider, 6 roller, 12 image (a bitmap over the app's own pixels; at most 8 alive) | `lv_<class>_create(parent)` | handle free; parent live and not a scroll bar; class 7 is refused |
| 0x02 | DEL | – | `lv_obj_del` | not the root, **not a scroll bar** (`BadClass`; a bar goes with its target, which keeps callbacks into it). Frees the handle, every descendant and the scroll bars that target a freed object |
| 0x03 | SIZE | `w i32, h i32` raw `lv_coord_t` | `lv_obj_set_size` | not the root. `LV_PCT(n)` = `0x2000_0000 \| n`, `LV_SIZE_CONTENT` = `0x2000_07D1` (SDK: `pct`, `CONTENT`) |
| 0x04 | ALIGN | `align u8, x i16, y i16` | `lv_obj_align` | not the root; `align` 0..=21 |
| 0x05 | FLAGS | `set u32, clear u32` | `lv_obj_add_flag`, then `lv_obj_clear_flag` | bits ⊆ HIDDEN 1<<0, CLICKABLE 1<<1, CHECKABLE 1<<3, SCROLLABLE 1<<4, SCROLL_ELASTIC 1<<5, SCROLL_MOMENTUM 1<<6 (mask `0x7B`) |
| 0x06 | STYLE | `prop u16, selector u32, value i32` | `lv_obj_set_local_style_prop` | `prop` in the **42** scalar ids of the 48 stock setters (`wp2_core::lvcmd::ACCEPTED_PROPS`, SDK `Prop`): the image sources 40 and 84 (pointers) and the unproven Actions ids 45, 92, 93 and 102 are refused. `TEXT_FONT` takes a font slot that is open. The selector is `lv_style_selector_t`: part (bits 16..) one of MAIN 0, SCROLLBAR 1, INDICATOR 2, KNOB 3, SELECTED 4, ITEMS 5; state (low 16 bits) one of DEFAULT 0, CHECKED `0x01`, FOCUSED `0x02`, PRESSED `0x20`, DISABLED `0x80` (no combined states). That keeps an object under LVGL's 63 local styles |
| 0x07 | FONT | `slot u8 (0..4), font u8 (0..16)`; handle 0 | `lvgl_bitmap_font_open` into a loader-owned `lv_font_t` | a slot that is already open is closed first and reopened in place. Nothing refreshes the objects styled with it, so their size keeps the old font's metrics until they are laid out again: reopen a slot before styling with it, not under live labels |
| 0x08 | TEXT | UTF-8, ≤ 128 B | `lv_label_set_text` (copies) | label only |
| 0x09 | VALUE | `v i32, anim u8` | `lv_arc_set_value`, `lv_bar_set_value` (also slider), `lv_roller_set_selected` | arc `v` fits `i16`; roller `v` fits `u16` and is clamped to the option count (skipped while OPTIONS was never set) |
| 0x0A | RANGE | `min i32, max i32` | `lv_bar_set_range` | **bar and slider only**, `min < max` |
| 0x0B | OPTIONS | `mode u8 (0 normal, 1 infinite)`, then `\n`-separated UTF-8, ≤ 240 B | `lv_roller_set_options` (copies) | roller only |
| 0x0C | EVENTS | `mask u8`: PRESSED bit 0, CLICKED 1, LONG_PRESSED 2, RELEASED 3, VALUE_CHANGED 4 | `lv_obj_add_event_cb` with the loader's trampoline | not the root. Only bits not registered before add a callback, so repeating a mask never duplicates events |
| 0x0D | FLEX | `flow u8, main u8, cross u8, track u8` | `lv_obj_set_flex_flow`, `lv_obj_set_flex_align` | flow ROW 0, COLUMN 1, +WRAP 4, +REVERSE 8 (no bit 2); aligns 0..=5 (START, END, CENTER, SPACE_EVENLY, SPACE_AROUND, SPACE_BETWEEN) |
| 0x0E | SCROLLBAR | `target u16`; the handle is the new bar | `jx_scroll_bar_create(root)`, `set_scroll_obj(target)`, `set_knob(1)` | handle free; target live and not a bar. The bar is a child of the root, so it is deleted after its target |
| 0x0F | KNOB | – | see "Crown" | not a scroll bar |
| 0x10 | ANGLES | `bg_start u16, bg_end u16, rotation u16` degrees | `lv_arc_set_bg_angles`, `lv_arc_set_rotation` | arc only, each ≤ 360 |
| 0x11 | FOCUS | `icon u16` (px, 0 = none) | callbacks on SCROLL 11 and LAYOUT_CHANGED 43 that apply the stock app list's fisheye (`lvcmd::focus_shape`, `docs/recon.md`) to each child: `PAD_LEFT` = shift, `TEXT_OPA`, child 0 resized to `icon · zoom / 256` and moved right by half of what it lost (`TRANSLATE_X`, a loader from 2026-11-16 on), so it zooms around its centre like the stock icon; the child nearest the centre line gets `LV_STATE_CHECKED` and the app gets `Event { kind: LV, x: handle, y: 100 (FOCUSED), delta: index }` when it changes | an `obj`, not the root; `icon ≤ 466`. One container per app (a new FOCUS replaces it), 32 children at most. Added 2026-11-12: an earlier 1.2 loader answers `-22` |
| 0x12 | LONG_MODE | `mode u8` (WRAP 0, DOT 1, SCROLL 2, SCROLL_CIRCULAR 3, CLIP 4) | `lv_label_set_long_mode` | label only. Added 2026-11-12 |
| 0x19 | IMAGE_SRC | `ptr u32, w u16, h u16, cf u8` (4 = RGB565) | builds an `lv_img_dsc_t` the loader keeps per handle, then `lv_img_set_src` | an image object (CREATE class 12) only; 1..=466 per side; `ptr` + `2·w·h` must lie inside the running app's image (checked before validation runs, like the batch buffer itself); the loader syncs the PSRAM cache, so re-submit after changing the pixels. Added 2026-11-27: an earlier loader answers `-22` |

### Deviations from the design spec

- **`STYLE_PROPS` has 48 ids, not 50.** The image has 44 setters that tail-call `lv_obj_set_local_style_prop`
  plus the 4 flex setters; the "50" of the first recon was a miscount (`docs/recon-lvgl.md`, "Verified for ABI 1.2").
  STYLE accepts 42 of them: the spec's "no wild pointer" rules out the image sources and the ids of unknown type.
- **The arc range is fixed 0..100.** `lv_arc_set_range` is absent from the firmware, so **RANGE works for bar and
  slider only**, and arc values are `i16`. `VALUE` on an arc clamps to 0..100 inside LVGL.
- **`VALUE_CHANGED` coalesces per (handle, event code)**, not per handle (below).

### Classes and parts

- `obj`, `label`, `btn`, `arc`, `bar`, `slider`, `roller`, `image` (bitmap over app pixels, docs/bitmap-assets.md),
  plus the vendor round scroll bar. The image has no switch,
  checkbox, list, msgbox, dropdown, textarea, keyboard or spinner (`docs/recon-lvgl.md`); compose them.
- **There is no theme.** The firmware styles nothing, so a widget without explicit styles for its parts is invisible
  or bare: a slider has no track, indicator or knob, an arc no arc, a button no background. Style each part with
  `STYLE` and the part in the selector (`LV_PART_INDICATOR 0x02_0000`, `LV_PART_KNOB 0x03_0000`,
  `LV_PART_SELECTED 0x04_0000`). `sdk/examples/lv-demo/src/screen.rs` is the reference (its e2e fails when the slider
  or arc indicator is not drawn). Text colour and font are inherited: set them once on `ROOT`.
- Colours are RGB565 values in the `i32` (`0x07FF`); `LV_RADIUS_CIRCLE` is `0x7FFF`.

## Fonts

One `FONT` record opens a stock NAND font into one of 4 slots; `STYLE` `TEXT_FONT` takes the slot. Fonts are
`/NAND:A/font<name>.font`, index in this order (`fw/wp2-loader/src/lv.rs`, `FONT_NAMES`; SDK `StockFont`):
`18M 24L 24M 24R 28M 28R 30R 32M 32R 34R 36L 36M 36N 36R 44L 48M`. Open fonts in their own batch (a failed
open returns `-2`). Fonts the app does not open use the built-in font. Each open font costs GUI heap; lv-demo opens one.

## Budgets

| | |
|---|---|
| live objects | 192 (`MAX_OBJS`), not counting the root; the 193rd gives `-12` |
| font slots | 4 (`MAX_FONTS`) |
| label text | ≤ 128 B (`MAX_TEXT`) |
| roller options | ≤ 240 B (`MAX_OPTIONS`) |
| batch | ≤ 2048 B; one record ≤ 4 + 255 B |
| GUI heap | lv-demo's screen (28 objects, one font, a scroll bar; stock look since 2026-11-12) used 6 868 B of the GUI heap: menu 94 728 B free, with the app 87 860 B (`tests/test_lv_e2e.py`). The design budget is 32–40 KB of objects (`docs/recon-lvgl.md`); an LVGL assert or heap exhaustion resets the watch |

## Events

`Event { kind: LV = 8, x: handle, y: LVGL event code, delta: value }`, in the same 15-slot queue as the ABI 1 events.
A subscribed object (`EVENTS`) reports `PRESSED` 1, `CLICKED` 7, `LONG_PRESSED` 5, `RELEASED` 8, `VALUE_CHANGED` 28.
For `VALUE_CHANGED` `delta` is the arc, bar, slider or roller value (clamped to `i16`).

- **`VALUE_CHANGED` coalesces.** A pending event with the same handle and the same code is replaced in place, so a
  slider drag leaves one event with the latest value, never a queue full of old ones. Other codes are never
  merged (`Queue::coalesce` in `fw/wp2-core/src/events.rs`; a race with `pop` is closed by the generation in `head`).
- `BUTTON`, `PAUSE`, `RESUME` and `MESSAGE` keep their ABI 1 meaning. Events are dropped while no app runs or the app is paused.
- `TOUCH_DOWN`/`TOUCH_UP` on the root's background keep their ABI 1 meaning (the root's pressed and released callbacks are the loader's input callback).

### Crown

- Without `KNOB`, the crown goes to the root and the app gets `CROWN ±1`.
- `KNOB` on an object **that is scrollable** sets flag `1 << 30` and makes it the current knob (`lv_disp_add_knob_obj`
  on an already registered object makes it current, commit `19803c3`): the crown scrolls it. `SCROLLBAR` sets the
  same flag on its target, so a list with a scroll bar is scrolled by the crown without a `KNOB`, unless
  something else took the knob afterwards.
- `KNOB` on any other object registers a knob filter once: the app gets `CROWN ±1` for turns over that object.
- **A plain `obj` is scrollable by default** (LVGL 8). `KNOB(panel)` meant to bring `CROWN` events then scrolls the
  panel instead (often with nothing to scroll), and the app gets no `CROWN`. Clear `SCROLLABLE` with `FLAGS` first.
- **Creating a roller makes it the knob** (the stock constructor sets `1 << 30` on itself): send `KNOB` to the
  object that should take the crown after the roller is created (`KNOB(list)` last).
- **A rightward drag across a slider is the stock back gesture**: the app gets `BUTTON` and has 1 s to leave
  (`docs/app-guide.md`, section 1, "Install and run"). A leftward drag works. Design sliders for taps and left drags.

## Lifetime

The loader owns every LVGL object and font.

- The first submit creates the root: `lv_obj_create(screen)`, 466×466, black, opaque, not scrollable, clickable. It
  hides the framebuffer image and takes the knob. Handle 0 is this root.
- Exit, crash, force stop and view delete all end in `teardown`: the thread is aborted, the root is deleted (and with
  it every app object), the open fonts are closed, the handle, event and option tables are cleared, and the mailbox
  is idle again. This runs before the app block is freed. A submit that is waiting at that moment is released.
- Nothing can leak into the GUI heap. Measured with lv-demo: menu 94 732 B free; after BUTTON 94 728 B; after a force
  stop in the middle of a submit loop 94 728 B (a 4 B difference after the first launch only, `tests/test_lv_e2e.py`).
- `DEL` of a subtree frees it at once: lv-demo's list (26 objects with the rows, labels and scroll bar) gives back
  5 764 B, and building it again lands within 8 B of the first build (`test_del_list`).
- A stuck app is stopped as in ABI 1: 1 s after `BUTTON`. Handle `BUTTON` in the event loop.

## What an app must not rely on

- A handle table that is out of step: after a `-22` the SDK `Batch` has already recorded the records, so its handles
  differ from the loader's. Treat a `Code` error from `submit` as a bug, not as a case to recover from.
- A style, flag or event the lists above do not name. The validator refuses it with `-22`.
- Anything on the simulator or on a loader older than 1.2.

## Emulator limits

- A visible LVGL object over the default stock face made LVGL redraw the face's rotated images through `sw_rotate`,
  which ends in a ROM API QEMU lacks (`unsupported API 0x524f4d`, `docs/recon-lvgl.md`). That was seen in the recon
  probe; the loader's own view and `tests/test_lv_e2e.py` never hit it (no `unsupported API` in any run).
- Colours in emulator frames read at 160/255 brightness (white is (160, 160, 160)); compare hue.
