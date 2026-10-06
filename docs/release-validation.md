# Source release validation — 2026-10-07

Rust/Cargo 1.99.0 is the supported minimum and the pinned CI toolchain.

Passed on the prepared sources:

- archive-rs workspace all-feature locked tests and all-target strict Clippy.
- archive-rs locked fuzz regression tests.
- ms-package all-target/all-feature locked tests, including the example bundle/media regressions and MSI metadata preflight regressions.
- ms-package strict all-target Clippy, formatting, doctests and documentation with warnings denied.
- archive-wasm build and actual Chromium Worker checks covering MSI/MSIX/bundles, native payload parity, archive round trips, encryption and Blob cancellation.
- Preliminary source-bundle extraction and offline all-target tests/doctests with fresh Cargo home and target directory.
- Source archive guards reject modified file hashes and path traversal; release guards reject empty, moving or abbreviated dependency revisions.
- Frozen dependency Git blobs were checked against all 9,323 source/evidence files; line-ending conversion is disabled to preserve their bytes.

A package honggfuzz smoke run completed 100,001 iterations in five seconds,
with zero crashes and zero timeouts. Its recorded peak RSS was 437 MiB.
Evidence is retained at `/data/cache/ms-package-release-fuzz-20261007-with-deps`.
This short campaign does not replace sustained coverage-driven qualification.

The tag workflow requires Windows tests, Chromium Worker checks and a fresh
extracted offline artifact build before publishing. Windows-native results have
not been claimed from this Linux validation. Broader real Microsoft-package
comparisons and process-wide allocator bounds remain outside the 0.1 preview
qualification; see release.md.
