# MSI media refactoring qualification — 2026-10-09

Versions: ms-package 0.2.2, caddy-msi 0.10.2, ms-cabinet 0.1.4,
caddy-archive-core 0.2.1. Existing public package APIs and supported installer
profiles remain compatible. Reusable media planning, interpretation, extraction
and emission are now backend-owned; the package wrapper keeps its authoring
profile, identity, path and publication policies.

## Qualification

- All-target/all-feature locked Rust tests, default and writer doctests, strict
  Clippy and rustfmt passed in both repositories; fork FFI tests also passed.
- Database-only backend, reader-only package and WASM media builds passed.
- [All 63 generated artifacts](evidence/msi-media-refactor-20261009/matrix-equivalence.json)
  matched the previous implementation byte-for-byte.
- [Authoring Worker](evidence/msi-media-refactor-20261009/authoring-worker.json)
  and [baseline Worker](evidence/msi-media-refactor-20261009/baseline-worker.json) passed.
- [All 18 Windows lifecycles and 28 Rust executables](evidence/msi-media-refactor-20261009/windows-lifecycle/README.md)
  passed in a separate guest directory. Historical qualification files remain intact.
- Fuzz smoke and strict fuzz Clippy passed. [Three instrumented targets](evidence/msi-media-refactor-20261009/fuzz-summary.json)
  each completed 129 iterations without crashes or timeouts.

Direct regressions cover immutable prepared plans, bounded File/Media
relationships, 255 UTF-16-unit cabinet references, embedded stream limits,
reserved cabinet fields, original resolver errors, cabinet error classification,
short writes, flush failures and caller finalization. Backend names permit
Unicode and nested relative paths; the existing package profile keeps its ASCII
restrictions. Encoding and all emitted bytes remain unchanged.

The candidate source digest record covers the files exercised by the artifact,
Worker and fuzz gates; subsequent rustfmt changes only adjusted formatting.
Registry-only consumer qualification passed after backend publication: the
[63-artifact comparison](evidence/msi-media-refactor-20261009/registry-matrix-equivalence.json),
[authoring Worker](evidence/msi-media-refactor-20261009/registry-authoring-worker.json),
and [baseline Worker](evidence/msi-media-refactor-20261009/registry-baseline-worker.json)
all passed. [Registry provenance](evidence/msi-media-refactor-20261009/registry-provenance.json)
records the published dependency checksums and current source digests. The default
backend build does not pull cabinet codecs. No spanning, signature trust, new
compression profile, general installability or upgrade claim is introduced.
