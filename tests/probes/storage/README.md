# Storage lifecycle probe

This is an emulator test fixture, not a store app. It uses the public typed storage API without opening LVGL,
starts a directory scan, then deliberately stops reading events. A back swipe must trigger the loader's existing
forced escape and release the open directory and scan cache. Do not install it on a physical watch as part of tests.

Build from this directory with `cargo build --release`. Package the ELF and binary with the SDK's normal packer.
`STORAGE_PROBE` exposes ready, service version, active slots, capacity, free space, collected files, complete and
partial as eight u32 values for the external debugger. When ready, LV_STATE's root must still be null. After escape,
running_slot must be 255, app_block and the storage scan pointer zero, and the directory closed. A normal app must
still launch afterward. The probe can be used with eight-slot or sixteen-slot firmware; the expected slot count comes from capabilities. Neither fixture changes the PR base.

Build with `--features icons` for the UI variant. It renders slot 0's icon as a 64px thumbnail and natively at 96px, and slot 15 (missing).
Its ready marker is 2. Use the crown button to arm the existing forced-back timeout; do not assume a swipe on an LVGL child reaches the stock back handler. Before escape the 64px descriptor must reference an allocated cache; after app teardown
the descriptor must be zeroed and its thumbnail freed, while native menu icons still work.
