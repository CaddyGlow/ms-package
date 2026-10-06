# MSI reader fork

`ms-package` uses [caddy-msi 0.10.0](https://crates.io/crates/caddy-msi/0.10.0),
from [CaddyGlow/rust-msi](https://github.com/CaddyGlow/rust-msi), pinned to
`=0.10.0` in `Cargo.toml` and imported under the existing `msi` name.
Cargo fetches the published registry package automatically; `Cargo.lock`
records its checksum. The release tag is `caddy-msi-v0.10.0` and its source
commit is `ae5d17e` on the `ms-package-0.10.0` branch.
Previously, builds used Git commit
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
