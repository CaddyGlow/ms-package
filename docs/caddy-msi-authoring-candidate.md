# MSI authoring backend 0.10.1 release

Prepared, qualified and published to crates.io on 2026-10-08 after explicit
user authorization. `ms-package` now pins the registry package `caddy-msi =0.10.1`.
This filename retains the candidate preparation record and its release provenance.

## Source and review artifacts

The candidate is `/data/cache/ms-package-authoring-caddy-msi`, version 0.10.1,
copied from the untouched `/data/cache/caddy-msi-release` source. Its changed
source files, MIT license, and retained malformed fixture were compared with
the installed crates.io 0.10.0 source. The base source commit recorded in that
package's `.cargo_vcs_info.json` is
`ae5d17e4d49609c7bb9bb67c0bfdb72c97479f74`. Upstream and historical provenance
remain in [msi-fork.md](msi-fork.md).

The [review patch](caddy-msi-physical-key-order.patch) applies cleanly to that
base with `git apply --check`. It changes the workspace version, lockfile,
table serialization, logical query ordering, and two direct regressions.
Crate names, public signatures, FFI workspace, license and historical fixtures
are retained. Publication used the verified crate artifact described below, prepared from an
uncommitted local directory. After publication, the exact qualified patch was
committed and pushed from the separate clean checkout
`/data/cache/ms-package-authoring-caddy-msi-github-20261008`.
The source commit is
[`5cea02ecba35c02bfc59ff2547c33a94695628f6`](https://github.com/CaddyGlow/rust-msi/commit/5cea02ecba35c02bfc59ff2547c33a94695628f6)
on `ms-package-0.10.1`. Annotated tag `caddy-msi-v0.10.1` has object ID
`e395db89f8be6959e6452c45cc03ddaaca58fa75` and points to that commit. Qualified
source hashes were verified before commit; the published archive predates the
commit and contains no embedded 0.10.1 VCS metadata.

| Artifact | SHA-256 |
| --- | --- |
| Review patch | `f3d5bfe9f3d9fd4bfcf04f2696502484d593c26e690e5666292966feb3002eeb` |
| Base `src/internal/table.rs` | `3b162ccfc407b190225e7b2856db1feb4f2a34dc33d6575cc19ac1e13bd7c49a` |
| Base `src/internal/query.rs` | `b75c858ea235948db65f74361335fbf97dca884a9d5e980ab5f741daccbc491b` |
| Candidate `src/internal/table.rs` | `fced53e5291547a5efe90fdf9e887a68b8844ad1f38f8427a557187f92225492` |
| Candidate `src/internal/query.rs` | `8cd936c5bed5408318211d968ea5ded7f3ca3efd828cdacc90d26749731151fc` |
| Retained malformed MSI fixture | `f2e7fdd74ef460e2f5a18f7746d8c345183efdcfc186db433305e90c064b5bf6` |

## Defect and correction

The 0.10.0 writer orders primary keys by decoded string values. Native Windows
Installer catalog indexes require physical rows ordered by numeric string-pool
references. A fresh `Custom` table failed native database opening; the identical
schema named `zCustom`, whose lexical ordering matched reference ordering,
opened successfully. This isolated the defect without payloads or installer
actions. Fixed-length strings reproduced it, excluding unlimited string columns.

The candidate sorts serialized rows by encoded primary keys across all tables.
Public Select results retain logical ordering by decoded primary keys. Direct
regressions inspect `_Tables`, `_Columns`, ordinary table references and numeric
secondary keys, and verify logical ordering after reopening.

Version-3 CFB storage was an earlier rejected hypothesis: native-created MSI
uses version 4, and authored streams still failed when transplanted into a
native container or when summary information was removed. The
[rejected V3 patch](caddy-msi-v3-authoring.patch) is diagnostic history, not a
release fix. Candidate creation retains the existing version-4 storage backend.

## Qualification

The Rust 1.99 development environment passed these candidate workspace checks:

```sh
cargo fmt --all
cargo test --all-targets --all-features --locked
cargo test --doc --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo package -p caddy-msi --allow-dirty --locked --offline
```

The library ran 63 unit tests, including the new physical ordering regressions,
and 17 doctests. Retained parser and database integration suites passed. The
FFI workspace compiled and its targets passed. Package verification compiled
the extracted 40-file source package successfully.

The verified crate artifact is
`/data/cache/rust/package/caddy-msi-0.10.1.crate`, 68,295 bytes, SHA-256
`9313f9a6ec85603645a545a81ecf7c39e5acc8f9336b64b88b294d1fe83ebd16`.

Using the isolated candidate override, the explicit x64, per-user, single
unversioned data-file example installed, repaired and uninstalled on Windows
with exit code 0 for every step. Its payload hash matched and uninstall removed
the payload. Artifact `/data/cache/ms-package-authored-encoded-order.msi` has
SHA-256 `3529c365ed0e09b297a0daa1c90151868e64354c4b935524a28ddca70216c36f`.
Native evidence is recorded in [package-authoring-validation.md](package-authoring-validation.md).

A canonical payload edit applying replacement, rename, add and remove operations
with a distinct explicit PackageCode also passed installation, deletion/repair
and uninstall, each with exit code 0. See the [edited lifecycle result](evidence/package-authoring-20261008/edited/result.json).
The edited package SHA-256 is
`7a6dca4af5814629f336a060cb214c24518fab5026ae833ca4e785bf0db07692`;
its installed/repaired payload SHA-256 is
`93bb899f71227543b80b530867d954845c1c18ec78b0d54fe7ee1bda25e4c902`.
The recorded Windows OS version is 10.0.26200.0 and Installer version is
5.0.26100.7920.

These results qualify this tested backend/profile. They do not establish
x86, per-machine, upgrades, versioned binaries, arbitrary table profiles, or
cryptographic signing. Diagnostics and rejected hypotheses are retained under
`/data/cache/ms-package-authoring-diagnostics` and the authoring evidence paths.

## Publication and consumption

The qualified `caddy-msi` 0.10.1 crate was uploaded successfully to crates.io.
The registry checksum matches the verified artifact SHA-256 above. The main
manifest and lockfile now resolve that exact registry version; local overrides
remain confined to the earlier isolated qualification checkout. Compatibility,
browser and source-release checks for the consumed registry package are recorded
in [package-authoring-validation.md](package-authoring-validation.md).

The matching Git source branch `ms-package-0.10.1` and annotated release tag
`caddy-msi-v0.10.1` were pushed atomically to `CaddyGlow/rust-msi` after explicit
user authorization. Historical branches and tags were preserved. The previous
0.10.0 tag has no GitHub Release object, so this update followed the existing
branch/tag convention without creating a new Release object. No FFI crate was
published. `ms-package` publication is a separate action and is not implied by
the backend publication.
