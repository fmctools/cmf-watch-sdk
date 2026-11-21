# Stock app.bin reconnaissance for the app loader

Firmware: JX402 `1.0.0.73`, `app.bin` sha256 prefix `30e7978c39d8fb79` (2,465,920 B, base
`0x10000000`). All addresses are virtual. Tools: Ghidra 12.1.2 headless (`scripts/recon/decomp`,
`scripts/recon/ghidra/Dump.java`), capstone, and the emulator probe `scripts/recon/psram_probe.py`.
Extract the image with `uv run python scripts/recon/extract.py` (writes `runs/recon/`, not tracked).

Confidence: **high** = read from code and consistent with a second source; **medium** = read from
code only; **low** = inferred.

## Values the plan depends on

| Item | Value | Confidence | How |
|---|---|---|---|
| Core | Cortex-M33 (ARMv8-M MPU RBAR/RLAR format at `FUN_100f6708`), FPU present (S-registers in QEMU state) | high | decompile + emulator registers |
| MPU | enabled with `PRIVDEFENA` (`MPU_CTRL = 5`) at boot with three fixed regions, none on `ResFbHeap`; default map keeps `0x2000_0000–0x3FFF_FFFF` executable (see "Hardware: caches and MPU") | high | `FUN_100f6708`, `FUN_100f67cc` |
| PSRAM | 8 MiB at `0x38000000` (aliases `0x18000000`, `0x34000000` write-through, `0x3C000000` uncached) | high | emulator machine model + literal survey |
| Resource heap | `ResFbHeap` (JHeap): base `0x380A9340`, size `0x676B80`, end `0x3871FEC0`; init `FUN_101884e4` → `FUN_10146aa4`. The size literal occurs **three times**: `0x10188508` (init), `0x10188588` (getter `FUN_10188584`, `ldr r0,=0x676B80; bx lr`, no direct callers — likely reached through an ops table) and `0x101885F4` (second getter at `0x101885F0`) | high | decompile + literal search |
| `APP_BASE` | `0x386FF000` | **superseded**: no fixed app or loader address; both run from `ui_mem` RES blocks (plan 2 memory map) | — |
| `APP_SIZE` | `0x20000` (128 KiB) | decision | see below |
| Reservation | plan 2's stub patches **all three** `ResFbHeap` size literals `0x676B80` → `0x655CC0`, so the heap ends at `0x386FF000` and `[APP_BASE, APP_BASE + APP_SIZE)` is never handed out. Not yet applied or verified: plan 2 patches, fills the region with a marker and walks the UI with a heavy face first | decision, unverified; **superseded by plan 2 revision: 4 KiB stub RAM only** | — |
| Display | 466×466 round AMOLED; UI is LVGL (`lv_obj_set_width(…, 0x1D2)` in `FUN_100bb954`) on the Actions view manager (`ui_view`, `view_create`) | high | decompile + emulator frame size |
| Motor drive | PWM device `"PWM"` (`0x101DF1C0`), registers `0x40060000 + 0x100·ch` (`+0x00` ctrl, `+0x14` period, `+0x18` duty), clock-divider `0x40001068 + 4·ch`, 4 channels; motor uses channel 0 | high | device struct + driver decompile |
| PWM clock for the motor | driver init `FUN_10121504` writes `0x303` to every channel's clock register. Actions SDK `pwm_acts.c` (pinned commit `26f51e58`): bits 9–10 select the source (0 = 32 kHz RC, 1 = HOSC 32 MHz, 2 = 64 MHz), divider field `0..255` = n+1, `256..260` = /512, /1024, /2048, /4096, /8192. `0x303` = HOSC /4096 = **7812.5 Hz**, which is the SDK's `PWM_PIN_CYCLES_PER_SEC` 8000 rounded to a power-of-two divider; `get_cycles_per_sec` (`FUN_10121468`) reports that 8000 | high | decompile + SDK source |
| Motor call | `FUN_100fb4b4(_, ch=0, period=0xFF, pulse=strength, flags=1)` → PWM `api[0]` `pin_set_cycles` (`FUN_1012197c`); `pulse = 0` → `api[4]` channel off (`FUN_101214ac`) | high | disassembly |
| Stock PWM frequency | 7812.5 / 255 ≈ **30.6 Hz**; duty = `strength / 255`. Not measured on hardware | high (code), unmeasured | arithmetic + emulator `motor.log` (decoder per SDK) |
| Motor type | **ERM** (single-ended unipolar PWM at ~31 Hz, strength by duty, patterns by on/off timing, no resonance tuning) | medium | driver shape; no datasheet |
| Vibration levels | table `0x101F4A00` = `[255, 250, 0]`; level 0 is forced to 255 with pattern 2 (700 ms), level 1 = **250/255 = 98 %** with pattern 0 (300 ms), level 2 = 0 (off) | high | `FUN_100e17cc` (property `0x25`) |
| Why "minimum is too strong" | the weaker stock level is 98 % duty; the levels differ mainly by duration | high | follows from the table |
| Vibration settings screen | `setvib_view` (id `0x56`, scene `0xEA8D7813`) is one LVGL roller. Its options are `snprintf("%s\n%s\n%s")` of the scene's texts Strong / Standard / Off (format literal at `0x100BBADC`, again at `0x100BBBD0` for a language refresh); the roller index is the level (property `0x24`, no range check in the setter `0x100E1739`). The crown handler `0x100BB858` steps up while the index is `<= 1` (`cmp r1, #1` at `0x100BB892`). Readers of the level that bound it: `0x100E17CC` (preview, property `0x25`) and `0x100E17F4` (play by level), both `cmp r3, #2`. `wp2apps firmware --weak 350` points both table literals at a 4-entry copy `[255, 250, 0, 89]` at `0x10259E80`, the format at `"%s\n%s\nWeak\n%s"` at `0x10259E90`, and raises the three bounds by one. Weak is roller index 2 but level 3 (Off stays level 2): hooks at `0x10259EA0` swap 2 and 3 on the five calls that cross between index and level: preview `0x100BB730` from `0x100BB8AE` and `0x100BB938`, set level `0x100BB6F4` from `0x100BBC00`, get level `0x100BB6B8` from `0x100BB966` and `0x100BBA3E`. The event `0x71` path (`0x100BBB18`, `0x100BBB40`) reads and writes the level itself and is left alone. The Settings list (`setlist_view` `0x44`, row 7 at `0x100B548C`) shows the level's name as the Haptic subtitle from `view + 0x1AC + level * 8` (the three scene texts `{char *, u32}`); level 3 read past them and showed nothing, so `0x100B54B2` calls a hook that returns a static `{"Weak", 0}` for level 3 (`0x10259EE0`, text at `0x10259EF0`) and `0x100B54B8` becomes `nop.w`. The selected row uses `font48m.font` (LVGL binfont in `res`), whose Cyrillic is only `БИРСавгдзийклнрстуъыь`: "Слабый" rendered as "Сла ый", so the label is Latin. Row texts are scene text nodes holding an index into `bt_watch.<lang>` (24 string tables in `res`); fonts are subset to the glyphs their strings use | high (emulator) |
| Motor switch-on | `FUN_100fb4b4` is called from three places: `0x100FB168` and `0x100FB2A8` pass pulse 0 (off), `0x100FB398` (pattern player) passes the strength. `wp2apps firmware --motor 350` redirects that one `bl` to a 14-byte hook at `0x10259F00` (the zero gap before the ksdfs tail; stock reads only 32 bytes at `0x10259E3B`) that scales the pulse by 350/1000: emulator `motor.log` shows boot `349 30`, level 1 preview and charge popup `341 30` | high (emulator) |
| Motor driver API | Zephyr device API at `0x101F5E7C`: `+0x0C` vibrate(dev, strength, pattern) = `FUN_100fb330`, `+0x10` set pattern table = `FUN_100fb0ec`, `+0x14` init = `FUN_100fb1fc`; driver handle at `*0x3801B164`; manager init `FUN_100e0cd8` | high | decompile |
| Pattern table | `0x101F4A0C`, 10 entries × 12 B: `u16 on_ms, u16 off_ms, u32 repeat, u32 total_ms`; count `u16` at `+0x78` (10). Entries: 300; 500; 700; 300/1700 for 60 s (×2, calls); 1000; 60; 150/850 for 2 s; 20; empty | high | dump |
| Properties | `ux2sys` table `0x101F4274`, 12 B each `{set, get, u16 size}`: `0x24` stored level (`+0x2C` of `0x380077F8`), `0x25` level preview → vibrate, `0x2A` (`SETMOTOR`) → vibrate 255 with pattern 5 (1000 ms); set `FUN_100e08b8`/`FUN_100e07b8`, get `FUN_100e082c` | high | decompile |

### Why `APP_BASE` / `APP_SIZE`

`psram_probe.py` filled PSRAM with a marker, reset the guest, walked the UI for 4 minutes (app list,
apps, swipes in all directions) and dumped PSRAM. The largest untouched block was
`0x38275000–0x383CF000` (1384 KiB), but it lies inside `ResFbHeap`, which serves resources and
frame buffers on demand, so "untouched during a walk" is not "free". No literal in the image points
into that range except ASCII coincidences. Taking the top 128 KiB of `ResFbHeap` and shrinking the
heap by the same amount is a guaranteed reservation at a cost of 2 % of the resource heap. Apps
link at `0x386FF000`, code and data in one image, executed directly from the `0x38…` mapping
(executable under the default map). The emulator does not model the MPU or caches; plan 2 verifies
execution there and repeats it on hardware only with approval.

### Consequences for plan 1

- Task 4 frequency knob: the motor is ERM, but at the stock ~31 Hz a low duty is likely felt as
  chopping rather than a softer buzz, so the lab keeps the knob: `[31, 62, 125, 250, 500, 1000,
  4000, 16000]` Hz, default 31 (stock).
- `motor(duty, freq)` in the loader maps to `pin_set_cycles` on channel 0 with
  `period = 7812 / freq` while the clock register holds the stock `0x303` (≥ 2 cycles, so up to
  ~3.9 kHz with coarse duty). Other frequencies need the divider changed (frequency API
  `api[6]` = `FUN_10121680`); the loader must write `0x303` back before handing the motor to stock.

## Emulator check of the motor path

`scripts/recon/motor_check.py` boots stock in a QEMU built with the motor log (cmf-watch-utils
branch `emu-motor-log`, divider decoded per the SDK table above) and calls the property handlers
through gdb with a manual call frame; it fails unless `(980, 30)` and `(1000, 30)` appear.
The log confirms the register values and timings the firmware writes; the frequency column is only
as good as the decoder, which now follows the SDK. `motor.log` (virtual ms, duty in ‰, Hz):

| Trigger | Log | Matches |
|---|---|---|
| boot | `… 1000 30` → `… 0 0` | pattern 6 (60 ms) at 255 |
| `0x25` level 1 preview | `… 980 30` for 300 ms | 250/255, pattern 0 (300 ms) |
| `0x25` level 0 preview | `… 1000 30` for 700 ms | 255, pattern 2 (700 ms) |
| `0x2A` `SETMOTOR 1` | `1000 30` in 1000 ms blocks until switched off | 255, pattern 5 (`total_ms` = 86,400,000, i.e. 24 h) |

## Items for plan 2 (located, not yet worked out)

| Item | Where to start | Confidence |
|---|---|---|
| App list | `applist_view` / `applist_list_view` / `applist_grid_view` / `applist_waterfall_view` strings at `0x10203A4B…`; launcher `launcher_app.c` (`0x1020364D`), `_launcher_key_event_handle` | medium |
| App launch | `system_app_launch` (`0x10054CF4`, from cmf-watch-utils docs), `system_app_launch_init` | medium |
| Overlay for notifications/calls | msgbox popups: `msgbox_cache popup …` (`0x101DF7BC`), `set_popup_msgbox` (`0x101DFAB4`), `notify_view` (`0x101DFC98…`), `btcall_view` (`0x1021B155`) | medium |
| Input | crown: `_knob_event_handler` (`0x1021B3E0`), keys: `*_key_event_handler`, touch: LVGL pointer indev (`INPUT_DEV_TYPE_POINTER`, `0x101FE090`) | low |
| Drawing | LVGL objects/canvas on the Actions view manager; colour depth and canvas API not yet read | low |
| Cyrillic | no Cyrillic UI strings in `app.bin`; fonts live in `/NAND:A/*.font` and are not inspected. The lab uses ASCII labels | medium |
| Storage | Zephyr FS mounts `/NAND:A` (UI), `/NAND:B`, `/NAND:C` (user dials, fonts), `/NAND:E` (animations), `/NAND:K` (boot) at `0x1020185B…`; these are read-only, so packages go to `/NAND:/wp2apps` on the writable FAT (plan 2, task 4) | medium |
| BLE command dispatch | handler table and AT group dispatcher `0x101F4D70` are documented in cmf-watch-utils `protocol/ble.md`; a free opcode is chosen in plan 2 | low |
| Stub location | NOR app partition `0x24000`, size `0x34B000`; `app.bin` uses `0x25A080`, leaving ≈ 964 KiB to append the stub. ACTH payload and header `sum32` must be recomputed (the emulator checks them, `emu/image.py`) | high |
| Zero runs inside `app.bin` | `0x101E5BB2` (10,830 B) etc. are initialised-data images, not free space | medium |

## Plan 2 findings

### Находки разведки

| Что | Адрес / значение | Увер. |
|---|---|---|
| ACTH | `+0x18` размер полезной нагрузки (сток `0x259C3C`), `+0x1C` = `0xFFFFFFFF − sum32(app[0x200:0x200+size])`, `+0x20` выравнивает `sum32(app[0:0x200]) = 0xFFFFFFFF`; пересчёт на стоке воспроизводит `0xDCF84B50`/`0x7929C5D6` | high |
| ACTH = `image_head_t` Actions SDK | `bootloader/soc/arm/actions/leopard/soc_boot.h` (lvgl/lv_port_actions_technology `26f51e5`): `+0x08` load_addr, `+0x0C` `ih_name[8]` (`0x62` = `"b"`), `+0x14` entry, `+0x18` img_size, `+0x1C` img_chksum, `+0x20` hdr_chksum, `+0x24` hdr_size `0x200`, `+0x2C` flags (сток 0, без подписи). `app_image_check()` (`boot/soc_boot_priv.c`) проверяет magic, вектор сброса по `+0x204` (нечётный, в пределах `code_max_len`), `img_size ≤ code_max_len`, обе суммы. Хвост `0x25A000..0x25A080` — `ksdfs.bin`, дописанный `build_firmware.py` после расчёта заголовка (сток не покрывает его размером, `0x1C4` нулей перед ним); прошивка ищет ksdfs по `0x10259E3B` (`FUN_100f5d2a`, «koff»), там нули, так что хвост не используется ни стоком, ни патчем. Установщик OTA проверяет только CRC32 файлов AOTA. Патчер: без заглушки размер стоковый, с заглушкой `len − 0x200`, итог сверяется `check_acth()` | high (SDK), medium (что бинарный загрузчик часов = SDK) |
| XIP | эмулятор отображает NOR `0x24000..+0x3DC000` на `0x10000000`, так что дописанные байты видны сразу | high |
| Инициализация кучи | `FUN_101884e4`: `bl 0x10146aa4` по `0x101884FA` (8 аргументов, 4 из них на стеке); литералы размера `0x10188508` (init), `0x10188588`, `0x101885F4` (геттеры) | high |
| BLE-диспетчер | `FUN_100d343c(frame)`, поля уже в host order; `c1 == 0xFFFF && c2 > 0x908B` → `default` по `0x100D34EA`: `ldrh r1,[r5,#1]` → `bl 0x100cc284` по **`0x100D34F0`** (`f8f7c8fe`), то есть чтение payload в `0x3804B0E6`, потом лог; `r5` — указатель на кадр | high |
| BLE-хелперы | `FUN_100cc284(buf, len)` — прочитать payload (0 = ok); `FUN_101b608c(&u16 out_len, buf, len, cmd)` → указатель на открытый текст или 0 (AES-CBC ключом сессии, CRC32); `FUN_101b605a(&u16 out_len, data, len, cmd)` → указатель на шифротекст; `FUN_101ddb66(dst, src, n, max)` = memcpy_s; `FUN_100cc2d4(frame, total)` — отправить; стоковый кадр ответа `0x3804B0DB` | medium |
| Ключ по команде | `FUN_100ccab4`: особый ключ только у `0x804B` и `(cmd & 0x7FFF) == 0x49`, остальные команды шифруются ключом сессии | high |
| Файлы (Zephyr fs, `struct fs_file_t` 0x2C Б) | open `0x10107030(file, path, flags)` (`READ 1`, `WRITE 2`, `CREATE 0x10`); read `0x10107238`; write `0x10107294`; seek `0x101072F0`; tell `0x10107350`; sync `0x101073A4`; close `0x101071E0`; unlink `0x1010764C(path)`; rename `0x10107704(from, to)`; поиск точки монтирования `0x10106FA0(&mp, path, 0)`; vtable `*(mp+0x1C)`: `+0x34` unlink, `+0x38` rename, `+0x3C` mkdir (обёртки `fs_mkdir` в образе нет) | high / mkdir medium |
| Таблица view | 162 записи по 0x18 Б, `0x100004B8..0x100013E8`: `{name, proc, presenter, 0, u16 id, u16 order, u16 w, u16 h}`; поиск `FUN_1018bc04(id)`; литералы начала: `0x101461AC`, `0x1018BC24`, `0x1018DF3C`; конца: `0x10055458`, `0x1018BC28`, `0x1018DF40`; открыть view: `FUN_101898bc(id, arg)`; id `0xF0` свободен | high |
| Сообщения view (стиль `proc`, как `goodbye_view` `0x10060A54`) | `proc(view_id, msg, data)`: 2 preload, 3 layout (`*(void**)data` — дисплей), 4 delete, 7 focus, 8 defocus, 0xB update, 0xD key (`data[0]` — код клавиши, байт `data+4 = 1` — «обработано») | medium |
| Коды клавиш (UART `view_key:`) | нажатие коронки `0x20000001` → `0x04000001`; поворот в списке `0xFFFF006F` / `0xFFFF006E` | medium |
| LVGL 8.3 | `lv_disp_get_scr_act 0x101255E0`, `lv_img_create 0x1013ED38`, `lv_img_set_src 0x1013ED50`, `lv_obj_set_pos 0x1012926C`, `lv_obj_set_size 0x101294C8`, `lv_obj_invalidate 0x10129FD8`, `lv_obj_del 0x1012C774`, `lv_obj_add_event_cb 0x10125A94`, `lv_timer_create 0x1013B10C`, `lv_timer_del 0x1013B1A4` (medium), `lv_event_get_indev 0x10125BD4` (low); события: `PRESSED 1`, `SHORT_CLICKED 4`, `RELEASED 8`, `SCROLL 0xB` | high, если не указано иное |
| Память UI | `ui_mem_aligned_alloc(type, align, size, caller) 0x101882C4` (типы 0..2); малые выделения `0x10146A3C(size, tag)` / free `0x101CE04A` | medium |
| ОС | `os_thread_create(buf, size, entry, p1, p2, p3, prio, opts, delay) 0x101BE1A2` (в начале `buf` лежит `k_thread` на 0x118 Б); `os_sleep(ms) 0x101BE1EC`; `k_thread_abort` — `0x10105C90` или `0x1019B2D8` | high / abort low |
| Всплывающие окна | `msgbox_cache_popup(id, udata) 0x100D5E34`; `set_popup_msgbox 0x100D6EB0` / `set_close_msgbox 0x100D6FB4` ведут флаги `*(u8*)0x3800FAF4` (бит 0 и бит 1 = окно открыто), id в `0x3800FAF6`/`0x3800FAF8` | high |
| PWM | `struct device` "PWM" `0x101DF1C0`, api `0x101FB190`: `[0] pin_set_cycles(dev, ch, period, pulse, flags) 0x1012197D`, `[4] off(dev, ch) 0x101214AD`, `[6] set(dev, ch, hz, duty%, pol) 0x10121681`; период — 16 бит | high |
| Список приложений | иконки из ресурсов сцены, построитель `FUN_1005e9d8` (число по `data+8`, записи по 3 слова с `data+0xC`, клик `0x1005EBA1`, событие 4); процедуры list `0x1005EEC9`, grid `0x1005EBC5`, waterfall `0x1005F101` | low, разбирает задача 9 |
| Эмулятор | ввод `input.txt`: `t down x y`, `c down` (коронка/питание), `r delta`, `p mV charger percent`; без стокового профиля `STOCK_AWAKE` не включается, поэтому харнес пишет `0xFFFFFFFF` в `**0x38037D14` (таймаут автосна, `system_set_auto_sleep_time`); NAND живёт в памяти QEMU и переживает сброс гостя | high |

`ResFbHeap`: литерал размера `0x676B80` встречается трижды (`0x10188508`, `0x10188588`, `0x101885F4`); патчер меняет **все три** на `0x675B80`: куча кончается на `0x3871EEC0`, выше лежит только RAM заглушки (`0x1000`). Загрузчик и приложения получают блоки кучи на время работы.

Stock 1.0.0.73 crashes (`MllcFailedHook`, 651,548-byte block, then a reboot) when the face carousel is scrolled with face `hvk9m1hmpyuheyd`; reproduced on unpatched stock, so the e2e walk does not scroll the carousel.

## Plan 2 revision: memory while running

Read-only recon for the owner's 2026-11-10 memory decision (loader and app allocated from the stock heap
only while running). Sources: `scripts/recon/decomp` on `0x10188200–0x10188700`, `0x10147124`,
`MllcFailedHook`; table dump of `0x101FD7F0`; a scratch build of `sdk/` with `ORIGIN = 0`.

| Item | Value | Confidence |
|---|---|---|
| `ui_mem` type table | `*0x1018837C = 0x101FD7F0`, 3 entries × 0x24 B: `{name, init, alloc, aligned_alloc, realloc, free, +0x18 dump, +0x1C, +0x20 total}`. Type 0 `FB`, 1 `GUI` (LVGL heap, 0x18000 B), 2 `RES`. `uimem dump [0\|1\|2], where 0-FB, 1-LVGL, 2-RES` | high |
| `ui_mem` API | `ui_mem_alloc(type, size, caller)` `0x10188234`; `ui_mem_aligned_alloc(type, align, size, caller)` `0x101882C4` (align must be a power of two, asserts otherwise); `ui_mem_realloc` `0x101883C4`; `ui_mem_free(type, ptr)` `0x10188380`. `caller` is a C string used only in the failure log | high |
| `FB` and `RES` share `ResFbHeap` | RES: init `0x101884E4` (the `ResFbHeap` init), alloc `0x10188518`, aligned_alloc `0x1018854C`, free `0x10188578`, `+0x18` `0x1018858C` (not a dump: the failure path, see below), total `0x10188584`. FB: aligned_alloc `0x101D78DC` (align ≤ 0x40, else returns 0) → `0x101885A8` (align 0x40, retries 15× with `os_sleep(2)`), free `0x101D78EA` → `0x10188578`, total `0x101885F0`. Both end in JHeap alloc `0x10147124` on the heap struct `0x38043824` | high |
| JHeap struct `ResFbHeap` (`0x38043824`) | `+0` name, `+4` total, `+8` free bytes (alloc subtracts, free adds), `+0xC` minimum free ever, `+0x14` flags/alignment bits, `+0x60` free-list head, `+0x68` end marker. Free block: `{next: u32, size: u32 & 0xFFFFFF}` (size includes the header) | medium |
| JHeap allocation policy | first fit from `+0x60`; a request of exactly `0x6A088` B (466·466·2, a full RGB565 frame) takes the last fitting block instead (best-from-end) | medium |
| **JHeap never returns NULL** | when `free < need` or no free block fits, `0x10147124` calls `MllcFailedHook` `0x101470C0`, which prints the heap dump, calls `0x100F6D84(0)` and spins forever → the watch resets. This is the `MllcFailedHook at line: 353` reboot seen with the heavy face. So loader, app and framebuffer allocations must be checked against the free list **before** calling `ui_mem_*` | high |
| Allocation size overhead | `need = size + hdr(type) + (flags & 0x3F) [+ 4 debug guard] [+ align − heap_align]`, rounded up to the heap alignment (4). Plan 2 uses the conservative bound `size + align + 0x100`; plan 2 task 1 measures the real overhead in the emulator | medium |
| Lock | JHeap alloc/free raise `BASEPRI` to `0x20` around list updates (no mutex). Walking the free list and allocating under the same `BASEPRI` makes check-then-alloc atomic against other threads | medium |
| Heap statistics | dump `FUN_10146BA4(heap, 1, 0)` prints `Total:%u, Free:%u (%u%%), Water:%u (%u%%), UsedMax:%u (%u%%), MaxBlock:%u`. Do not call RES `+0x18` (`0x1018858C`) for it: that one dumps, saves and panics (the watch reboots) | high |
| Rust codegen for relocatable images | with `ORIGIN = 0` and `-C link-arg=--emit-relocs`, rustc 1.97 emits `R_ARM_THM_MOVW_ABS_NC`/`R_ARM_THM_MOVT_ABS` pairs for addresses (hello: 4 + 4, vibration-lab: 20 + 20, plus 6 `R_ARM_ABS32`). With `-C target-feature=+no-movt` addresses go to literal pools: only `R_ARM_ABS32` remain (hello 3, vibration-lab 28), plus PC-relative `R_ARM_THM_CALL`/`R_ARM_THM_JUMP24` that need no fix-up. `e_entry = 0x1`. rustc warns that `no-movt` is an unknown/unstable feature, but honours it | high |
| Cache maintenance for code in PSRAM | `0x101BEE52 (addr, size)`: SPI1 (PSRAM) cache write-buffer flush + write back and invalidate of the range; `wp2_core::load::load` calls it. See "Hardware: caches and MPU" | high |

Heap probes (emulator, patched image): `ui_mem_aligned_alloc(2, 8, 0x10000, 0)` costs 0x10018 bytes of `free` (`u32(0x38043824 + 8)`), i.e. a JHeap overhead of `0x18`, within the plan's `SLACK`. `FUN_1018858D` is not a dump helper but the stock failure path (prints `Dump:ResFbHeap … MaxBlock:0`, then `save_all_data` and panics), so the tests never call it and check the free list (`u32(0x38043824 + 0x60)` chain) instead.

Heavy-face selection for the e2e walk (candidates in `emu faces --sort downloads` order, first whose stock control walk of 240 s has one boot, no `MllcFailedHook`, no fatal):

| Face | Control on unpatched stock | Verdict |
|---|---|---|
| `hvk9m1hmpyuheyd` (NMS 176, 1267 downloads) | 2 boots, 1 `MllcFailedHook` (651,548 B) | rejected |
| `kcumc43wn4cb8e9` (Wavy Seconds, 534) | `emu run --fmc-face` fails: QEMU stops on an unimplemented write at `0x40070504` during boot | rejected |
| `w23jrco8sqew3nv` (Concentric Hal, 476) | 1 boot, no `MllcFailedHook`, no fatal | chosen |

## Plan 2, task 4: view table bounds

Disassembly of every PC-relative load of the six literals listed under "Таблица view" (`scripts/recon` + `llvm-objdump`):

| Literal | Reader | Meaning |
|---|---|---|
| `0x1018BC24` / `0x1018BC28` | `FUN_1018bc04(id)`: `for (e = start; e < end; e += 0x18) if (e->id == id) return e;` | view table start / end |
| `0x1018DF3C` / `0x1018DF40` | `FUN_1018dc9c` (view create), the same inline loop with step `0x18` | view table start / end |
| `0x101461AC` | `FUN_10146060`: loop `0x100003A0 .. 0x100004B8`, step `0x1C`, entries go to `os_thread_create` | **end of the preceding table**, not the view table start |
| `0x10055458` | `FUN_10055420`: `0x100013E8 .. 0x100015E8`, count `(end − start) >> 4` passed to `0x100D5D64` | **start of the following table**, not the view table end |

The view table sits between two linker-generated tables, so their bounds share values with it. The patcher
therefore rewrites only the four view literals; `0x101461AC` and `0x10055458` stay stock (rewriting them would
walk the thread table over the new view table and drop the following table). No other code reads the
table: no aligned word in the image points into `0x100004B8..0x100013E8` except two bit-mask constants
(`0x10000800` at `0x1017324C`, `0x10000808` in data at `0x101EC848`), no `movw` of `0x04B8`/`0x13E8`/`0xF30`,
and the count `0xA2` is never used — both readers are bounded by the end pointer. The view manager
`0x10187000..0x10190000` has no `cmp rN, #0xEE..0xF0`; ids are stored as `u16`; the highest stock id is
`0xED`. Byte `+0x13` of an entry (high byte of the `0x0102` field) indexes a 3-entry display table at
`0x38043A78` (`FUN_1018e580`, must be ≤ 2).

Preload result (`FUN_1018dc9c`, `0x1018DE08`): the view is created and its scene prepared
(`FUN_1018e580`, `FUN_1018e6e4`), then `proc(id, 2, view+0x1C)` (or `presenter(id, view+0x1C, 2, …)` when
`+8` is set). A non-zero result logs, calls the scene cleanup `FUN_1018e6bc`, frees the view and returns the
code to `FUN_101898bc`, so the previous view stays on screen. The stub returns `-2` there when the loader
is missing, corrupt or does not fit.

### Mounts and `mkdir` (emulator probe, `fs_get_mnt_point` + mount list walk)

| Mount | Type | API (`mp+0x1C`) | Writable |
|---|---|---|---|
| `/NAND:` | 0 (FAT) | `0x101F6AC0`: all 20 entries set, `mkdir` (`+0x3C`) = `0x101079F1` | yes |
| `/NAND:A`, `B`, `C`, `E`, `K` | 2 | `0x101F5CD8`: open, read, lseek, tell, close, mount, unmount, stat, statvfs only; `write`, `truncate`, `sync`, `opendir…closedir`, `unlink`, `rename`, `mkdir` are 0 | no |

`fs_get_mnt_point` takes the longest prefix, so `/NAND:C/…` reaches the read-only type 2 fs. Stock writes go to
`/NAND:/…` (`/NAND:/epo.bin`, `/NAND:/record.pcm`). The plan's `/NAND:C/wp2apps` (and its fallback, files in
the root of `/NAND:C`) cannot be written; `wp2_core::layout::DIR` is `/NAND:/wp2apps` instead, on the FAT,
which has `mkdir`.

### Calling stock code from gdb

A plain QMP/gdb stop can land in an ISR or the idle thread. Calling a function that takes a mutex there
(`fs_get_mnt_point`, `FUN_101898bc` → `os_mutex_lock 0x101BE218`) panics with `Fault during interrupt
handling`. The harness therefore breaks on the entry of `os_mutex_lock` (only threads call it) and runs the
call from that thread. `os_sleep` is not a usable hook: nothing calls it while the watch idles.

## Plan 2, task 6: LVGL colour format and touch input

Ghidra (`scripts/recon/decomp`) on `0x1013E9AC`, `0x1013E52C`, `0x1012F38C`, `0x10125BD4`, `0x101C4C80..0x101C5000`,
`0x1012EE94`; view delete path disassembled at `0x1018D7FC..0x1018D8AA`.

| Item | Value | Confidence |
|---|---|---|
| `LV_COLOR_DEPTH` | **16**: `lv_canvas_fill_bg` (`0x1013E9AC`) clears a `TRUE_COLOR` buffer with `memset(data, 0, w·h·2)`; stock icons are `TRUE_COLOR_ALPHA` 96×96 in 27648 B (3 B/px) | high |
| `LV_COLOR_16_SWAP` | **0**: the built-in decoder (`0x1012F38C`) turns palette entries into `b>>3 \| (g>>2)<<5 \| (r>>3)<<11`, plain RGB565 | high |
| Framebuffer | `FB_BYTES = 466·466·2 = 0x6A088`, type `FB` (0), align `0x40`; `lv_img_dsc_t.header = 4 \| 466<<10 \| 466<<21` | high |
| `lv_coord_t` | **32 bit** (`LV_USE_LARGE_COORD`): `lv_indev_get_point` copies two words; `lv_point_t` = `{i32 x, i32 y}` | high |
| `lv_obj_add_flag(obj, flags)` | `0x10128D39` (`obj+0x24 \|= flags`); stock calls it with `2` (`LV_OBJ_FLAG_CLICKABLE`) next to `lv_obj_add_event_cb` (`0x10125A95`) | high |
| `0x10125BD4` | **not** `lv_event_get_indev`: it is `lv_obj_remove_event_cb(obj, cb)` (walks `spec_attr` event list, returns bool) | high |
| `lv_event_get_target / current_target / code / param / user_data` | `0x101C4C93` (`e+0`) / `0x101C4C97` (`+4`) / `0x101C4C9B` (`+8 & 0x7F`) / `0x101C4CA3` (`+0x10`) / `0x101C4CA7` (`+0xC`) | high |
| Event parameter of `PRESSED`/`RELEASED` | the active indev (`*0x3802AED4`), as in LVGL 8.3 (`lv_event_send(obj, 1/8, indev_act)` in the indev proc) | high |
| `lv_indev_get_point(indev, &point)` | `0x101C4F57`: pointer/button type → `act_point` (`indev+8`, `+0xC`), else `-1,-1` | high |
| Image cache | `LV_IMG_CACHE_DEF_SIZE = 0`: `_lv_img_cache_open` (`0x1012EE94`) reopens a single static entry per draw and `lv_draw_img` closes it, so no cache entry keeps `&DSC` or the framebuffer | medium |
| View delete order (`FUN_1018DC9C` area) | display op `+0x10` → `proc(id, 4, view+0x1C)` → … → scene cleanup `FUN_1018E6BC` (display op `+4`). The view's own `lv_obj_del` in msg 4 runs before the scene is torn down | medium |
| Layout after preload | the view manager sends msg 3 only after a `{9, 3, id}` message: `ui_view_layout(id)` `0x101D72FF` (`0x10182060(id, 3, 0, 0)`). Stock preload handlers (`goodbye_view`) end with `return ui_view_layout(id)`; a preload that just returns 0 leaves the view created and focused but never laid out (e2e: no `view 240 layout started`) | high |

## Plan 2, task 10: "Custom Apps" row in the stock app list

Ghidra (`scripts/recon/decomp`) on `0x1005E790..0x1005F300`, emulator probes on stock and patched NOR.

### Stock app list

| Item | Value | Confidence |
|---|---|---|
| Entries | `0x101EAF40`, flash, **25 × 12 B** `{u32 icon_res_id, u32 label_res_id, u32 target}`; the ops table follows at `0x101EB06C`, no room. Literals: `0x1005E7E4` (click), `0x1005E9C4` (grid), `0x1005EC40` (list text), `0x1005ED58` (list preload), `0x1005F170` (waterfall preload) | high |
| `target` | `≤ 0xF4` → `FUN_101898bc(target, 0)`, else a Thumb function. Views: 0 `0x6A` sportlist, 3 `0x73`, 10 `0xDC`, 11 `0xC5`, 12 `0x90`, 13 `0x88`, 15 `0x8C`, 16 `0xCF`, 17 `0x1C`, 18 `0x61`, 19 `0x81`, 20 `0xCD`, 21 `0xCE`, 22 `0x86`, 23 `0x43`, 24 `0x44` setlist; functions at 1, 2, 4–9, 14 | high |
| Icons, labels | hash ids into the scene resources `/NAND:A/bt_watch.sty`/`.res` and `/NAND:A/bt_watch.<lang>`; loaded by `FUN_101848d4(scene, ids, out_img_dsc, 0, n)` / `FUN_10184a5c(scene, ids, out_text, n)`. There is no "Apps" string | high |
| Count | 25 is an immediate everywhere (`0x1005ECB8`, `0x1005ED02`, `0x1005F134`, `0x1005F218`, `0x1005F230`, `0x1005F2CE`, click `0x1005E7B0` `cmp r4,#0x18`); `data+8` only stores it | high |
| View data (list) | `applist_view` id `0x12`, desc `0x101EB07C` (`+4` size `0x230`). `data = *(view+0x14)`: `+0` screen, `+4` scroll container (rows 0..24 in order), `+8` u8 count, `+0xC` 25 × `lv_img_dsc_t`, `+0x138` 25 × `TextRec {const char *text; u32 0}`, `+0x200` label `lv_style_t`, `+0x20C` font `/NAND:A/font32M.font`, `+0x214` label height (39). Arrays are packed, so a table entry cannot be added without moving ≈ 20 offsets | high |
| Styles | preload `0x1005E848` reads property `0x2C`; `*(0x101EB0AC + 4·style)`: 0 list desc `0x101EB154` (**default**), 1 waterfall `0x101EB184`. Grid (`0x101EB0B4`) is dead in 1.0.0.73: `applist_grid_view` shares id `0x12` and the lookup returns the first entry | high |
| Style desc | `{u8 flags, u8, u16 view_id, u16 data_size, …, +0xC layout, +0x14 focus, +0x1C preload, +0x20 unload, +0x24 res reload, +0x28 key, +0x2C misc}`; the layout is called only through the RAM pointer to the selected desc (`0x1005E8A4`) | medium |
| Click | event 4 (SHORT_CLICKED) on a row → `0x1005ED75` → `FUN_1005e790(row->user_data)`: `index < 25`, `table[index].target`. Emulator: tap on row 0 → `view:106` (sportlist) | high |
| Row (layout `0x1005EEE4`) | `row = lv_obj_create(cont)`, `user_data = i`, size `0x18A × icon.h`, cb `0x1005ED75` on 4, first row `lv_obj_align(1, 0x10, 0x50)`, then `lv_obj_align_to(row, prev, 0xD, 0, 8)`; vendor image create `0x1005A015` + flag `0x4000` + set_src `0x1005A02D`; vendor label create `0x1005B579`, size `(0x172 − icon.w) × data[+0x214]`, style `data+0x200`, long mode `0x101ACBA5(3)`, set_text `0x1005B70D(&TextRec)`, flag `0x4000`, `lv_obj_align_to(lab, img, 0x14, 0x18, 0)`. After the loop: restore scroll, fisheye once, vendor scrollbar, return 0 | high |
| Unload `0x1005EC51` | save scroll → `lv_obj_clean(scr)` (all rows) → `lv_style_reset(+0x200)` → free 25 images and 25 texts by `data[8]` → close font. `data[8]` must never change | high |

LVGL addresses (Thumb): `lv_obj_create 0x10127CFD`, `lv_obj_get_child(obj, i) 0x1012C3DD` (negative from the end),
`lv_obj_get_child_cnt 0x1012C4AD`, `lv_obj_clean 0x1012C881`, `lv_obj_set_size 0x101294C9`, `lv_obj_set_x 0x101290CD`,
`lv_obj_align 0x101C5DE5`, `lv_obj_align_to 0x1012A87D` (5th argument on the stack), `lv_obj_add_flag 0x10128D39`,
`lv_obj_add_event_cb 0x10125A95`, `lv_obj_add_style 0x1012BB15`, `lv_obj_get_scroll_y 0x101C6515`,
vendor image `set_zoom 0x1005A2D1`.

### The patch (P5, option D)

The rodata word `0x101EB160` (list desc `+0xC`, stock `0x1005EEE5`) points to the stub's `wp2_applist_layout`. It
calls the stock layout and, when that returns 0 with `data[8] == 25` and 25 children in the container, adds a
"Custom Apps" row built exactly like the stock rows and shows it first. Its own cb on event 4 calls
`FUN_101898bc(0xF0, 0)`. The table, the count, the indices and the view data stay stock; the row dies with the screen
in the stock unload. Only the list style is covered: with property `0x2C == 1` (waterfall) there is no such row.

- Order: the row stays the **last child** (index 25), because the stock fisheye `0x1005ED98` (cont event 11, user
  data `data`) walks `children[0 .. data[8]]`. The row is placed with `lv_obj_align(row, 1, 0x10, 0x50)` (stock row 0's
  spot), then each stock row is re-chained with `lv_obj_align_to(row_i, prev, 0xD, 0, 8)`. A stub cb on cont event 11
  applies the stock fisheye to our row, then `lv_event_send(cont, 11, NULL)` (`0x101259C5`) re-runs both. No child-move
  API is needed; clicks use `row->user_data`, so stock rows keep their indices.
- Fisheye (`0x1005ED98`): `mid = (FUN_101298b8(row) + FUN_10129b04(row)) >> 1` (top and bottom y in the content),
  `d = |mid − 233 − lv_obj_get_scroll_y(cont)|`; `d < 104`: zoom 256, opacity 255, `x = d / 3.25 + 16`;
  `d < 281`: zoom `256 − (90d − 9360) / 177`, opacity `255 − (180d − 18720) / 177`, `x = (d − 104) / 1.92 + 48`
  (floats at `0x1005EEC0/C4`); else untouched. Zoom goes to `simple_img_set_zoom 0x1005A2D1` (row child 0), opacity
  to `0x101C74C5(label, opa, 0)` (row child 1), x to `lv_obj_set_x`. The stub's integer version matches the float
  one for every `d < 281`.
- Icon: stock format `TRUE_COLOR_ALPHA` 96×96 (`header 0x0C018005`, RGB565 LE + A8, 27648 B), own artwork (circle
  `0x2966` with 2×2 rounded squares `0xF79E`, the stock circle and glyph colours, glyph box 40 px like the stock one),
  drawn at layout time into one RES block `[64 B][pixels][lv_img_dsc_t]` (≈ 27.7 KiB), freed by a cb on the image's
  `LV_EVENT_DELETE` (33). Unaligned pixel data (block + 12) stops QEMU on the Display Engine write `0x4006C020`; the
  stock icons are 64-byte aligned and so is ours. The emulator's VG-Lite model reads 3-byte pixels only behind the
  stock resource-cache header (`cmf-watch-utils` `acts_vg_resource_packed`: `+8` size `align64(64 + w·h·3)`, `+16/+20`
  w/h, `+24` 3, `+32` pixel address) and otherwise assumes ARGB8888 (garbage). The firmware programs the GPU the same
  way for stock icons and ours, so the 64 B mimic that header; the hardware does not read it.
- Label: `TextRec {"Custom Apps", 0}` in stub flash. The vendor label keeps the pointer (static flag) and re-reads
  `rec->text` on every refresh (language change), so both must live as long as the label.
- RES heap: the label canvas (A4, 280×39) takes ≈ 5.5 KiB while the list is open and is freed by its destructor
  (`0x101ACB9F`); the icon block is freed on delete. `ResFbHeap` free bytes are equal after each list open/close
  cycle (emulator).
- The saved scroll position is restored by the stock layout before the rows move; positions are the same, only the
  end of the list may come up one row short.
- Harness: list scrolled to the top: "Custom Apps" at `y ≈ 128`, Exercise (row 0) at `y ≈ 232`; scrolled to the end
  (16 swipes `(233,400)→(233,120)`): Settings (row 24) at `y ≈ 340`, row 23 at `y ≈ 236`.
  The stock view manager logs `view <id> focused` for every view (including `0xF0`); `_view_focus:1` is logged
  only by stock procs. The crown scrolls the list as in stock (same frame deltas).

Rejected alternatives: a settings entry (`sysmenulist_view` `0x4C`, same static table shape), a long crown press in
the list (desc `+0x28` key handler; the long-press code and whether the system swallows it are unknown), own
resources (labels and icons come from vendor `/NAND:A` resources).

### BLE install channel (task 5)

- The stock decrypt `0x101B608D` logs `check_extra_data fault` and drops frames with an empty plaintext, so END
  carries one dummy byte.
- `uart0.log` logs every incoming frame as `head_id/ability_id/cmd_id/length_id` and every reply as
  `wewear send ability:… cmd:…`; handy for probes.
- `0x804D` shows the "ble_connect" popup and sets `set_autosleep_time:5000`.
- With the clock off by more than 2 minutes, `0x8004` time sync makes the stock app log `TIMESTAMP OVER 2min
  RECOVERED` and reboot through a factory recovery; the host handshake skips `0x8004`.

### Bind popup, authkey and session key (early-review fixes)

- Stock key buffers: authkey `0x38048E08` (16 B; `0x804B` and `0x8049`/`0x0049` frames, `0x100CCAB4`), session
  key `0x380029A6` (16 B; every other system frame). At boot `0x100CC348` copies the authkey from `0x38007804`
  (all zero, `key1 init: 0x0 …` in `uart0.log`) and loads the session key from the NVRAM key `APP_SESSION_KEY`
  (absent in the fixture: `g_session_key_save err:-2`, buffer stays zero). `SESSION_KEY init` (`0x100CCFDC`,
  from the `0x804B` handler) sets `session = sha256(nonce ‖ authkey)[:16]` and saves it to NVRAM.
- The `0x804B` handler does not check its request: it answers any frame with a fresh nonce encrypted under the
  current authkey. Right after boot the authkey is zero, so a client that never paired gets the nonce, derives
  the session key and is answered by the stub (probe `runs/e2e/fixprobe-old.log`: `INFO` with the derived key got
  status OK). A frame encrypted with an all-zero session key alone is not answered.
- The stub therefore answers `0x90A0..0x90A4` only while both the authkey and the session key are non-zero. A
  non-zero authkey comes only from a passed `0x8047` challenge (needs the pairing secret). There is no cleaner
  stock signal: `0x8047` sets its flags `0x380498AA/AB` before it checks the signature.
- **Threat model: the gate only closes the zero-key path.** The pairing secret is not secret to a BLE client.
  Any connected client reads it with `AT GETSECRET` on the shell characteristic, and the second `0x8049` binds
  without a tap on the watch. This holds in the emulator (the CLI and the e2e "attacker" in
  `test_unpaired_session_refused` both do it) and on the real watch: the owner's read-only check (2026-11-10)
  got the 32-hex secret on an unbonded Mac with no confirmation on the watch (macOS MTU 247, all expected UUIDs
  present). **Anyone in BLE range can complete the stock handshake and install a package silently.** That is
  the same exposure as stock OTA, and the owner accepted it as is; there is no code change for it.
- `0x8047` sets `*0x380498AB = 1`; the next `0x8049` (`0x100CDCAC`) then opens the bind popup
  (`ble_pair_request_msgbox`, sets the flag to 2) and does not answer. Any later `0x8049` sees 2 and answers at
  once with the authkey; `0x804D` closes the popup when the flag is 2.
- Confirming the popup (`bind_pick_receive`, `0x100D09E6`) calls `0x100CD2CC`, which zeroes the authkey (and
  `0x38007804`), and sends the `0x0049` reply encrypted with that zero key. The host decrypted it with its
  authkey and failed with `bad padding`: the demo install flake (owner taps on the popup,
  `runs/e2e/demo-215315`). The host now sends `0x8049` twice and, on a zero-key reply, restarts from `0x8047`.


## Plan 2, task 7: app thread, time, crown and touch

Ghidra (`scripts/recon/decomp`) on `0x101BE1A2`, `0x10146060`, `0x10105C90`, `0x1019B2D8`, `0x1018B234`, the knob
functions below; disassembly of `0x101842A4`, `0x101BA1C8`, `0x101DB2A6`, `0x1019B7E0`, `0x100D5C90`. Probes:
`runs/e2e/task7-probe{1,2,3}.log`.

| Item | Value | Confidence |
|---|---|---|
| `os_thread_create(buf, size, entry, p1, p2, p3, prio, options, delay_ms)` | `0x101BE1A3` → `k_thread_create(buf, buf + 0x118, size − 0x118, …)`, delay `-1` = never, returns the thread; entry gets `(p1, p2, p3)` | high |
| `k_thread_abort(tid)` | **`0x10105C91`**: `z_thread_entry` (`0x101BA1C8`) calls it with `k_current_get()` (`0x1019B2CD`) after the entry returns; it unpends, aborts the timeout and unlinks the thread. `0x1019B2D8` is a bool "current thread is preemptible" check, not abort | high |
| Thread priorities | stock thread table `0x100003A0` (0x1C B: name, stack, `u16` size, `u8` prio at `+0xA`, p1..p3, entry): created at `-16`, then `k_thread_priority_set(+0xA)`. `ui_service` **3** (its loop `0x1018B234` runs the view messages, so view procs, LVGL timers and event callbacks), `lvglworkq` 2 (`k_work_queue_start` at `0x101842BC`), `bluetooth` 2, `media` 1, `gps_service` 4, `sensor_service` 5, `logsrv` 0xB, `vendor`/`ble_fate`/`wewear`/`wewear_ftp` 0xD. Apps ran at 4 here; task 9 moved them to 14 (below) | high |
| Time | `sys_clock_tick_get()` `0x1019B7E1` returns `u64` ticks; 1 tick = 1 ms (`os_sleep(ms)` hands ms to `k_sleep` unscaled; the shell `uptime` prints the same value as ms via `0x101DB2A6`) | high |
| Crown rotation | `view_key 0xFFFF006F/6E` reaches only list views. Any other view gets rotation only through an LVGL object registered as the display's knob object: `lv_obj_add_event_cb(obj, cb, 0x2D, …)` registers it (`0x10125A94` → `lv_disp_add_knob_obj` `0x10128C50`; also `lv_obj_add_flag` bit 30), and the object then gets event `0x2D` whose param points to the `i32` step. `lv_obj_del` unregisters it (`0x1012C644` → `0x10127D14`). The loader maps step > 0 to `+1`; `r 80` → `0x6F` (+1) in a list view, the sign of `0x2D` itself is not observed | high / sign low |
| Event callback filter | filter 0 (`LV_EVENT_ALL`) also delivers every draw event (`0x12`–`0x2C`) each frame and does not register the knob; the loader registers one callback per code (1 `PRESSED`, 8 `RELEASED`, `0x2D`) | high |
| Short tap | `PRESSED` at `(200, 140)` but `RELEASED` reports y = 103 for a 125 ms tap (correct after a 0.5 s hold): the menu starts the row that was pressed. Apps see `TOUCH_UP` with that point | high (emulator) |
| Button | unhandled `0x20000001` is ignored; unhandled `0x04000001` makes the stock jump back to the clock (`view_stack: jump 240 -> 16`). The loader handles `0x04000001` only while an app runs | high |
| `OPEN_VIEW(0x10, 0)` | jumps the view stack to the clock and deletes view 0xF0 (msg 4) | high |
| Block placement | opening 0xF0 deletes the face, and the loader (`0x380A9A38`) and app (`0x380A9678`) take the freed face region every time; a block allocated from outside while the face shows cannot move the loader. A block of `app_size` allocated with the menu open moves the app (`0x380A9758`) | high (emulator) |
| Screen-off after BLE | the panel blanks ~5.7 s after `Ble wake lock 0` (the install connection closing) although `keep_awake` set the timeout to `0xFFFFFFFF`; with the screen off no thread enters `os_mutex_lock` and `emu.call` hangs. `HelloTest.ready` waits for it and wakes the watch with the button | medium |

## Plan 2, task 9: stock popups, pause and thread priorities

Disassembly (capstone) of `k_thread_foreach` `0x10199430`, the shell `kernel threads` handler `0x100F137D`, the
thread switch-out hook `0x1004D6F0` and `set_autosleep_time` `0x10149420`; emulator probes
`runs/e2e/task9-probe{,2,3}.log`, `task9-hog-prio{4,14}.log`.

| Item | Value | Confidence |
|---|---|---|
| Thread list | `_kernel` `0x380445B0`, list head `+0x2C`, `k_thread`: next `+0x74`, name `+0x78`, `s8` prio `+0xE`, state `+0xD`, `u64` run time `+0xC0`. The switch-out hook prints `Thread %p %s run time %d ms` for a run over 10 ms at prio ≤ 14 | high |
| Priority map (emulator, after boot) | cooperative: `displayworkq` −12, `BT HCI RX` −10, `BT TX` −9, `BT RX` −8, `prd_tsk_workq` −2, `dspworkq` −2, `sysworkq` −1. Preemptive: `main` 0, `media` 1, `BT MONITOR` 1, `bluetooth` 2, `lvglworkq` 2, a work queue 2, `ui_service` 3, `gps_service` 4, `diskio_cache` 4, `sensor_service` 5, `res_preload` 5, work queues 6, 7, 8, `userworkq` 8, `launcher` 10, `logsrv` 11, `wewear_read` 12, `tcomp_workq` 12, `vendor`, `vendor_read`, `ble_fate`, `fate_read`, `wewear`, `wewear_ftp`, `wewear_ftp_read` and a work queue 13, `standby` 14, `actlog` 14, `shell_uart` 14, `idle 00` 15 | high |
| App priority | `idle` is 15, so `CONFIG_NUM_PREEMPT_PRIORITIES` = 15 and 14 is the lowest priority `k_thread_create` accepts for another thread. Apps run at **14**, equal to `standby`, `actlog` and `shell_uart`. With `motor-hog` spinning: at 4, `wp2apps list` failed after 7.3 s (the vendor/wewear threads at 13 starve); at 14 it answered in 1.5 s, and `standby` still ran (the panel blanked after the BLE disconnect and closed the view, which it did not at 4) | high (emulator) |
| Charge popup | `p 4000 1 60` (charger in) → `msgbox_cache popup … mid:262` → `set_popup_msgbox id:0 - 262, flag:1`: `*(u8*)0x3800FAF4 = 1`, `*(u16*)0x3800FAF6 = 262`; `set_autosleep_time:10000`; the stock buzzes the motor itself (`980 ‰`, 30 Hz, 300 ms). No table lookup or `call` needed | high |
| View under the popup | `view 240 focus 0` → popup view 62 created and painted → `view 240 defocused`, `pause draw`; `view 62 focused`. The `----->view:N ev:[…]` lines come from stock procs, so they do not show what the stub proc receives; the loader treats msg 8/7 and the flag byte the same (`overlay()`). Over the face the popup's `view_cache: shrink` deleted cached views 209/210; over view 240 it deleted nothing (`call shrink 0) ret:0, -3`). The stub proc does get msg 8 on open and msg 7 on close (`StubRam.trace` `0x800F0`, `0x700F0`; fix-final probe, PauseTest) | high |
| Knob sign | `r 80` gives a negative `0x2D` step: the loader sends `CROWN` −1 (vibration-lab Pattern Tick → Long), the opposite of `view_key 0x6F` (+1) in list views | high (emulator) |
| Closing | the button: view 62 gets `0x20000001` and `0x04000001`, `msgbox_cache close` → `set_close_msgbox … flag:0` (byte back to 0) → view 62 deleted → `view 240 focus 1`, `resume draw` (~30 ms). Unplugging (`p 4000 0 60`) also closes it, but sets `set_autosleep_time:5000`; the panel blanks 5 s later and the watch enters S1…S3BT, which stops the emulator (unimplemented read `0x40010000`). `set_autosleep_time` only stores the value (`*ptr = s · 1000`), the armed timer keeps it, so `keep_awake` after the close does not help. The harness closes with the button and leaves the charger in (`dc5v in, wake lock!`) | high |

## Plan 2, final fixes: overlays, swipe back, PWM driver

Probes `runs/e2e/fixfinal-probe{,2}.log`; Ghidra on `0x1012197C`, `0x101214AC`, `0x1012163C`, `0x1014C818`; a
`BL` scan for callers of `0x100D5E34` and `0x100D6460`.

| Item | Value | Confidence |
|---|---|---|
| Msgbox calls | `msgbox_cache_popup(mid, udata)` **`0x100D5E35`**; `0x100D6461 (mid, bsync)` is **close** (`msgbox_cache close … mid:%u, bsync:%u`), not popup. Both are safe to call from any thread through `emu.call` | high |
| Notifications and calls | every stock overlay is a msgbox in `notify_view` (view 62) and sets `0x3800FAF4`: message notifications are mid **252** (`todo_list_msgbox_handler`), the incoming call is mid **246** (`btcall`, `BTCALL_MSGBOX_ID`). Mid 252 without stock message data fails its layout (`ev:25 err:-5`) and closes within 3 ms, so the e2e uses the call box: it opens without data, ignores the button (`btcall_msgbox_key`) and is closed with `0x100D6461(246, 0)`. A real phone notification (ANCS or the wewear app protocol) is not reproduced: its BLE opcode and payload were not found in the time box | high / BLE path not found |
| Swipe back | a right swipe in view 240 arrives as key `0xFFFF0071`; unhandled, the stock pops the view stack (see "Back navigation" below): opened with `OPEN_VIEW` from the clock that is `jump 240 -> 16`. A btcall/msgbox close holds `gesture lock` for a few seconds |
| PWM driver | `pin_set_cycles` `0x1012197D` logs through the Zephyr log core (`0x1014C818`) and selects the pin (`0x1012163C`) before it raises `BASEPRI` to `0x20` for the register writes; `off` `0x101214AD` writes the control word and logs. No `k_mutex`/`k_sem` seen in these, but the log path is not proven non-blocking, so the loader does not call the driver under `BASEPRI`. Each `pin_set_cycles` first writes control `0` and then enables: `motor.log` shows a `0` line before every re-set in the same ms | medium / log path |
| Tap vs drag | LVGL 8.3 sends `CLICKED` only when no object scrolled during the press. The loader menu (LVGL objects since the menu rebuild) keeps its list always scrollable (bottom padding lets the last row rise to the title), so a drag over a row scrolls and launches nothing; a short tap whose release point is off (y 140 → 103) still clicks the pressed row (`PRESS_LOCK`) | high (emulator probe `runs/menu-lvgl/probe.log`) |

ABI 2 note (final review M7): `wp2_core::events::Queue::clear` moves `head` to `tail` with a plain store while `pop`
CASes `head`; a consumer preempted for ≥ 2 clears plus 14 pushes can see `head` back at its old value and pass the
CAS (ABA). Unreachable in ABI 1 (`clear` only on launch and pause); an ABI 2 queue should carry a generation
counter in `head`.

## Back navigation: view stack and the back swipe

Ghidra (`scripts/recon/decomp`) on `range:0x10189140-0x10189A00`, `callers:0x101897fc`, `callers:0x101896b8`;
emulator probe with gdb breakpoints on the five stack functions (`runs/back/probe1.log`).

| Item | Value | Confidence |
|---|---|---|
| View stack | array of 10 entries × 0x10 B `{u32 presenter, u32 group, u32 arg, u16 view_id, u16}` at the literal in `0x10189444`, `u8` count at `+0xA0`, bit 0 of `+0xA1` = initialised; mutex `0x101BE218`/`0x10189148`; `0x101891F8 (from, to)` does the transition (deletes/defocuses `from`, opens `to`) and logs `view_stack: jump <from> -> <to> (res=…)` for every kind of move | high |
| Calls | push **`0x101898BC (id, arg)`** (`OPEN_VIEW`, `0x10189398(entry, 1)`), **pop `0x101897FC ()`** (count ≥ 2: top → top − 1), jump-to `0x101896B8 (id, presenter, arg)` (pops to `id` if it is in the stack, else pushes), reset `0x101895A0 (presenter, id, …)` (stack = one entry), remove `0x10189638 (n)`, depth `0x1018949C`, top id `0x101894A8` | high |
| Launcher key handler | **`0x1005DB08 (view_id, key)`** gets every key no view handled (byte `+4` of the view's key message left 0). Right swipe: `(key & 0xFFFF) == 0x71` and view not in `100..105` → `0x101D7358()`, **pop**. Button `0x04000001`: depth 1 → clock or app list (`0x1005E834` from the clock); deeper → `0x1005DAAC`, reset to the clock (`jump N -> 16`). Crown rotation `0x1E/0x1F` keys and two configurable side-key shortcuts are handled here too | high |
| Stock apps | no own back code: a stock app opened from the app list is `[16, 18, app]`; its swipe falls through to the launcher and pops to 18 (`jump 106 -> 18`, Exercise, from `0x1005DD83`); the crown press resets to the clock (`jump 106 -> 16`, via `0x101AD159`) | high (emulator) |
| View 0xF0 | the "Custom Apps" row pushes 240 the same way (`[16, 18, 240]`), so the stock already treats the menu like a stock app: swipe → `jump 240 -> 18`, the list is rebuilt with its saved scroll ("Custom Apps" back at the top, `runs/back/probe-menu-swipe.ppm`); button → `jump 240 -> 16` | high (emulator) |
| Running app (before the fix) | the loader left `0x71` unhandled, so a swipe in an app popped 240 straight to the app list: the app thread was aborted by the view delete (no `BUTTON`, no save) and the menu was skipped | high (emulator) |
| Fix (loader only) | `map_key` maps any key with low half `0x71` to `BUTTON` while an app runs and marks it handled, so the launcher never sees it: the app gets `BUTTON`, the 1 s escape stops it if it does not return, the menu shows. In the menu the key stays unhandled (stock pop to the list). The stub is unchanged | high (emulator) |

## Hardware: caches and MPU

Ghidra (`scripts/recon/decomp`) on `0x10142C80`, `0x100F0084`, `0x101BEE00`, `0x101BEE52`, `0x10199CC8`,
`0x100F6708`, `0x100F67CC` and callers; capstone on `0x100F0072…0x100F00F4`, `0x100F6804`, `0x101053C0`,
`0x10105534…0x10105C88`; a literal scan for `0xE000EF50…0xE000EF7C`, `0xE000ED14`, `0xE000ED9x`; a scan for
`BL` to the functions below and for `MSR CONTROL`.

| Item | Value | Confidence |
|---|---|---|
| Core caches | none used. No literal in `0xE000EF50…0xE000EF7C` (ICIALLU, DCIMVAC, DCISW, DCCMVAC, DCCIMVAC), and the one `0xE000ED14` literal (`0x10105418`) is the `SHPR` base of `NVIC_SetPriority` (`0x101053E4`, `[0xE000ED14 + (irq & 0xF)]` for irq < 0), not a CCR write. The Cortex-M33 has no architectural L1 cache; Zephyr is built without cache support | high |
| PSRAM cache | the vendor SPI1 cache in front of PSRAM. Ops register `0x40018004` (bit 0 start/busy, bit 1 range, bit 3 invalidate, bit 4 write back; `0x1F` with bit 2 is the write-buffer flush), range `0x40018008` start / `0x4001800C` last byte, both CPU addresses. Driver `0x100F0084 (op, addr, size)`, no-op for `addr` outside `0x34000000…0x3FFFFFFF`: op 1 write back range (`0x13`), 2 invalidate range (`0x0B`), 3 write-buffer flush (`0x1F`), 4 write back all (`0x11`), 5 invalidate all (`0x09`), 6 write back + invalidate range (`0x1B`), 7 same for all (`0x19`); range ops and 3, 5 busy-wait on bit 0 (`0x100F0074`) | high (code), bit names medium |
| Mappings | `0x38…` is write-back cached: `0x101BEE00 (addr, size)` does op 3 + op 1 for `0x38000000…0x3BFFFFFF` and only op 3 for `0x34…`/`0x3C…` (write-through and uncached). This matches the alias table above | high |
| Stock routine used | **`0x101BEE52 (addr, size)`**: under the k_mutex `0x380040D0` (`0x10101BDC` → `k_mutex_lock` `0x10199CC8`, asserts when called from an ISR) it runs op 3 then op 6 for `0x34000000…0x3BFFFFFF`, op 3 alone for `0x3C…`, returns 0. Stock calls it from UI and GPU paths on PSRAM buffers before DMA (`0x100FFA70`, `0x10142574`, `0x1018898C`, `0x101CD670`) | high |
| `lv_gpu_acts_flush_dcache` (`0x10142C80`) | only widens a dirty-range record and forwards; no cache operation | high |
| Loader/app load | `wp2_core::load::load` calls `0x101BEE52(block, image_size)` after relocation and CRC, with `dsb` before and `dsb; isb` after. The stub loads the loader through it, the loader loads apps through it. Callers run in the UI thread, never in an ISR or under `BASEPRI` | decision |
| MPU setup | boot `0x100F6804` (prints `arm mpu init`) and PM resume `0x101BAC78` call `0x100F67CC`, which sets three regions through `0x100F6708 (idx, base, size, type)`. The first region enables the MPU: `MAIR0 = 0x44444444`, `MPU_CTRL = 5` (ENABLE + PRIVDEFENA). `RBAR = base | attr`, `RLAR = (base + size − 0x20) | 1`; attr `0x1F` type 0 (RO, XN), `0x1A` type 1 (RW), `0x1E` type 2 (RO, executable) | high |
| MPU regions | 0: `0x00000000…0x0000FFFF` RO exec; 1: `0x10000000…0x10FFFFFF` RO exec (NOR XIP); 2: `0x38000000…0x38000A77` RO exec. None is XN and none covers `ResFbHeap` `0x380A9340…0x3871EEC0`; that range falls to the privileged default map (SRAM region, executable). Other regions come only from the debug shell `mpu_set` (`0x100F3F7E`) | high |
| Thread privilege | all threads privileged: the only `MSR CONTROL` writes (`0x10105544`, `0x101055E0`, `0x10105C80`, `0x10253642`) clear FPCA or set SPSEL, never nPRIV; no userspace syscall strings | high |

Unverified on hardware: whether the SPI1 cache serves instruction fetches from `0x38…` through the same lines as
data (if it does, the write back alone would be enough; the invalidate costs only refills). The emulator does
not model the controller (stock calls it during normal drawing, so its writes are accepted there).

## ABI 1.1: host channel, time, battery, wake lock, sensors

Ghidra (`scripts/recon/decomp`) on the functions below, capstone on `0x100CBF84`, `0x100D343C`, `0x10147748`; the
ux2sys property table `0x101F4274` dumped (0x91 entries `{set, get, u16 size}`); emulator probes
`runs/api/probe{1,2,3}.py` and `tests/test_api_e2e.py`. Design and API: `docs/abi-1.1.md`.

| Item | Value | Confidence |
|---|---|---|
| BLE dispatch thread | `wewear_read` (prio 12, 12 KiB stack, created by `wewear` `0x100CC150`): loop `0x100CBF84` reads an 11 B header with the stream read `0x10147748` and calls `0x100D343C`; it calls `os_sleep(10)` (`0x100CBFC8`) only while the stream is not connected. Context `*0x3801A658`: `+0` the thread, byte `+4` bit 0 connected, `+0x18` the stream | high |
| Stream read | `0x10147748` blocks while connected: the stream's own read (`+0x50` ops `+8`) waits; its 50 ms semaphore loop (`0x10147786`) is not reached in the emulator. A hook in either place ran about once per 5 s | high (emulator) |
| Stream write | send `0x100CC2D4` → `0x10147890`: waits up to 21 × 50 ms for room, writes all or nothing; free space `0x101CE411(stream)`, negative when closed | high |
| Crypto buffers | encrypt `0x100CCB34` / decrypt `0x100CCC0C` use static buffers and the AES engine, so all stub frames go out from `wewear_read` | high |
| Opcodes | `c1 == 0xFFFF`, `c2 > 0x908B` → `default` (`0x100D34AC bhi`): `0x90A0..0x90A7` reach only the stub | high |
| "Paired" popup | every bind (`0x8047` + `0x8049`) opens msgbox 270, then 267 ("Paired", `reset_work dur:1 sec`) which stays until a key closes it; its delete handler runs `launcher ui_enter clock view` (jump to the clock, view 0xF0 deleted). Reusing the session key without a bind shows nothing | high (emulator) |
| Charge popup | on the charger, msgbox 262 opens again at every screen wake | high (emulator) |
| RTC | `0x100C14B4(struct rtc_time *)` calls the RTC driver (`*0x2FFB44A0`, api `+8` → `+8`), then year + 1900, month + 1: `{u8 sec, min, hour, mday, mon, wday, u16 year, u16 ms}`; holds local time (log `rtc_time_d/_t`). `0x100C1488` = RTC as epoch minus zone | high |
| Zone | `0x100C1458` → `s8 h · 3600 + s8 m · 60` from `0x2FFB449C` (`gSysTimeZone`); set by the phone time sync (`0x100D2A0C`). Emulator fixture: 86,400 (24 h, never synced) | high |
| Battery | power supply props through `0x101CE6FC` (prop 3 capacity), `0x101CE702` (prop 0 status: 3 charging, 4 full, 2 discharging), `0x101CE70E` (prop 4 DC5V); ux2sys `0x0A` (3 B: percent, status code, low flag) adds low-battery bookkeeping, `0x0B` = DC5V > 0 | high |
| Wake locks | `0x101B59A8(id)` sets bit `id` in the words at `0x38007708 + 0x40 + 4 · (id / 32)` and takes the full wake lock on the first one; `0x100C1858(id)` clears it, the last one calls `sys_wake_unlock` (restarts the auto-sleep time). `0x100C1738` (AT `SETWAKELOCK 0`) then blanks at once when the timeout ran out. Ids are view ids (`0x1C`, `0x43`, `0x63`, `0x6B`, `0xF6` …) plus `0x113`, `0x11F` (AT), `0x123`; `0xF0` is unused | high |
| ux2sys properties | get `0x100E082C(prop, out)` and set `0x100E08B8(prop, in, size)` assert (and reset) on an id > `0x9E` or a NULL handler. `0x06`/`0x07` screen-on and active time (`0x07` → `set_autosleep_time`), `0x08` temporary active time, `0x0C` BT connected, `0x2B` g-sensor sample, `0x62` GPS info, `0x63` GPS mode, `0x02` HR mode | medium |
| Accelerometer | property `0x2B` copies 24 B from `0x3800C9C8`; with emulator input `a 100 -200 1000` it stayed zero without a stock consumer | high (emulator) |

## Screen-off and the app view

Ghidra and capstone on the launcher's message handler (`0x1005DE00…0x1005E290`, literals at `0x1005E4D8…`),
`0x100677C0`, `0x10067828`, `0x100E082C`; emulator probes with a BLE session holding the guest in S1
(`tests/test_back_e2e.py`, `ScreenOffTest`).

| Item | Value | Confidence |
|---|---|---|
| Early-suspend | the launcher sends message `0x82` to the top view (`0x101D72CE(top, 0x82, 0)`, top from `0x101894A8`), then reads ux2sys property `0x6E`; non-zero → "AOD enter" `0x100677C0`: opens overlay `0x113` (`0x100D5E34`) over the current view. No view is deleted | high (static) |
| AOD overlay = msgbox | AOD enter calls `0x101B9EEE(0x113, 0)` → `0x100D5E34` = `msgbox_cache_popup`, the same path as the charge popup (mid 262): the AOD overlay is stock msgbox `0x113` and sets the popup flags `0x3800FAF4` (bit 0 → id at `+2`, bit 1 → id at `+4`, `set_popup_msgbox` `0x100D6EB0`, base `0x3800FAC0`). AOD exit is `0x101B9EF6(0x113, 0)` → `0x100D6460` (close msgbox). A loader that pauses on any popup flag pauses the app on every AOD | high (static); calling AOD enter in the emulator was not reached (the ui thread takes no lock while dark) |
| Late-resume | "AOD exit" `0x10067828` closes overlay `0x113`; if the view stack is empty (`0x1018949C` = 0) it resets to the clock (`0x1005DAAC`), else it sends `0x83` to the top view. The stack `[16, 18, 240]` stays | high (static) |
| Suspend / resume | only set the launcher's state bits (`+0x1E` bits 0–1) and log | high (static) |
| Wake key | the press that wakes the panel is dropped by the stock (`drop key workup`), the app gets no `BUTTON` | high (emulator, S1) |
| App across blank | with a BLE session (S1): `display blanking on` → `Exit S1` → `blanking off`; the app keeps running, view 240 focused, an LVGL app's screen is back (`runs/wake-probe/lv-demo-woken.png`); a motor buzz started while dark fires | high (emulator) |
| Not modelled | deep sleep (S2/S3BT) stops the emulator ("Guest hit a fatal or unsupported path"); forcing the AOD branch (`r4 = 1` at `0x1005E120`) does too. Both need a hardware check | — |
| Earlier note | "the stock closes the app view on auto-sleep" (`docs/herdr-agents.md`, `docs/app-guide.md`, 2026-11-11) came from emulator runs; no stock path that deletes view 240 on blank was found | medium |

## Music tile (sport1_view)

Ghidra on `0x10076F00..0x1007A400` (`sport1_view`, id `0x64`, desc `0x101ED554`: data `0x504` B, scene
`0x6880AE34`, layout `0x10079148`, paint `0x10078B2C`, preload `0x10078044`).

| Item | Value | Confidence |
|---|---|---|
| Tiles | `lv_tileview` with tiles (0,0) dir 2, (1,0) dir 3, (2,0) dir 1; it opens on (1,0). Music is (2,0) | high |
| Scene resources | `SCENE_OPEN 0x10184804(scene id, &{id, node, res}, "/NAND:A/bt_watch.sty" 0x10203EA6, "/NAND:A/bt_watch.res" 0x10203E91, lang 0x100DFB88())` (refcounted, as popups open theirs); close `0x101D7714(&scene)`; images `0x101848D4(&scene, ids, lv_img_dsc_t[n], 0, n)`, free `0x101D775A(dsc, n)`; texts `0x10184A5C(&scene, ids, {char *, u32}[n], n)`, free `0x101D777A(texts, n)` | high (emulator) |
| Ids | 22 images at `0x101ED460` (6 pause, 7 previous, 8 next, 9 volume, 10 play, 12 grey play), 3 page dots at `0x101ED4F0`, 22 texts at `0x101ED4FC` (14 "No music") | high (emulator) |
| Objects | `simple_img` (`0x1005A014`/`0x1005A02C`) play `TOP_MID 0,173`; previous `align_to(play, OUT_LEFT_MID, -32, 0)`; next `OUT_RIGHT_MID, 32, 0`; volume `BOTTOM_MID 0,-44`, all clickable; labels 266 px × one line, `SCROLL_CIRCULAR`, title `TOP_MID 0,44` sans32 anim speed 20, artist `align_to(title, OUT_BOTTOM_MID, 0, 12)` sans32 colour `0x9492`, "No music" `TOP_MID 0,44` font32M; page dots image `TOP_MID 0,440` | high |
| Fonts | opened by `0x100DF9A0(lv_font_t *, path)`: `lvgl_bitmap_font_open` plus per file: font32M and font28M line height +6, font32R and font24M +4, `/NAND:C/sans32.font` gets the emoji28 fallback; `LVGL_BITMAP_FONT_CLOSE` frees the fallback | high |
| Paint (`0x101AF03C`) | `0x4C` set: title/artist from `0x4F`, play image pause (`0x4D` set) or play, "No music" hidden; else title/artist hidden, "No music" shown, grey play | high |
| Taps | previous/next send `0x4E` 3/4, play `0x4E` 1 or 2 by its own flag, only with `0x31` and `0x4C` set; else a "connect the phone" popup; volume opens a volume popup (`0x50`) | high (static) |
| Volume popup (`0x100786DC`) | once, on the view's root: `lv_obj` 466 × 466, bg black opa 255, `CENTER`, no `GESTURE_BUBBLE`; `lv_img` images 15 (volume 0) / 16 at `TOP_LEFT 173,45` (the whole pill with its + and − icons); `lv_slider` 12 × 148 `CENTER`, `BG_IMG_SRC` (`0x28`) image 17, indicator (`0x20000`) image 18, range 0..max, value current, not clickable; two `lv_btn` 120 × 96 (flag `CHECKABLE`) `TOP_MID 0,45` / `BOTTOM_MID 0,-45`, CLICKED → `0x100772EC(5 / 6)` = `0x4E` volume up / down; on the overlay `0x2D` (knob) and `0x0C` (gesture) callbacks, clickable, no `SCROLLABLE`; then `0x10182238(0)`, `0x1E` on close. Shown again if hidden | high (static, emulator frame) |
| Volume (`0x50`, 2 B) | `{max, current}`; the popup shows current (0 when max is 0) | high (static) |
| Popup crown (`0x10078FE4`) | counts knob events per direction, six make one `0x4E` 5 (step > 0) or 6, with `0x26 {8}` (vibrate) first while current < max (up) or > 0 (down) | high (static) |
| Popup close (`0x101AEFF8`) | gesture dir 2 (right): hide the overlay, `lv_indev_wait_release`, `0x10182238(0x1E)`. The paint refreshes icon and slider once a second while it shows | high (static) |
| Emulator | images 17/18 are `TRUE_COLOR_ALPHA` (cf 5); drawing them stops the emulator ("fatal or unsupported path") for the stock popup too. With cf 4 in their descriptors both draw | high (emulator) |
| Centred text | the labels draw centred, inherited (no `text_align` on them); the loader sets it on its container | high (emulator) |

## Return to watch face

Capstone on `0x1005D968`, `0x100C27CC…0x100C28D0`, the launcher message handler (`0x1005DEDE`, `0x1005E128`,
`0x1005E1B0`) and the settings view `return_watch_view` (`0x100BF66C`); emulator: `tests/test_back_e2e.py`,
`ScreenOffTest.test_app_survives_return_to_face`.

| Item | Value | Confidence |
|---|---|---|
| Setting | ux2sys **`0x9C`**, seconds: 30, 120 or 1800 (the three rows of `return_watch_view`) | high (static) |
| State | `jx_return` struct **`0x38019B10`**: `+0` callback, `+4` mutex, `+0x18` timeout ms, `+0x1C` suspend time. Launcher init `0x100C27CC(0x1005D969)` sets the callback | high (static, emulator) |
| Early-suspend | after `0x82` to the top view: `0x100C2808` logs `suspend`, stores the time (`0x100F96C0`, ms, the log clock) and `0x9C` × 1000 | high (emulator) |
| Late-resume | first `0x100C286C` (`resume, <t> - <elapsed>`): elapsed ≥ timeout and callback ≠ 0 → callback; then AOD exit and `0x83` | high (emulator) |
| Callback | `0x1005D968` (`jx_return_user_handle`): stays for 16 top views (table `0x101EAED8`: the sport views `0x64…0x69`, `0x6B`, and `3`, `0xF`, `0x63`, `0x88`, `0x1C`, `0x51`, `0x52`, `0x9F`, `0xA9`), for music `0x90` while it plays, or under a message box; else `return clock, stack_num:%u` and a jump to the clock (`0x101AD140(…, 0x10, 1)`), which deletes view 240 | high (emulator) |
| Why only with heart rate | in deep sleep the time of `0x100F96C0` does not advance (assumed: a counter that stops with the main clock), so before heart rate the elapsed time stayed below 30 s and apps survived long waits. The measurement takes a system wake lock (`wake_lock: 0 3`), the watch stays in light sleep, the elapsed time counts and the next wake closes the app. With the screen held on (rest) there is no suspend at all | medium (hardware symptom, not measured) |
| Loader | while an app runs the loader writes 0 to the callback and `0x1005D969` back otherwise and on view delete; the menu (view 240 without an app) returns as before | high (emulator) |

Emulator: with the panel dark under a BLE session (S1) the earlier 90 s probe did not return (the paired NOR's
`0x9C` value was not read; 120 or 1800 s would explain it); with `+0x18` set to 1000 ms after the blank the old loader went `return clock, stack_num:3`,
`jump 240 -> 16`; the new loader keeps view 240 and the app.

## Project 2: the stub's own channel key K

- `AT GETSECRET` is handled by `0x100E9DD0`: `0x1010E215("APP_SECRET", buf, 32)` reads the pairing secret from NVRAM
  (32 ASCII hex chars) and prints it with `GETSECRET:%s,OK`. `0x1010E215(name, buf, len)` is the NVRAM read; it
  takes the NVRAM lock (`0x1010D81C(-1, -1)`, released by `0x1010D810`) and tries two stores (`0x38001B50`, then
  `0x38001B18`). Returns the bytes read or < 0. Safe from any thread.
- The stub's `K` is that secret as 16 bytes. Frames under `K` are tried only after the stock decrypt rejects them
  (the stock logs `check_extra_data fault`, harmless). Proof: `tests/test_chan_key_e2e.py` (a `K` host with no
  bind, a stock host and a `K` host on one link, install under `K`; no popup).
- The loader 1.3 on the watch lets an app `send` only while `session_word() == host_key` (`fw/wp2-loader/src/svc.rs`).
  So under `K` the stub also stores the stock session word in `host_key`: a `K` host counts as listening while
  the stock session next to it stays the same. A watch that was never bound since its last factory reset has a
  zero session word and app messages to a `K` host fail with `-107` (any watch paired with Nothing X is bound).
- On a shared link every host sees every notification: a host must skip frames and `EVENT`s its key cannot open
  (`link.Session.request`, `channel.recv`).

## Heart rate

Ghidra (`scripts/recon/decomp`) and capstone on the functions below, the ux2sys table `0x101F4274` and the view
table; emulator probe `scripts/recon/hr_probe.py` (frames in `runs/hr/`, not tracked). The emulator has no PPG
model, so every reading here is a synthetic value written with gdb. API built on this: `docs/abi-1.4.md`, "Heart
rate". Heart-rate values are personal data: never log, commit or report a real one.

| Item | Value | Confidence |
|---|---|---|
| Reading | **u8 `0x3804B310`**, bpm, 0 = none. Getter `0x100E2364`, setter `0x100E2358`. The only non-zero writer is `hr_callback` `0x100E2EE8` (pointer at `0x100E233C`), type 5 at `0x100E3308`: `bpm = *(u32 *)(data + 4)`, a zero is skipped, else `strb` at `0x100E337A`; the same branch updates the day statistics (`0x380077F8 + 0x43B8…`, per-30-min array) and the HR alarm check. Zero writers: heart view layout `0x100704D4`, breath view `0x1008AC54`, `0x1006F20C` ("heart senor mode set") | high (emulator) |
| On the wrist | **u8 `0x3804B30C`**, non-zero = worn (ux2sys `0x13`, getter `0x100E2C30` logs on every call). 1 in the emulator. Not worn: the heart view's 2 s timer `0x10070984` opens `heart_warn_view` (`0x15`), the workout screen shows `--` | high (code), not-worn path not run |
| Owner | **u8 `0x3804B30F`**: 1 = property 2 (heart view, now apps), 2 = a stock workout (`0x100E2340(2)` in `0x100C9434`), 0 = none. `hr_callback` branches on 2 (`0x100E331C`) | high (emulator) |
| On / off | ux2sys property **`0x02`** "change_hr_mode" (u32), setter `0x100E2370`; the stock heart view calls it through the synchronous `0x100E07B8(prop, *in)`. 1: owner 1, `0x101964C8(5, 0)` (log `jxen_algo_req id:5`) → `0x10196460(4, 5, 0, 0)` posts message `0x23`; 0: owner 0, `0x10196510(5, 0)` → `(5, 5, …)`. It also sets the u16s `0x3800263A`, `0x3800263C` to `0xFFFF` (purpose unknown) | high (emulator) |
| Request count | the message handler counts requests per algorithm: enable `0x100CB160`, disable `0x100CB2A8`, **i8 `0x38048D16 + id`** (id 5 = HEARTRATE) plus per-consumer counters (table `0x101F33B4`, 19 × 28 B, entry 8 for id 5). The sensor runs while the count is above 0. Every start needs exactly one stop: a first loader draft that asked again every 10 s left the count at 1 after the app's exit and the driver kept measuring (UART `jxdis_algo_ret 5) 1 -> 0`, no `VC9202 Stop`) | high (emulator) |
| Driver | VC9202: start `0x10104E66`, stop `0x10104F5C`, one flag per work mode, printed `m<mode> h s a t`. HR is mode 0 (flag `h`); the stop of a mode drops its flag and picks the next one still set. Mode 3 (`a`) is on from boot in the paired NOR (purpose not checked) | high (emulator) |
| Background measurement | `heart_period_enable` `0x100E6078`, manager tick `0x100E61BC` starts mode 5 under its own flag (`0x3801B1B8 + 1`) and stops it through `hr_manager_disable` `0x100E6120` after 60 or 120 tries. It goes through the same request count, so a balanced app and the background do not cut each other | medium (code; the background run was not caught in the emulator) |
| Heart view | `heart_view` id **`0x14`** (handler `0x100701AD`): layout clears the reading, sets property 2 to 1 (log `heart senor mode set 1`), repaints every second (`0x101AE99A` → paint `0x100702BC`, reads `0x100E2364`; 0 shows "Measuring"); delete `0x101AE876` sets it to 0 if the view had set it. `app_heart_view` is `0xB2`, `heart_warn_view` `0x15` | high (emulator) |
| Heart view: screen | focus (`0x101AE8DC`, view table `0x101ED050`): `0x100C16F0(25, 1, 0x14)` (auto-sleep 25 s while id `0x14` holds it, `set_autosleep_time:25000`), then property 2 = 1; defocus and delete: `0x100C1858(0x14)` (default auto-sleep back) and property 2 = 0. `0x100C16F0(s, keep, id)` stores `s` and calls `0x100C1634(0, keep, id)`: a soft hold (bit `id` in `0x38007748…`, `keep` marks it as not blocking the auto-sleep check `0x100C1738`) that still counts in `+0x3C` of `0x38007708`; a hard lock (`WAKE_LOCK`, type 1) taken meanwhile is dropped only when that count reaches 0 (`0x100C1858`), so a soft hold kept for minutes keeps the panel on after any `screen_on` (workout e2e `test_1c`, 2026-11-14). The loader does not take it. Messages `0x82` (early-suspend) set property 2 to 0 and `0x83` (late-resume) to 1 (`0x101AE7AE`): the stock never measures with the panel dark through property 2 | high (code, emulator) |
| Start path compared | emulator UART, loader start vs heart view: the same lines (`change_hr_mode :1`, `jxen_algo_req id:5`, `jxen_pref 8) 1`, `os_hr_set_work_mode_handler mode=0`, `jxen_algo_ret 5) 1 -> 1`, `VC9202 Start Later---m0 h1`). The heart view adds only the 25 s hold and the restart per wake. `hr_callback` is registered once at boot (`0x100E01F0` → `0x100E2308`, message `0x23`/2), with no gate on the view or a subscriber; the wear and HR getters are plain reads | high (emulator, code) |
| HR watchdog | the 1-min task (`0x100E7748`, `_1min_task_proess`) posts `0x23`/9 (`0x101DA9E0`) while view 16 exists (u32 `0x3801969C` = its view data; 0 under an app in the emulator); `check_hb_is_exceptional` `0x100CB470`: per check with the HR consumer count (`0x3801A5BC`) above 0, u32 **`0x3801A5D4`** + 1; at 5: `jxclose 5` (every request dropped) and `only_reserve_wearing_detection` `0x10105090` (VC9202 to mode 3). The owner byte stays 1, the reading is never written again. Proven by calling `0x101DA9E0` five times under an app (emulator) | high (emulator), not a cause under an app |
| Strength training: prepare | `sportlist_view` `0x6A` → `sport_prepartion_view` `0x6B`, the GO screen with "❤ bpm" under the clock: property 2 = 1 (owner 1), shows the **raw reading** | high (emulator) |
| Strength training: run | GO: the GO screen sets property 2 to 0, sport start `0x100C9434(1, type)` sets owner 2, `0x101964C8(5, 0)` and a 1 s sport timer; sport end: owner 0, `0x10196510(5, 0)`. `sport1_view` `0x64` (paint `0x10078B2C`) shows the HR from the sport snapshot `0x380197FC + 0xC` (ux2sys `0x34`, a copy of `0x3800CAB0`; byte `0x3800CABC`), written at `0x100C86C8` from the sport algorithm's float output, not from the raw reading. `--` while the wrist byte or the value is 0 | high (code), emulator in part |
| Screen | property 2 does not hold the screen: with an app's measurement on, auto-sleep (1.5 s) blanks the panel and no wake-lock id is set (`0x38007748…`, emulator). The HR enable takes only a system lock, `wake_lock: 0 3` from `0x100CB0C5` (type 0; type 1 is the screen lock that `0x101B59A8(id)` takes). The stock Strength training run screen also went dark during the workout (emulator) | high (emulator) |
| CPU | with the measurement on and no PPG model, 28 of 30 gdb samples of the current thread were `idle 00`, kernel uptime kept pace with guest time | high (emulator) |
| Emulator RTC | the RTC (`0x100C1488`, `api.time()`) advances about **0.43 s per guest second**, the same with the measurement on or off (4 windows of 60 s: 25–27 RTC s); kernel uptime matches guest time. The model does not tick the calendar registers (`0xC014…0xC01C`), the stock driver keeps them. Anything timed in RTC seconds runs about 2× long in emulator tests (workout now reads the RTC once per launch and counts on with `now()`); a real RTC runs on its crystal | high (emulator) |
| Battery | the PPG LED runs while the count is above 0. The stock holds it only while its heart or GO screen is open or a workout runs. An app's measurement also feeds the day statistics, as the heart view's does | medium |

Emulator proof (`scripts/recon/hr_probe.py heart`, stock unpaired NOR with the stub): opening `0x14` logs
`change_hr_mode :1`, `VC9202 Start Later---m0 h1`, owner 1, reading 0 ("Measuring"). A poke of `0x3804B310 = 67`
draws "67 BPM" (`runs/hr/2-heart-67.png`); calling `hr_callback(5, {0, 72})` (on `ui_service`) draws "72 BPM" and min/max 72
(`3-heart-72.png`). The button closes the view: `change_hr_mode :0`, `VC9202 Stop Later---m0 h0`, owner 0.
`scripts/recon/hr_probe.py strength`: the GO screen of Strength training draws the poked 67 under the clock
(`runs/hr/s3-4.png`); after GO owner is 2, the run screen shows `--` with the raw reading at 68, and a poke of
`0x3800CABC` is overwritten within a tick.

Hardware (2026-11-14, owner): the stock heart view shows a reading after a few seconds; an app's measurement
(loader start as above, panel on its 5 s auto-sleep, measuring on with the panel dark) never wrote one (owner 1,
wrist 1, reading 0 for tens of seconds). The loader now stops on `0x82` and starts anew on `0x83` as the heart
view does (`fw/wp2-loader/src/hr.rs`); the heart view's 25 s hold was tried and dropped (it kept the panel on,
above). Not proven on hardware; the leading guess is that a measurement left running across a dark panel does not
deliver, and that 5 s of lit panel may be short for a first reading.

Not proven: real PPG readings and their rate (the stock heart view repaints once a second), the not-worn path
and whether the reading drops to 0 off the wrist, the sport algorithm's input (`hr_callback` calls `0x100CA284`
with the bpm), the stock workout's pause, the battery cost. Calling `hr_callback` on whatever thread enters
`os_mutex_lock` first stopped the emulator twice ("Guest hit a fatal or unsupported path"); the probe calls it on
`ui_service`.

## GPS

Ghidra (`scripts/recon/decomp`) on the functions below, the ux2sys table `0x101F4274` and the AT table
(`0x101F4E30…`); emulator probe `scripts/recon/gps_probe.py` (`modes`, `workout`; frames in `runs/gps/`, not
tracked) with the emulator's GPS model (UART1 NMEA: `$GPRMC` and `$GPGGA` for the position given by
`g 1 lat_e6 lon_e6`, fix 1, 8 satellites, HDOP 0.9, altitude 0.0, speed 0.0; one GGA per ~1 s). Every position
here is synthetic. API built on this: `docs/abi-1.4.md`, "GPS". GPS positions are personal data: never log, commit
or report a real one.

| Item | Value | Confidence |
|---|---|---|
| Receiver | Airoha **AG3352** (`gps_3352` log tag) on UART1 (`0x4003C000`, IRQ 23), power pin on GPIOB (`ag3352_poweron` `0x100FC900`, 1 on, 0 off), commands in Airoha `$PAIR…` NMEA (`0x101978FC` writes them). Zephyr device `"gps"`: ops `+4` enable (`gps_enable ok`, `0x101975E4`), `+8` disable (`gps_disable ok`, `0x101976C4`), `+0x28` set mode, `+0x2C` host download (firmware) | high (strings, emulator UART) |
| Thread | `gps_service` (prio 4, stack `0x800` at `0x38016D08`, entry **`0x10198178`**, thread table entry `0x1000049C`): message `0x23` with sub-op 0 = enable (mode in the payload), 1 = disable, 2 = an NMEA line from the UART, 5 = EPO download done; messages 3 (init), 5 (`GPS SUSPEND_APP`: disable), 6 (`GPS RESUME_APP`) | high (code) |
| Mode, property **`0x63`** | ux2sys setter **`0x101B9B4E`** (u32): 0 → `0x101DAABA` (message `0x23`/1, disable), else `0x101DAAAE(mode)` (`0x23`/0). No getter. `AT SETGPSSWITCH n` (`0x100EA254`) accepts 0..5 and calls the setter through `0x100E08B8` | high (code, emulator) |
| Mode state | **u8 `0x380443F4`** `[0]` EPO counter, **`[1]` mode in force (0 = off)**, `[2]` applied mode, `[3]` special mode (2 = EPO download, 3 = host download: a disable is refused while set, `…do not disable !!!`), `[4]` recovery mode (restored after EPO). Enable with `[2]` = 0 powers the AG3352 on (`ag3352_enable_poweron`, `gps_enable ok`); disable powers it off (`ag3352_disable`, ~1.5 s later `power pin false`) and zeroes the info block | high (emulator) |
| Modes | **0** off. **1** normal: the stock workouts' mode; at enable also `set_rtc_time_location` (sends `$PAIR590` time and `$PAIR600` last location to the receiver as aiding data) and `auxiliary_gps_request` (asks the phone for EPO/aiding, `0x100D28DC` sends `0xA06A`); at the first NMEA `$PAIR064,200` (`0x10197D40`). **2** EPO download, **3** firmware host download (special modes, not for apps). **4** `$PAIR512` + `$PAIR130,0,FFFFFFFE` …, **5** `$PAIR512` + `$PAIR130,0,8FF7DFDF` … (PAIR130 = per-constellation satellite masks; which constellation set 4 and 5 pick, or whether 4/5 trade accuracy for power, is not known). In the emulator 1, 4 and 5 give the same fix | high (code) for 0–3, low for what 4 and 5 mean |
| Ownership | **no request count**: one mode, last writer wins. `sportlist_view` `0x6A` sets 1 at layout (`0x1007EF3C`, log `sport_view_set_gps_enable 463 1`), `sport_prepartion_view` `0x6B` sets 1 again for a GPS sport (`sel_sport_type = 2 need_close_gps_flag = 0`, `_display_gps_propartion_view`); leaving them sets 0 (`0x1007B1FC`, `0x1007EC00` if the view had set it). The factory GPS test view (`GPS(%d)`, `0x10091244`) sets 0 at close. Whoever sets 0 turns the receiver off for everyone | high (emulator, code) |
| Info block | **`0x38016C40`**, 0x68 B, filled by the NMEA parser **`0x10197D40`** (minmea formats at `0x10198C64` …): at each **GGA** (format `tTfdfdiiffcfcf_`) it is cleared, then **f64 latitude `+0x38`**, **f64 longitude `+0x40`** (degrees, signed), **f32 altitude `+0x48`** (m, GGA field 9), **u32 fix quality `+0x4C`**, **u32 satellites in use `+0x50`**, **u32 time to first fix `+0x54`** (s from the enable, set at the first fix, kept while the fix holds). **GSV**: SNRs as bytes from `+0`, count `0x380542C8` (≤ 0x31). `$PAIRM…` (type 10): f32 at `+0x5C` (meaning unknown). **RMC and ZDA are classified but not parsed**: no speed, course or HDOP anywhere in it | high (code, emulator: lat/lon/quality/sats/ttff matched the fed position; altitude only seen at 0) |
| Property `0x62` | getter `0x100E37D0` writes the pointer `0x3800CD80`, not the info block; `AT GETGPSSNR` (`0x100EA1D4`) prints `sats, lon, lat, alt, (snr0)…(snr5)` from the block it gets (`GETGPSSNR:%d,%.3f,%.3f,%.1f,…`). The loader reads `0x38016C40` directly | medium (the two addresses were not reconciled; the emulator filled `0x38016C40`) |
| Rate | one GGA per second in the emulator (the model's cadence; `$PAIR064,200` at mode 1 is assumed to set the receiver's own rate, Airoha docs not checked) | medium |
| Consumer path | each GGA with a fix: sensor callback `0x10101BEC` (copies lat/lon, posts sensor `0x18`), `gps_sensor_handler` `0x10197468` → sport `input_gps_data` (`0x1004C6F8`, the workout's track and distance) | high (emulator UART) |
| Stock track store | while the GPS has a fix, **with or without a workout**, `record_gps_data` stores `{time, lon, lat}` every ~5 s (`gps_data_write` `0x100C4BCC`: 12 B records in a RAM buffer, flushed to flash when full, `erase_gps_data_storage_newone` at boot); `sport_gps_data_sync` sends stored points to the phone. So an app's GPS session leaves points in the stock store; whether Nothing X shows or uploads points outside a workout's time window is **not known** | high (emulator) for the writes, unknown for the sync |
| Boot | at boot gps_service enables briefly to read the firmware version (`$PAIR025`), then disables (`not get gps version` in the emulator) | high (emulator) |
| Power | the AG3352 is powered only between enable and disable; a disable powers it down even when another consumer still wants it (no count) | high (code); current draw not measured |

Emulator proof (`scripts/recon/gps_probe.py modes`, stub NOR): mode 1, 4, 5 each log `_gps_service_proc enable[n]`,
`ag3352_enable_poweron`, `gps_enable ok`, `$xxGGA: position time ok: 1` and fill the block (fix 1, 8 sats, ttff 1 s,
the fed position); 0 logs `disable[0]`, `gps_disable ok`, `ag3352_disable gps_work power pin false` and zeroes the
block. `workout`: opening the sport list turned the GPS on (mode 1), the GPS sport's GO screen set 1 again, the button
back to the face set 0. The run screen itself was not reached (the panel had gone dark in the probe), so the stock
workout's own start, pause and end path were read from code only.

Not proven: real fixes, time to first fix and the fix rate on hardware; what modes 4 and 5 change; the battery
cost; whether the stock track store's points reach Nothing X outside a workout; whether `$PAIR600` aiding with a
stale location slows a first fix.

## Widget cards (face swipe left)

Static (capstone on the stock `app.bin` with the stub, 2026-11-14); what the emulator proved is under "Emulator
results" below.
The pages left of the face are the clock plus up to 7 cards. A card is either a grid of system widgets or a whole
stock view. The phone sets the list over BLE; the firmware keeps it as one property.

| Item | Finding | Confidence |
|---|---|---|
| Page list | launcher (`launcher_app.c`) at `0x1005D8BE`: double-buffered list (`0x380014D4` / `0x38001500`), entry 0 = `0x10` (clock), then `CardList(out + 1, 7)` `0x1009B7EC` (logs `CardList cnt:%u`) | high (static) |
| Card config | ux2sys property **`0x2F`**, 98 B (`0x62`), read into RAM **`0x38045CA0`** (`0x100E096C(0x2F, buf, 0x62)`): 8 entries × 12 B, byte `0x60` = `cycle_scrolling_sw` (pages wrap), byte `0x61` not traced | medium |
| Entry, byte 0 = type | **1** = widget card: `+2` u8 widget count (≤ 4), `+3` four `{u8 widget_id, u8 param_id}` pairs; shown by the next free slot view `0xD0 + n`. **2** = view card: `+2` u16 `view_id`, the page is that stock view itself. Type 1 with count 0 is skipped | medium |
| Slot views | `card_widget_view`, `card_widget2_view` … `card_widget8_view`, ids **`0xD0`…`0xD7`** in the stock view table (`0x10000D88` … `0x10000CE0`). Each proc is a 0x1C-byte wrapper calling the style-desc proc `0x100D587C` with its own desc (`0x101EFF64` for `0xD0` … `0x101EFF34` for `0xD7`, stride 0x30) | high |
| Slot desc | `{u8 0, u8 2, u16 view_id, u32 0x94 data size, u32 scene 0xE6EB202D, +0xC layout 0x1009B9E9, +0x10 0x101B1AD3, +0x14 0x101B1A09, +0x18 0x1009B8F9, +0x1C… defaults 0x101B1A05/7B/3D/73}`; identical for all 8 apart from the id | high |
| Slot layout | `0x1009B9E9` (`_card_widget_view_layout`): config of the n-th widget card `0x1009B7B0(view_id − 0xD0)` (logs `card widget_info == NULL` without one); per widget: `wgt_reg` lookup `0x1009B79C(id)` (`wgt_reg not found, wgt_id:%u`), a container `lv_obj_create(screen)` sized by class, radius by class, bg opa 255, `user_data` = `param_id`, a `SHORT_CLICKED` callback `0x101B1B15` with the entry's tap action, position; the state (`lv_mem_alloc` of the entry's size, zeroed; `wge out of memory, id:%u, size:%u`); then `create(ctx)` and `update(ctx)`. Finally a 1000 ms `lv_timer` (`0x101B1B07`, kept at data `+0x90`) | high (static) |
| Grid | sizes by class (`0x101DF780`): 0 = 166×166 circle (radius `0x7FFF`), 1 = 166×166, 2 = 166×344, 3 = 344×166, 4 = 344×344 (radius 38 for 1–4). Origin (61, 61), gap 12, area to 344: a 2×2 grid of 166 px cells | high (static) |
| View data (`0x94` B) | `+0` screen, `+4` the card's config copy (count first), `+0x10` four 0x1C-byte widget contexts, `+0x80` four `wgt_reg` pointers, `+0x90` the timer | high (static) |
| Widget context (`ctx`) | `+0` a copy of the presenter's first 16 bytes, `+0x10` the container, `+0x14` the state (NULL when the size is 0), `+0x18` u16 widget id | high (static) |
| Callbacks | create `+0x10 (ctx)` and update `+0x14 (ctx)` once at layout; update every second from the timer while the slot view is on top (`0x1018C020(view_id)`); focus `+0x18 (ctx, arg)` from desc `+0x14`; reload `+0x1C (ctx, arg)` from desc `+0x24`; delete `+0xC (ctx)` from desc `+0x20` (unload), which then frees the state and deletes the screen. All but create and the first update are skipped while the state is NULL. Return values are ignored (stock no-ops return 0) | high (static) |
| Tap | `0x101B1B14`: the action `0` does nothing, `≤ 0xF4` is `OPEN_VIEW(action, 0)` (`0x6B` first sets the sport type from the container's `user_data`), above that a function tail-called with r0 = the container (current target), not the event | high (emulator) |
| Widget registry `wgt_reg` | flash table **`0x101EFC88`**: 99 pointers, ids `0x00`…`0x62` (lookup returns NULL above `0x62`). **Id `0x40` is NULL** (free). Entry: `{u8 id, u8 size class (0, 1, 3; the first style of a family is 3), u16 state size, u32 resource scene, u32 tap action (a view id, or a function that opens an app), 5 callbacks}`; callbacks `+0xC` delete, `+0x10` create, `+0x14` update, `+0x18` focus, `+0x1C` reload (row "Callbacks") | high (static) |
| BLE command | **`0x906D`**: case 27 of the `0x9052…0x908B` jump table at `0x100D351C`, handler **`0x100D0594`** (tag `view_card_setting`). Get and set; set clamps to 8 cards and 4 widgets per card, copies the 98 B to the settings store `0x38007880` and calls `0x100E05EC(0xE4, 0)` (save/refresh, assumed). The same frame carries `cycle_scrolling_sw` (pages wrap). Plaintext (session key, as every stock command): request `00` = get, `01 <wrap> <count ≤ 8> <cards>` = set, a card being `02 <u16 LE view_id>` or `01 <n ≤ 4> n × <widget_id, param_id>`; set replaces all 98 B (zeroed first). Reply **`0xA06D`**: `01 <wrap> <count> <cards>` to a get, `00` to a set. `src/wp2apps/cards.py` | high (static) |

Widget families by tap action (ids are `wgt_reg` indices):

| Ids | Opens | Family |
|---|---|---|
| 1–6 | `0x7E` `activity_record_steps_view` | steps |
| 7–12 | `0x7B` `activity_record_calories_view` | calories |
| 13–18 | `0x7D` `activity_record_stands_view` | stand |
| 19–24 | `0x7C` `activity_record_strenght_view` | exercise / strength |
| 25–30 | `0xB2` `app_heart_view` (function `0x1006F1E5`) | heart rate (`wgtN hr curr`) |
| 31–36 | `0x73` `bloodoxygen` | SpO2 |
| 37–42 | `0x9B` `pressure_detecting_view` (`0x101B30C9`) | stress (`wgt_stress_style*`) |
| 43–48 | `0xE9` `sleep_data_view` (`0x10074739`) | sleep |
| 49–53, 97, 98 | `0x5A` `weather_main_view` (`0x1007F699`) | weather |
| 54–58 | `0x63` `timer_running_view` | timer |
| 59–62 | `0x8C` `breath_view` | breathing |
| 63, 65–67 | `0x1C` `stopwatch_view` | stopwatch |
| 68–71 | none | not identified |
| 72–76 / 77–81 / 0, 82–86 | `0x95` / `0x94` / `0xA8` | call log / contacts / dialer |
| 87–91 | `0x6B` `sport_prepartion_view` | sport (`widget_sport sport_type`) |
| 92 / 93 / 94 / 95–96 | `0x90` music / `0x79` `appactivity_record_view` / vitality (`0x100872F9`) / `0xDA` `female_main_view` | music / activity / vitality / cycle |

The stub's widget (`fw/wp2-stub/src/card.rs`): the stub header's last word (reserved, 0 before) points at a
`wgt_reg` entry for id `0x40`, and `wp2apps firmware` writes it into the free slot `0x101EFD88` (the stock word is
checked to be 0). "Apps": class 3 (344×166), tap action `0xF0` (the loader menu, opened by the stock tap handler),
the Custom Apps icon and "Apps" in `font32M` on the stock card grey (RGB565 `0x2966`; the emulator frame reads it as
`#1A1C1F`, white as `#A0A0A0`); the state holds the font and the icon block, delete
frees them. The callbacks live in the stub because the loader is in memory only while view `0xF0` is open. The
host puts the widget on a card with `wp2apps cards add-apps` (or `cards set widgets:0x40`). Nothing X does not
know id `0x40`; how it shows or rewrites such a list is untested.

### Emulator results (2026-11-15, stub version 4)

Proven with gdb breakpoints on the stub's callbacks (probe scripts and logs in `runs/app-widgets/`, not committed) and
`tests/test_app_widgets_e2e.py` / `tests/test_cards_e2e.py`:

| Item | Finding | Confidence |
|---|---|---|
| `param_id` → `user_data` | card `widgets:0x40/3,0x40/0`: create gets `ctx.obj` with `*(u32 *)(obj + 0x10)` = `3` and `0` (gdb `CREATE obj=0x3806a594 user_data=0x3`, `CREATE obj=0x3806a85c user_data=0`). Set by the layout (`str r3, [r7, #0x10]` at `0x1009BB70`) before create | high (emulator) |
| Tap action > `0xF4` | the stock handler tail-calls it (`bx r3` at `0x101B1B48`) with **r0 = the container** (`lv_event_get_current_target`), not the event: gdb `TAP obj=0x3806a594 user_data=0x3`, then UART `view_stack: jump 208 -> 240` from the stub's `OPEN_VIEW(0xF0)` | high (emulator) |
| Card lifecycle | the card view is laid out (delete + create of every widget) when the face shows, not per swipe: face → card → face → card ran create once; a tap into view `0xF0` deleted the widgets, and crown back to the face created them again. So a record read in create is fresh after every app run | high (emulator) |
| Tile size, grid | two wide (class 3) widgets per card at (61, 61) and (61, 239), 344×166 each | high (emulator frames) |
| Mixed cards | the stock lays a card out in two rows in list order: a wide (class 3) widget takes a row, class 1 and class 0 share one (left, right). The app widget mixes with stock ones in either order: `0x40/1,0x5d` (app on top of a stock wide), `0x5d,0x40/1`, `0x40/1,0x2e,0x23` (app, then class 1 + 0 below, as the stock `0x19,0x2e,0x23`), `0x2e,0x23,0x40/1`, `0x40/1,0x04` (class 1 alone bottom left). More than two rows (`0x40/1,0x40/2,0x51,0x53`): the third row is not shown, no fault. A tap on the app tile of a mixed card starts the app. Frames `runs/app-widgets/mixed/` (not committed) | high (emulator) |
| Tile colour | the stock tile is RGB565 `0x2966` (the frame reads 26, 28, 31, as the stock Settings row). The stub first used `0x18E3` (`#1A1C1F` from the frame taken literally), which read (15, 18, 15), visibly darker next to a stock widget | high (emulator frames) |

App widgets (`fw/wp2-stub/src/card.rs`): `param_id` 0 is the "Apps" tile (opens the menu). `N` = 1…8 is the app in
slot `N − 1`: create reads its 64-byte record `/NAND:/wp2apps/<slot>.wgt` (title at 0, value at 24, NUL-terminated)
and draws a grey title and a white value in `font32M`; without a record it shows the package name (header `+8`),
without a package "Apps". Update reads nothing. The tap function stores `N` in `StubRam.launch` (`+0xB`, was
padding) and opens view `0xF0`; the loader takes it at LAYOUT and launches that slot as a row tap does (an
uninstalled slot leaves the menu; an older loader ignores the byte and shows the menu). A refused loader load
clears it. The app writes the record with an `lv_submit` WIDGET record (op `0x14`, alone in a batch, payload 64 B,
`Api::widget`); the loader writes the file on the app thread. App removal (BLE `DELETE`, the menu's long press,
another app installed into the slot) unlinks it. Stub size: `0x2B5F` → `0x2DB3` bytes; with the view table `0x3D00` of `0x4000`.

### App pages and the Apps menu (2026-11-15, stub version 5)

| Item | Finding | Confidence |
|---|---|---|
| View card with our view | `cards set view:0xF0` makes the loader view a page left of the face: the pager lays it out with the face (PRELOAD + LAYOUT before any swipe), swipes in and out give `focus`/`defocus` + `resume`/`pause draw`, no fault. Frames `runs/app-widgets/view-*.png` (not committed) | high (emulator) |
| Free view ids | the stock view table ends at id 237; `0xF1`…`0xFF` are unused. `wp2apps firmware` adds `0xF8`…`0xFF` (the same proc as `0xF0`, 8 × 24 B) | high (static) |
| App page | view `0xF8 + slot`: the loader starts that app at LAYOUT (`cards.rs page_slot`) when its package declares a page. DEFOCUS pauses it (`APP_PAUSED`), FOCUS resumes or restarts it. A widget tap (`jump 208 -> 240`) PRELOADs `0xF0` while the page still exists: the stub hands the loader to the new view (`StubRam.loader_view`) and ignores the old view's later DELETE. The next face layout creates the page again and it starts the app again | high (emulator, `tests/test_app_pages_e2e.py`) |
| Install under a page (stub 6) | the loader and its app run from RES heap copies (`load.rs`), so the packages may change under them. The stub tracks the loader view's focus (`StubRam.loader_focus`, msg 7/8); with the page out of view `BEGIN` for `0xFF` or the page's slot is taken and `END` sets `StubRam.reload`. At the page's next FOCUS the stub runs a PRELOAD on the same view: the old instance gets DELETE, the new one's preload calls `ui_view_layout`, the view manager sends a fresh LAYOUT and the new loader starts the new app. No fault, the stock keeps the page | high (emulator, `tests/test_page_install_e2e.py`) |
| Card list store | the 0x906D set copies its 98 B to `0x38007880` and calls `0x100E05EC(0xE4, 0)`. The loader does the same from the Apps menu: the store changes at once, the pager uses it from its next layout (CardList re-reads property 0x2F into `0x38045CA0` then) | high (emulator) |
| Declarations | `app_features!` puts `"WP2F", flags, 0, 0, 0` into `.wp2info` after the icon (offset 0 without one); the menu's scan reads it after the icon. Not in the package header: its reserved bytes must be 0 for every stub so far | high |

Still open: two app pages, or `view:0xF0` and an app page on one list, each PRELOAD takes the loader from the other
and leaves the other page blank (the menu allows one page; `cards set` does not check); what the stock does with the
crown press on a page (the loader leaves it to the stock); an app page's memory while the face shows (its block stays
allocated); a `cards set` while the card is on screen; the package icon on the tile (the stub has no room for an
icon reader; the tile is text only); `param_id` of the stock families; byte `0x61`.

Not proven in the first pass (no emulator on that machine) and settled by the emulator results above: the widget
drawing and tapping. Still unmeasured: what the 1 s update costs (the stub's update returns at once).

## Stock watch faces over BLE: set active, delete (0xFFFF9055)

For FMC Companion (tap = make active, context menu = delete). Ghidra (`scripts/recon/decomp`) on the BLE dispatcher
`FUN_100D343C`, its `0x9055` handler `FUN_100D3138` and `WFManager.c` (`0x100DC500..0x100DE900`); checked on the
emulator (stock paired NOR over netsim, 2026-11-18): get, set active, drop an id.

One command does both: **`0xFFFF9055`**, the phone side of `0xA055`. Session-encrypted like any stock command (sent
on the command characteristic in the emulator run). Payload, ids **LE**:

| op | payload | effect | reply |
|---|---|---|---|
| get | `00` | `sync_watch_face_info` (`FUN_100D2D5C`) | `0xA055` list, as after auth |
| set | `01 ‖ idx ‖ cnt ‖ 00 ‖ cnt × u32 id` | `WFManager_SetWatchFaceListForBle(idx, cnt, ids)` `0x100DE6FC` → pending opt 2 → `_SetList` in `FUN_100DE10C` | `0xA055` with the single byte `00` |

The `0xA055` list is `01 ‖ idx ‖ cnt ‖ 07 ‖ cnt × u32 id`: byte 1 is the **active index** (`WFM GetList idx:%u cnt:%u`,
`FUN_100DE67C`: `*(u8*)0x3801AE55`), byte 2 the count (at most 10), byte 3 the constant 7 (`FUN_101B91C8`). Every
entry of the watch's list is sent; records are 16 B from `0x3801AE5C`.

`_SetList` walks the sent ids in order and keeps those already in the watch's list, in the new order; the active face
becomes the sent entry at `idx` (index into the **sent** list; 0 if that id is not on the watch). It then saves the
list (`FUN_100DD16C`) and reloads the face. Consequences:

- **Set active**: send the current list unchanged with a new `idx`. Emulator: `idx 0 → 1` gave
  `WFM _SetList … Opt ret:0`, then `WFM Load Idx:1, Id:274` at once, and the next get returned header `01 01 06 07`.
- **Delete**: send the list without the id (and `idx` of the face that should stay active). Emulator: dropping 280
  gave `cnt 5` in the next get. The list entry goes at once; a downloaded face's `.wfc` stays on NAND until
  `WFManager_DelRubbish` (`FUN_100DCFEC`, deletes `/NAND:/*.wfc` whose id is not listed) runs at boot (`0x100E0F2E`)
  or at the next upload init (`0x9063` `0x100EBB4A`, `0x9075` `0x100EBEAC`). Built-ins (`273 274 275 276 277 280`,
  table `0x101F3D54`) are never deleted from NAND, only unlisted.
- Ids not on the watch are ignored: the command cannot add or bring back a face. The reply is `00` even when nothing
  matched (all unknown ids leave the list as it was), so read the list again to confirm.
- `0xFFFFFFFF` in the list is the photo ("album") face, not a separator: `FUN_100DC534` returns kind 1 for `-1`,
  0 for the six built-ins, 2 for downloads. Keep it in a set list like any other id.

Other paths:

- Gadgetbridge's `WATCHFACE` `0x009F/0x0001` does **not** change the face: the dispatcher (`0x100D52AE`) only logs
  `set wf id:%d` with `payload[0] - 1` and answers an empty `0x009F/0x0003`. `0x009F/0x0002` sends the `0xA055` list
  (`FUN_100D2D5C`), like `0x9055 00`.
- Delete through the upload flow (`0x9075` with `new_wf_id = 0`, cmf-watch-utils `protocol/ble.md` §4.1, verified on
  hardware) still needs the data phase; `0x9055` needs none.

| Claim | Confidence |
|---|---|
| `0x9055` layout, set active, unlist | high (static + emulator, built-in faces) |
| `0x9055 00` on hardware | high: 2026-11-18 a real watch answered `01 03 06 07` + six downloaded ids (all > 2³⁰): downloaded faces **are** listed, no `0xFFFFFFFF` when no photo face is installed |
| Set active on hardware | high: same list with `idx 3 → 4` and back, reply `00`, header followed, the face on screen changed (owner) |
| Downloaded face: unlisted, then its file deleted at boot / next upload | medium (static only) |
| `0xFFFFFFFF` = photo face | medium (static) |
| `0x009F/0x0001` is log-only | high (static) |

Not yet tried on hardware: unlist (delete); a set while the watch is in face edit mode (`WFM in EditMode` suggests the
handler waits for it); dropping the active face (the code takes the entry at `idx`, so pick another `idx`).

## Watch face boot guard (stub 9)

A face that faults the stock while it loads at boot made the watch loop: fault, panic, reboot, the same face again
(the DataPulse port of cmf-watch-utils `runs/bootloop-repro/`: 6 boots and 5 faults in 60 s, HARD FAULT on
ui_service ~0.15 s after `WFManager_Parser enter`; sometimes a hung parser and the 11 s watchdog instead). The
logged pc `0x100EEB46` is not the cause: it is the assert in `0x10199CC8` (a k_mutex taken in an ISR) that the fatal
handler's own storage flush (`0x100C6234`) hits, so the first fault is masked.

| Item | Finding | Confidence |
|---|---|---|
| Boot path | `WFManager_Init` (`0x100DE020`, once; callers `0x100676FE`, `0x1006E3CC`, `0x100B1924`, `0x100B2352`, all UI views on ui_service) → `WfmPara_Load(0x3801AE50)` (`0x100DEB34`, its only call `0x100DE068`) → `WfmCheck_CheckFast` per id (`0x101B92E2`) → active entry → `WFManager_Load_ByIdx(-1)` → `WFManager_Parser` | high (static + UART) |
| WfmPara | 0x80 B: magic `0x3DAFB58D`, u8 count +4, u8 active index +5, u8 att +6, u8 kind +7 (0 built-in, 1 photo, 2 download), u32 active id +8, 16-B entries from +0xC (u32 id, u8 att from the wfc header +0x17), checksum +0x7C. `WfmPara_Load` copies it from the RAM mirror `0x3800BC9D`, `WfmPara_Save` (`0x100DEBC4`) back (asserts on count 0) | high (static + emulator) |
| Persistence | the mirror is inside data_storage type 1 (`0x3800ACD0`, 0x12F8 B, NOR ring at 0x488000, table `0x101F2C78`). `0x101B5A6E(type, -1)` writes a record. Type 1 is written only by `0x100C6234` (power-off `0x10148C60`, menu reboot `0x10148CF4`, `0x101CE678`, the fatal handler) and the factory reset. `AT REBOOT` and a watchdog reset go straight to `0x100EEFF4` and lose a list change: in the emulator a `0x9055` unlist was gone after `AT REBOOT` | high (static + emulator) |

The stub replaces `bl WfmPara_Load` at `0x100DE068` (stub header version 2, field 9, `fw/wp2-stub/src/wfguard.rs`;
the hook itself is `para_load` in wp2-core `wfguard.rs`, shared with the Watch 3 Pro stub below):

- no flag file `/NAND:/wp2wfgrd`: create it (empty);
- the flag is there (the last boot never confirmed its face) and the list loaded: `fall_back` (wp2-core `wfguard.rs`)
  makes the first listed built-in other than the active face active; with no other built-in listed the active
  entry becomes `0x111` (`0x112` if it was `0x111`). Then `WfmPara_Save` and `0x101B5A6E(1, -1)`, so the change
  survives any reset;
- either way an `lv_timer` (ui_service) unlinks the flag 20 s later.

Cost: one file create and one unlink per boot, one NOR record (type 1) only when it falls back. A good face loads as
before. The bad face stays listed and on NAND: the phone deletes it (`wf-delete`) or unlists it (`0x9055`).

Emulator (preview.10 QEMU, `tests/test_wfguard_e2e.py`, DataPulse over `local273.wfc`, the default face):

| Run | Boots | Faults | Result |
|---|---|---|---|
| stock app.bin, `--watch-face` (cmf-watch-utils `runs/bg-base/`, 30 s) | 4 | 3 | loop |
| stub 9, same NAND (cmf-watch-utils `runs/bg-guard-1/`, 60 s) | 2 | 1 | boot 2 makes 274 active, stable |
| e2e `runs/e2e/wfguard-154319/` | 3 | 1 | boot 2 on 274, flag gone after 20 s; `0x9055` unlists 273; type-1 flush + `AT REBOOT`; boot 3 on 274 with 5 faces, no fallback; frames `1-fallback.png`, `2-after-reboot.png` |

An earlier e2e run without the type-1 write (`runs/e2e/wfguard-153350/`) showed why it is needed: after `AT REBOOT`
the list was back at 273, the flag was already cleared, and the watch faulted again.

Not covered: the confirm timer with the screen off (this QEMU stops at S3BT ~15 s after the screen goes dark, so the
e2e keeps it awake; if LVGL timers stop while dark, the flag only goes at the next wake, and a reset before that
falls back once without need); a face that faults later than 20 s after the load (first minute tick, a later
redraw); a bad face made active at run time (`0x9055` set, an upload) faults once there and once more at the next
boot before the guard takes over; a download (only a replaced built-in was tested); hardware.

### Watch 3 Pro (jx402_08 1.0.1.77)

Re-derived from the stock app.bin (sha256 `815a21fc…`, as cmf-watch-utils `wp3-stock`) by matching the WP2 functions
and their log strings; the code is the same shape with other addresses. `fw/wp3-stub` (744 B, header
`WP3S`, version 1, one hook) holds only the guard, appended at the end of app.bin (`0x102BD280`); `wp2apps firmware
--model wp3 --stub target/wp3-stub.bin` patches the call and fixes ACTH. Addresses are in wp2-core `wfguard.rs` (`WP3`).

| Item | WP2 1.0.0.73 | Watch 3 Pro 1.0.1.77 |
|---|---|---|
| `WFManager_Init`, its `bl WfmPara_Load` | `0x100DE020`, `0x100DE068` | `0x100D6DAC`, `0x100D6DF4` |
| `WfmPara_Load` / `WfmPara_Save` / face att | `0x100DEB34` / `0x100DEBC4` / `0x100DC6B0` | `0x100D7990` / `0x100D7A24` / `0x100D4C70` |
| WfmPara | 0x80 B at `0x3801AE50`, 7 entries | 0xF0 B at `0x38026CAC` (checksum +0xEC), 14 entries; mirror `0x3800F73A` |
| data_storage type 1 | `0x3800ACD0`, 0x12F8 B; write `0x101B5A6E` | `0x3800E1A0`, 0x2430 B; write `0x101D0ECC` (same veneer to `0x100B6B40`) |
| built-ins | `0x101F3D54`: 273–277, 280 | `0x10220810`: 274, 276, 275, 821–824 (ids −1…−3: photo and other kinds) |
| `lv_timer_create` / `_del` | `0x1013B10C` / `0x1013B1A4` | `0x10148E6C` / `0x10148F04` |
| `fs_open` / `fs_close` / `fs_unlink` | `0x10107030` / `0x101071E0` / `0x1010764C` | `0x10115F18` / `0x101160C8` / `0x10116638` |

Unlike WP2, the 1.0.1.77 fatal handler's type-1 flush works (the list survives the fault, `cfgOk:1` at the next
boot). The DataPulse port does not fault 1.0.1.77 (it renders as 274), and no other faulting face is known, so the
e2e (`tests/test_wfguard_e2e.py`, `Wp3WatchFaceGuardTest`) models one with gdb (`tests/e2e/wp3_badface.gdb`): 276
made active in the default list, a fault once `WFManager_Init` picked it. 276 rather than 274 because this emulator's
ROM lacks a call 276 draws with (`unsupported API 0x524f4d`, from `0x1014F822`), so 274 has to be the fallback.
The runs use cmf-watch-utils' `watch_face_preview` on the patched app (skips the setup screens; boot 1 always
reboots out of setup).

| Run | Boots | Faults | Result |
|---|---|---|---|
| stock app.bin + the model (`runs/wp3/stock-1/`, 60 s) | 6 | 5 | loop |
| `wp3-stub` (`runs/wp3/guard-3/`, 90 s) | 3 | 1 | boot 3 makes 274 active, stable to the end |
| e2e `runs/e2e/wfguard-wp3-224800/` | 4 | 1 | boot 3 on 274, flag unlinked by the timer, QEMU reset, boot 4 loads 274 from NOR with no fallback; stable |

Not covered beyond the WP2 list: a face that really faults 1.0.1.77; boots with a reset during the confirm timer
under gdb (a breakpoint left on the timer path made the virtual watchdog fire once, `runs/wp3/guard-6/`);
the 3 Pro emulator profile is experimental (synthetic storage layout).
