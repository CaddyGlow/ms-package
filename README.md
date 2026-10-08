# ms-package

Portable inspection and payload integrity with optional experimental authoring;
see [release scope](docs/release.md) and [changelog](CHANGELOG.md).

Standalone repository for the `ms-package` crate, preserving APPX/MSIX and
MSI APIs. Crate dependencies resolve from crates.io. Browser and source-release
checks use compatible sibling `archive-rs`, `cabinet`, `ms-compress`, `wim-rs` and
`mkiso-rs` checkouts. The patched MSI reader comes from the
[CaddyGlow/rust-msi fork](https://github.com/CaddyGlow/rust-msi/tree/caddy-msi-v0.10.1),
published as `caddy-msi` and pinned to version `0.10.1` in `Cargo.toml`.
See [fork provenance](docs/msi-fork.md).

Portable package interpretation with experimental authoring behind the optional
`write` feature. The default configuration retains the inspection APIs.
See [authoring profiles and limits](docs/package-authoring.md) and the
[backend audit](docs/package-authoring-audit.md). Signing, custom action execution,
and implicit host media discovery remain outside the portable APIs.

`AppxPackage` reads a single APPX/MSIX ZIP using `archive-core`, preserves manifest
bytes, parses XML metadata, and validates SHA-256 hashes of decoded 64-KiB blocks.
Package integrity never implies signature verification or publisher trust.
`AppxBundle` inspects declared architecture/resource identities and requires an
explicit nested filename selection with a decoded-byte bound. Selection binds the
nested manifest's Name, Publisher, Version, architecture, and ResourceId to the
bundle declaration. `validate` separately checks the outer block map and metadata
coverage; callers must also validate each selected nested package. The test-only
fixture producer emits two-architecture valid/corrupt bundles and native expected
JSON for inspection tests. Optional authoring uses separate public types.
Encrypted packages, upload containers, sparse packages, and
signature verification are currently explicit unsupported profiles.

`InstallerPackage` adopts `msi` 0.10 and its `cfb` 0.14 backend for portable compound
storage, encoded stream names, string pools, schemas, typed cells, and summary
information. File, Directory, Component, and Media references produce declared
payload paths. Embedded cabinets use `archive-core` (and its `cabinet` backend);
external cabinets and loose files require a caller-supplied `MediaResolver`.
Transforms and patches are rejected. Destination paths describe declared metadata,
not the result of evaluating installation conditions.

## Dependency audit

`caddy-msi` (imported as `msi`), `cfb`, `quick-xml`, `base64`, and `thiserror`
are MIT licensed; `sha2` is MIT OR Apache-2.0. These dependencies operate on portable Rust I/O and do not need
Windows APIs or subprocesses. Their native filesystem convenience methods are not
used. No signature/trust backend is linked. The upstream MSI/CFB parser owns its
sector-chain validation; caller row limits bound returned table rows, but do not
impose a process-wide heap limit during initial opening.
`open_with_metadata_limit` bounds aggregate encoded metadata streams and checks
string-pool lengths before the MSI parser allocates string buffers.
`open_bounded` caps the whole compound-storage input before opening; the convenience
`open` sets a 512-MiB storage ceiling. Both convenience openers cap encoded
metadata at 64 MiB (or the smaller storage bound); use
`open_with_metadata_limit` to choose an explicit metadata budget.

Detailed specification references consulted on 2026-10-05:

- [APPX block schema](https://learn.microsoft.com/en-us/uwp/schemas/blockmapschema/element-block): decoded 65536-byte blocks and block-map namespace.
- [Bundle Package schema](https://learn.microsoft.com/en-us/uwp/schemas/bundlemanifestschema/element-package): nested architecture, resource, version, and size declarations.
- [MS-CFB revision 12.0, 2024-04-23](https://learn.microsoft.com/openspecs/windows_protocols/MS-CFB/53989ce4-7b05-4f8d-829b-d08d6148375b): compound-storage structures delegated to `cfb`.
- [MS-CFB header fields](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/05060311-bfce-4b12-874d-71fd4ce63aea): header signature and sector layout.

## Evidence and remaining gates

The generated `browser_fixtures` example uses canonical Installer table column
ordering and a TARGETDIR root. On 2026-10-05, independent `msitools` read its File
table and `msiextract` extracted the 25-byte `payload.txt`, whose SHA-256 is
`19ba9cbc1fdf70e5059a31e80c841ac2acfed882225d9d5f33b546f1fb208ba7`.
This is a synthetic portable-reader comparison, not Microsoft deployment evidence.

On 2026-10-06, `msitools` 0.106 independently exported the generated external-media
Installer's File and Media rows (`payload`, size 25, `data.cab`) and `msiextract`
extracted the same payload hash above. The fixture producer also emits a mixed
embedded/external Installer whose second cabinet is deliberately absent, for
checking late-failure handling without simulating installation.

Synthetic regressions cover decoded block-map mismatch, malformed XML and compound
storage, duplicate/traversing block-map records, exact stream bounds, directory
cycles, short/long directory naming, embedded/external cabinets, explicit missing
media, loose files, and bounded compound-storage opening. Native unit tests and
`cargo check -p ms-package --target wasm32-unknown-unknown` pass. These do not establish Microsoft-tool
interoperability. Synthetic tests cover multiple cabinets and loose media; the
parent browser gate records actual WASM/native parity for its explicit fixtures.
Independent Microsoft fixture comparisons, broader real-world multi-cabinet and
loose-media profiles, and sustained parser fuzzing remain required before claiming
the complete package phases. Resource bundle regressions cover the standard
omitted Type/Architecture defaults and ResourceId identity binding. The parent
archive capability document records target-specific archive backend availability.

## Use

Requires Rust 1.99.0. Source release archives include sibling sources and vendored
Cargo dependencies; see `BUILD.md` at the archive root for offline build commands.
From a checkout, run `cargo test --all-targets --all-features --locked`.

```rust,no_run
use std::fs::File;
use ms_package::{AppxPackage, InstallerPackage};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut appx = AppxPackage::open(File::open("example.msix")?, Default::default(), 1 << 20)?;
    let integrity = appx.validate(256 << 20)?;
    println!("Verified {} files", integrity.files_verified);

    let installer = InstallerPackage::open_with_metadata_limit(
        File::open("example.msi")?, 10_000, 512 << 20, 16 << 20,
    )?;
    println!("Tables: {:?}", installer.tables());
    Ok(())
}
```

## Publication

`ms-package` is published on crates.io after `caddy-archive-core` and `ms-cabinet`.
Its dependency aliases preserve the `archive_core` and `cabinet` imports.
Version tags validate the library and browser bindings, verify the offline source
bundle, publish the crate, and create the GitHub Release.

Current development uses published `caddy-archive-core` 0.2.1. The optional
`write` feature also supports Deflate through `ms-compress` and caller-managed
external, loose and multiple-cabinet MSI media. See the
[authoring API and supported profiles](docs/package-authoring.md).
