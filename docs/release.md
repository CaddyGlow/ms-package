# Source release procedure

Historical version 0.1.0 was a preview of read-only APPX/MSIX and MSI inspection. It is a
crates.io and GitHub release. The archive foundation crates must be published
first; the patched MSI dependency is available as `caddy-msi` 0.10.1.

Version 0.2.0 adds experimental authoring, tracked in
[authoring qualification](package-authoring-validation.md). Its optional APIs
do not change the historical 0.1.0 inspection-only release evidence. Authoring
profiles require their own independent gates before publication claims expand.

## Reproduce the source artifact

Install Rust 1.99.0, Python 3.12 or newer, and a native C/C++ build toolchain.
Keep ms-package, archive-rs, cabinet, ms-compress, wim-rs and mkiso-rs
in adjacent directories. Preserve their licenses and fixtures.

```sh
python3 scripts/source-release.py --output /tmp/ms-package-0.2.0-source.tar.gz
python3 scripts/source-release.py --verify /tmp/ms-package-0.2.0-source.tar.gz
```

Official packaging requires every source checkout to be clean and committed.
`--allow-dirty` creates an explicitly marked local preview whose manifest records
file hashes and dirty/uncommitted repositories. Never publish that as an official
release. Archive timestamps, ownership and modes are normalized so identical source inputs
produce identical compressed artifacts. All Cargo registry and Git sources, including the patched MSI fork,
are bundled with Cargo checksums and licenses. Verification extracts the archive,
checks every file hash and runs all-target package tests and doctests offline
with a fresh Cargo home and target directory. Compilation uses bundled sources,
not sibling checkouts or a populated Cargo cache.

## GitHub release

Commit and push the reviewed sources. The checked-in `.github/release-dependencies.json` records the frozen dependency
commits used for the first release. Set the repository variables to those
commits before pushing the package tag.
To override them, configure repository variables
`ARCHIVE_RS_REF`, `CABINET_REF`, `MS_COMPRESS_REF`, `WIM_RS_REF`, `MKISO_RS_REF`
to exact 40-character commit hashes. These revisions must
contain the compatible sources validated with ms-package. Repository URLs may
be overridden using the corresponding `_REPOSITORY` variables. Release jobs
reject moving branches, tags and empty revisions. Tag `v0.2.0` only after the
reviewed source and dependency commits are available to CI.

The tag workflow runs native tests, strict Clippy, doctests, the pinned Rust
minimum, WASM compilation, Windows tests and Chromium Worker checks. It then
creates the source bundle, verifies an extracted copy offline, publishes the
crate on crates.io, and creates the GitHub Release
with SHA-256 checksums and the checked-in changelog. Repositories and source
artifacts are public only after an explicit publication decision.

## Qualification boundaries

The MSI metadata limit bounds aggregate encoded table and summary streams before
the MSI parser opens them. Declared string lengths must fit `_StringData` before
string-buffer allocation. The CFB directory is parsed under the storage-byte
bound; decoded strings, table cells and directory structures have overhead, so
this is not a process-wide allocator budget. Callers handling hostile packages
should use an isolated process with an OS memory limit if that guarantee is
required.

Existing synthetic and msitools comparisons do not establish complete Windows
Installer or Microsoft APPX interoperability. Broad real-package comparisons,
Windows-native independent tool results and sustained fuzz campaigns must retain
input hashes, tool versions, decoded payload hashes and failures before any
stronger support claim. Windows CI checks Rust behavior, not installation.
Signature validation, publisher trust, sparse/encrypted packages, upload
containers, transforms, patches and installation remain outside this preview.
