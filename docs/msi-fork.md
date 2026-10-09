# MSI reader and writer fork

`ms-package` uses [caddy-msi 0.10.2](https://crates.io/crates/caddy-msi/0.10.2),
from [CaddyGlow/rust-msi](https://github.com/CaddyGlow/rust-msi), pinned to
`=0.10.2` with the optional `media` feature and imported as `msi`.
Cargo resolves the published registry package automatically. Its locked checksum
is `c59023262a23d30e440433d5958c331f4a9765f23405a5b338624be8ab75b853`.

The 0.10.2 media refactor is committed as
[`1d9a8fe80766ed81136ab2b8c3b0574f59784d83`](https://github.com/CaddyGlow/rust-msi/commit/1d9a8fe80766ed81136ab2b8c3b0574f59784d83)
on `ms-package-0.10.2`, tagged
[`caddy-msi-v0.10.2`](https://github.com/CaddyGlow/rust-msi/tree/caddy-msi-v0.10.2),
and published after qualification on 2026-10-09. The registry artifact embeds
that exact VCS revision. The FFI crate remains unpublished. Database APIs and
the default database-only dependency graph remain intact; cabinet I/O uses
registry ms-cabinet 0.1.4 behind the media feature. See
[the refactoring qualification](msi-media-refactoring-validation-20261009.md).

## Historical 0.10.1 provenance

The previous registry checksum is
`9313f9a6ec85603645a545a81ecf7c39e5acc8f9336b64b88b294d1fe83ebd16`.

The 0.10.1 source was prepared from the unchanged 0.10.0 registry source and
published on 2026-10-08 after authoring qualification. It corrects physical
primary-key ordering of serialized MSI tables while preserving decoded logical
ordering for public Select results. The exact source patch, base/source hashes,
package verification and native lifecycle boundaries are recorded in
[caddy-msi-authoring-candidate.md](caddy-msi-authoring-candidate.md).
The source was packaged and published before its Git commit. The exact qualified
patch was subsequently committed as
[`5cea02ecba35c02bfc59ff2547c33a94695628f6`](https://github.com/CaddyGlow/rust-msi/commit/5cea02ecba35c02bfc59ff2547c33a94695628f6)
on `ms-package-0.10.1` and pushed with annotated tag
[`caddy-msi-v0.10.1`](https://github.com/CaddyGlow/rust-msi/tree/caddy-msi-v0.10.1).
The registry artifact predates that commit and has no embedded 0.10.1 VCS
metadata. The committed source files match the qualified/published source hashes.
The FFI crate was not published.

Historical 0.10.0 provenance remains unchanged: its release tag is
`caddy-msi-v0.10.0`, and its source commit is `ae5d17e` on the
`ms-package-0.10.0` branch. Before registry publication, builds used Git commit
`32b12989452a8f16eab2cdf6fc5053a2b773f609` directly.

The fork is based on upstream `mdsteele/rust-msi` commit
`3a7e4a00b344e909ebba86629a19b9a6b284db51`, the msi 0.10.0 release previously
copied from crates.io into `vendor/msi`. The upstream workspace, FFI crate,
public APIs, and MIT license are retained. The schema metadata patch matches the previously vendored source.
A later Clippy compatibility fix uses checked division for table row counts
with the same zero-width behavior, and `write_all` in the signature fixture.

Required `_Tables`, `_Columns`, and `_Validation` metadata cells are checked
instead of unwrapped. Malformed values return `InvalidData` rather than
panicking. The fork contains the unchanged synthetic fuzz input from
2026-10-05, iteration 935, and a direct parser regression test. The original
input and package-level regression remain in `tests/fixtures` and
`tests/msi_malformed_metadata.rs` here. Its SHA-256 is
`f2e7fdd74ef460e2f5a18f7746d8c345183efdcfc186db433305e90c064b5bf6`.

The MSI license is retained locally in `licenses/msi/LICENSE`, so source and
binary release bundles retain its notice without a vendored source tree.
To update the fork, review changes and run both its parser regressions and
the `ms-package` tests before changing the pinned version and Cargo lockfiles.

The fork workspace, including its FFI crate, now requires Rust 1.99 and
uses edition 2024. Edition migration preserves macro matching and adjusts
reference patterns; formatting, full workspace tests, and strict Clippy
were verified with Rust 1.99.0.
