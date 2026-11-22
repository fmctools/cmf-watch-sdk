# Widget and start blob from the phone (contract for the phone)

The phone sets an app's widget card text, and a small blob the app gets when it starts, without the app running.
One new stub command, `PUSH` (`0x90A8`), from **stub 8** on (the same OTA as 16 slots). The stub writes the same
NAND files the app would: the widget card shows the text the next time the cards are laid out.

## Why the stub

The loader is in memory only while its view (the Apps menu or an app page) lives; a widget card or a closed menu
does not load it, so it cannot answer the phone. The stub always runs and already owns the K channel and the app's
files (`fw/wp2-stub/src/ble.rs`, `push`).

## The command

Sent like every K frame (`docs/abi-1.4.md`, "Writing a host"): `c1 = FFFF`, `c2 = 90A8`, reply `c2 = B0A8`.

Request plaintext: `slot:u8 ‖ kind:u8 ‖ data`

| `kind` | `data` | Stored as |
|---|---|---|
| `00` widget | a v1 record (exactly 64 bytes), from stub 9 also a well-formed v2 record (up to 128 bytes), or empty to remove | `/NAND:/wp2apps/<d>.wgt`, the card's record |
| `01` blob | 1..128 bytes, or empty to remove | `/NAND:/wp2apps/<d>.psh` |

`slot` is `0..15` (`<d>` its hex digit, `docs/abi-1.4.md`, "16 app slots"). The slot must hold an app package.

Widget record (the same bytes as the SDK's `api.widget(title, value)`, `widget_record` in `sdk/cmf-wp2-app`):

```text
offset  0: title, UTF-8, at most 23 bytes, then 00 up to offset 24
offset 24: value, UTF-8, at most 39 bytes, then 00 up to offset 64; 0A starts a second line
```

Cut long text at a UTF-8 character boundary. The card shows the title grey, the value white; a line wider than the
tile ends in `…`. Removing the record shows the package name again.

### v2 records (stub 9)

A record whose first byte is `FF` (never the start of UTF-8) is v2: `FF ‖ template:u8 ‖ fields`, at most 128 bytes,
the exact length of its template:

| template | fields | length |
|---|---|---|
| `00` text | `title[24] ‖ value[40]` as v1 | 66 |
| `01` columns | `n:u8` (1..4), then `n × { caption[10] ‖ icon:u8 ‖ value[10] }` | 3 + 21·n (24, 45, 66, 87) |

Text fields are UTF-8, at most 9 (caption, value) or 23/39 (text) bytes, then NUL up to the end of the field. `icon`
indexes the package's widget icons (`docs/app-icons.md`), `FF` = none. In `columns` `°` (C2 B0) is drawn as the
stock `º` (C2 BA). The text card (v1 and template `00`) is drawn as is and its font has no `°`: send `º` there.

- `PUSH` refuses a malformed v2 record (`BadRequest`); a v2 record to a stub before 9 is `BadRequest` too (it takes
  exactly 64 bytes): read the stub version first and push v2 only to 9 or later.
- The stub draws v2 only when the whole record is valid, including every icon index below the package's icon count;
  anything else (and a template it does not know) shows the package name. The face never depends on the record.
- Note: the design said 64..128 B for `PUSH` and op `0x14`; a `columns` record with one or two columns is shorter
  than 64, so both take any length up to 128 and the record's own checks decide.

Test vectors: `wp2apps.install.columns_record([(caption, icon or None, value), …])` builds the same bytes as the
SDK's `Api::widget_columns` (golden in `tests/test_install.py` and `sdk/cmf-wp2-app`). From a Mac:
`wp2apps widget set <slot> --columns 'Now:0:21°' '10:00:1:19°' --hci-port P` (caption:icon:value, empty icon = none).

Reply plaintext:

| Bytes | Meaning |
|---|---|
| `00` | Ok: written, or the same bytes as stored (no write) |
| `0A ‖ wait:u32le` | Later: this record was written less than 60 s ago; retry after `wait` ms. Nothing written |
| `07` | NoApp: no package in that slot |
| `01` | BadRequest: slot ≥ 16, unknown kind, wrong length |
| `02` | Io: the NAND write failed (the old file may be gone) |

## Limits (NAND wear)

- One write per slot and kind per **60 s** (`PUSH_EVERY_MS`, `fw/wp2-core/src/proto.rs`), counted from the last
  write since boot. The widget and the blob of a slot have separate clocks.
- Sending the same bytes again is free: `00`, no write, the clock is not touched. So the phone may resend its state
  after every reconnect without checking.
- Schedule: push only when the text changes; for a value that changes often (a counter), push at most every few
  minutes. Each write is a FAT write on NAND.

## The blob

- When the loader starts the app (menu, widget tap or page), the blob is the app's **first host message**: the app
  reads it with `recv` / the `MESSAGE` event like any message from the phone (`docs/app-guide.md`, section 4),
  `slot` = its own. Needs an ABI 1.1 app and the loader of 2026-11-19 or later.
- It stays on NAND and comes again at every start until the phone changes or removes it: it is state ("the last
  weather"), not a queue. An app that must not act twice keeps a version number inside it.
- While the app runs, a new blob waits for its next start. To reach a running app use `MSG` (`0x90A5`); a running
  page app behind the face is paused and answers `Paused` (9) to `MSG`, so push the blob and the widget instead.

## When (from which stub)

1. Read the stub version: `INFO` (`0x90A0`) with slot `0xFE` (`docs/abi-1.4.md`, "Stub version").
2. Version ≥ 8: `PUSH` works. Older: do not send it; a stub before 8 leaves `0x90A8` to the stock firmware and
   no reply comes. The widget then changes only while the app runs.
3. The slot comes from the phone's own installs or from `INFO` listing (`wp2apps list`).
4. Removing an app (on the watch or with `DELETE`) also removes its widget record and blob.

Reference: `wp2apps push --slot N --title T --value V`, `wp2apps push --slot N --blob HEX`, `wp2apps push --slot N`
(removes the widget record); `install.push`, `install.widget_record` in `src/wp2apps/install.py`. Test:
`tests/test_widget_push_e2e.py`.
