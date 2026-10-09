# caddy-archive-core 0.3.1 update — 2026-10-09

ms-package 0.3.0 uses registry caddy-archive-core 0.3.1, checksum
`037a4d82a6a4c3964e456f3b9cdd1610ac6036f99a9cf88aec93a86b86076a4f`.
The integration/source-release archive pin is
`8e0724332641e5f6a1315c4544497016e3152322`. The published core's VCS metadata
records the previous `b336c98eae34b27836270e76947ffce31555f81c` revision with a
dirty tree; its checksum is the registry provenance. The subsequent clean
0.3.1 commit records the reproducible release fixtures and version bump.

The public package API exposes archive-core entry, metadata, limit and error
types. Updating their dependency identity requires a minor version boundary:
existing archive 0.3.1 consumers expecting package-core 0.2.1 stay on ms-package
0.2.x. Function names and package authoring profiles are unchanged.

Locked all-target/all-feature Rust tests, default and writer doctests, strict
Clippy, rustfmt and WASM builds passed. Isolated archive workspace compilation
passed after reconciling its ms-package and package-core declarations; actual
sibling checkout files were not changed.

The [baseline Worker](evidence/archive-core-031-20261009/baseline-worker.json)
passed all 18 archive profiles, package readers, six MSI media layouts, encrypted
archives, cancellation and editing checks. The
[authoring Worker](evidence/archive-core-031-20261009/authoring-worker.json)
passed all nine create/edit/limit and native parity checks. The browser pipeline
now verifies and copies the archive release's 15 MSI media regression artifacts,
which its new Worker suite requires in addition to generated package fixtures.

Separate debugger ports were used for the final local browser runs after an
initial parallel run shared the Chromium debugger endpoint. Only the distinct
final baseline/authoring receipts are qualification evidence. Historical media
and Windows lifecycle evidence remains intact; no installer code changed.
