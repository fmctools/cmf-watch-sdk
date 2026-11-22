# Read-only storage inspection

`cmf_wp2_app::storage::Storage` reports writable capacity, partition reservations, installed apps and a
bounded file listing. It is available to any app, including canvas apps; it does not require a Storage app or
create an LVGL root. The backend currently targets JX402 firmware 1.0.0.73. It does not establish Watch 3 Pro
support.

This is an additive loader service. The `Api` table, ABI major/minor, `.wp2app` format, existing LVGL records,
slot paths, menu ordering, widget/PUSH behavior and stock OTA sequence are unchanged. Existing binaries do not
need rebuilding. Own-app persistence remains on `Api::load_data` and `save_data`; this service does not replace it.
No firmware update is required for this service; stub 8 is required only when using slots
8–15.

## Use from an app

Open with `Storage::open(api)`. A loader without service v1 returns `storage::UNSUPPORTED` (-95), including
when its command decoder answers -22. Show an update message and keep normal navigation available. Probe
support at runtime; the ABI minor alone does not prove a loader has this service.

- `capabilities()` returns the service version and limits, including the active stub's 8 or 16 app slots.
- `filesystem()` refreshes measured FAT data-cluster capacity and free bytes, the allocation unit and logical
  sizes of SDK package/data/widget/loader files. These file sizes are subsets of used space, not additional
  allocations. It does not treat firmware reservations as writable space.
- `partition_count()` and `partition(index)` read the checked stock partition table. Offsets are relative to
  each record's device; partitions on different devices cannot be summed as if they shared an address space.
- `installed_app(slot)` returns `Some(InstalledApp)` for a valid package, `None` for an empty/invalid package
  and -22 for a slot unavailable on the active stub. A slot is not its sorted menu-row index.
- `begin_scan()`, `scan_step()`, `file(index)` and `cancel_scan()` provide the running app's single file
  snapshot. Start again to invalidate and replace it. Process a step between events, not in a busy loop. Read
  collected files before cancelling. `ScanProgress.complete` ends the walk; `partial` means the result is
  incomplete even if `error` is zero.

Paths and names are returned as bytes, with convenience UTF-8 accessors. No file contents, deletion,
installation, raw flash access or arbitrary filesystem calls are exposed. Metadata can still contain private
filenames; apps must not send it to a phone or server without the user's request.

The scan covers `/NAND:` only. It visits at most 96 files, 16 directories (including the root), and at most 95 path bytes plus a NUL terminator. Long paths and exhausted limits are skipped and set `partial`; a filesystem
error also records its negative status. There is no silent truncation. Capacity and files can change while
stock firmware writes history; this is not a transactionally frozen filesystem snapshot. Logical lengths are
not per-file physical cluster-chain usage.

## Installed-app icons

`Batch::installed_app_icon(object, slot, IconSize::Native)` displays the validated 96px menu icon.
`IconSize::Thumbnail` displays a 64px cached copy. The object must be an ordinary `Class::Obj`; make its
background transparent to preserve alpha. No pointers or pixel buffers cross the API.

Missing or unloadable icons clear the image background. An app can draw a fallback using its metadata;
`InstalledApp.has_icon` describes the package descriptor and does not guarantee the menu cache could allocate
its pixels. Native mode reuses menu pixels; thumbnails are allocated lazily, at most one per menu entry, and
freed after app LVGL teardown on exit or when the menu entry changes. Native menu icons remain available.

## Lifecycle and budgets

Requests use the loader's existing UI mailbox. Filesystem operations and app-stop cleanup are serialized on
`ui_service`, while the calling app waits. A force stop cannot abort a directory operation midway. One scan
step reads at most one directory entry. Capacity and catalog queries are synchronous; call on launch/refresh,
not every frame.

The scan's 11,136-byte file/directory cache is allocated only when a scan starts. Allocation failure returns
-12; cancellation or app stop closes the directory, frees the cache and invalidates indices. Pause retains it
for resume. Normal exit, the existing forced escape and loader-view teardown use the same app-stop path. There
is one cursor per running app, shared by its `Storage` handles; handles are not independent filesystem
sessions. SDK request buffers are serialized and reentrant calls return -16.

## Wire format

Integers are little-endian. Requests are standalone `lv_submit` records inside the calling app block, not
mixed with LVGL commands. The loader range-checks the entire buffer before queuing it.

Storage record: `[0x19, 128, 0, 0]` followed by a 128-byte payload. Payload byte 0 is the operation, bytes 1–2
are the index (`u16`), byte 3 is zero, and bytes 4–127 are response space. The loader clears response space.
The function returns 0 on success or a negative error; response bytes are unspecified on error. Unsigned fields
keep all 32 bits.

Offsets below are relative to the response space (record byte 8). Unused bytes are zero. Index must be zero
unless stated otherwise.

| Operation | Index | Response |
|---|---|---|
| 0: capabilities | 0 | version `u16` @0; active slots `u8` @2; max partitions `u8` @3; max files `u16` @4; max directories `u16` @6; path bytes `u16` @8; supported icon sizes mask `u8` @10 (bit 0:64px, bit 1:96px) |
| 1: filesystem | 0 | eight `u32` @0,4,…,28: total, free, package bytes, data-blob bytes, widget bytes, loader bytes, installed package count, allocation unit |
| 2: partition count | 0 | count `u8` @0 |
| 3: partition | partition index | name `[u8;8]` @0; kind, file ID, device, mirror `u8` @8,9,10,11; offset `u32` @12; reservation `u32` @16 |
| 4: installed app | app slot | valid `u8` @0; icon-descriptor flag `u8` @1; slot `u8` @2; name `[u8;24]` @4; version `u32` @28 |
| 5: begin scan | 0 | none; resets the previous snapshot |
| 6: step scan | 0 | collected count `u16` @0; complete, partial `u8` @2,3; filesystem error `i32` @4 |
| 7: cancel scan | 0 | none; invalidates collected file indices |
| 8: file | collected file index | logical size `u32` @0; NUL-terminated path `[u8;96]` @4 |

Filesystem package/data/widget counters cover `.pkg`, `.dat` and `.wgt`; `.psh` and temporary files remain
part of overall used space and are visible to the file scan. They are not hidden free space. App-specific
categorization should be based on the listed paths, not inferred from those three counters alone.

Icon record: `[0x1A, 2, handle_lo, handle_hi, slot, side]`, side 64 or 96. Normal whole-batch validation
checks class, handle and slot range 0–15 before any mutation. A slot absent from an eight-slot stub has no
image.

Malformed/unknown commands and indices return -22; unsupported service/backend -95; request contention -16;
allocation failure -12; invalid/corrupt stock metadata -5; arithmetic overflow -75. Stock filesystem errors
are propagated. Limit exhaustion is reported through `partial`, not fabricated totals.

Future incompatible layouts need a different record or a new service version, not a reinterpretation of v1
fields or existing commands.
