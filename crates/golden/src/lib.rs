//! Golden-image tests for Nova Film's renderer (test-only crate; the library is empty).
//!
//! `tests/golden.rs` builds small procedural projects (no media files), renders one frame of each
//! through the CPU reference compositor (`novafilm-render`) and compares it with a committed PNG
//! in `goldens/` using `novafilm_testkit::golden`. The same scenes are composited on the GPU
//! (`novafilm-gpu`) when an adapter is available and compared with the CPU result.
//!
//! Regenerate references with `NOVAFILM_BLESS=1 cargo test -p novafilm-golden`.
