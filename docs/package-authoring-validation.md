# Authoring qualification, 2026-10-08

This is new authoring evidence. Historical inspection-only release records and
fixtures remain unchanged. The implementation is experimental until each
declared profile passes its independent gates.

## Implementation and automated checks

The optional `write` feature implements stored ZIP32 single-package creation and
editing, bundle creation and canonical editing, raw MSI database operations, and
an experimental flat embedded-cabinet data-file builder and canonical payload
editor. Manifest bytes and effective MIME mappings survive supported APPX edits.
Unsupported extensions/storage/signature profiles fail explicitly.

Host regression checks cover 64-KiB block boundaries, empty files, deterministic
bytes, actual-byte limits, failed reads/destination flushes, ZIP local headers,
URI content-type paths, XML extension/MIME preservation, signature removal,
nested identity/integrity, bundle offsets/resources, custom MSI data, typed row
and declared foreign-key checks, stream replacement tails, representability,
explicit identity policies, and coordinated payload edit operations.

XML generation/parsing uses `woxml` 0.6.0 and `roxmltree` 0.21.1, both with
default features disabled. A standalone allocation-enabled `#![no_std]` check
compiled both backends for `thumbv7em-none-eabi` using Rust 1.99.0. This verifies
the XML backends; the package crate and its I/O remain `std`.

The authoring Worker harness at `tests/browser-authoring` passed using the
archive-rs browser runner and package, bundle and MSI readers. It compares native/browser
creation and edited bytes for all three formats, validates outer and selected
nested package integrity, compares decoded payloads, verifies custom MSI table
and stream preservation, and checks creation-limit errors. MSI browser checks
exercise databases, not native installation. Cancellation is synchronous:
the caller may terminate the Worker; interruption within a Rust call is not
claimed. Reproduce after building archive-rs bindings with `packages`:

```sh
bash scripts/check-authoring-browser.sh ../archive-rs /tmp/archive-browser/pkg
```

The script needs Rust's WASM target, matching `wasm-bindgen-cli`, Node with
WebSocket support, Python and Chromium. It retains generated artifacts/logs and
prints their location. CI runs it after the existing archive-rs Worker checks.

## Independent APPX and bundle experiments

A disposable `win11-dev` VM ran Microsoft MakeAppx from the published
[Windows SDK BuildTools 10.0.26100.1742](https://www.nuget.org/packages/Microsoft.Windows.SDK.BuildTools/10.0.26100.1742).
The executable file version and OS information belong with retained diagnostics.
The tool's adjacent packaging, OPC and signature manifests/DLLs are required.

MakeAppx successfully unpacked the representative stored single-package output
and repacked the extracted manifest/payloads with validation enabled. The initial
simplified application bundle fixture failed manifest validation. The corrected
fixture includes complete application metadata, supported language resources and
correctly sized PNG logos; MakeAppx successfully extracted both architecture
packages from its bundle. Dummy executable payloads are format fixtures, not
deployment evidence. See [unpack diagnostics](evidence/package-authoring-20261008/makeappx-unpack.log),
[repack diagnostics](evidence/package-authoring-20261008/makeappx-repack.log),
[initial bundle failure](evidence/package-authoring-20261008/makeappx-unbundle.log)
and [corrected bundle diagnostics](evidence/package-authoring-20261008/makeappx-unbundle-v2.log).

These experiments establish acceptance of explicit representative outputs;
they do not establish complete manifest-schema validation by the portable API,
arbitrary real-package preservation, signing, trust or deployment.

## MSI gate and retained failures

`msitools` 0.106 independently exported the authored File table and extracted
its 44-byte `payload.txt`. SHA-256:
`99c3bde76b15abb8280c56cb563623c054fbc7c00c0451b98b0a1b03491b6000`.
Windows Installer rejected the initial output with error 1620 before installation.
Changing UTF-8 code pages to Windows-1252 did not resolve it.

A temporary fork candidate changed CFB creation from version 4 to version 3.
Windows also rejected that candidate; native Windows-created empty MSI databases
use version 4, disproving version 4 as the sole cause. Ole32 opened the candidate's
CFB successfully, while native `MsiOpenDatabaseW` returned 110. Transplanting its
streams into a native-created container and removing summary metadata still
failed. Investigation therefore concerns MSI stream serialization rather than
an established compound-storage incompatibility. The CFB-version hypothesis is
retained as failed evidence; it is not a qualified fork fix.

See [the rejected artifact](evidence/package-authoring-20261008/rejected-cfb-v4.msi)
and [Windows Installer log](evidence/package-authoring-20261008/rejected-cfb-v4-install.log).
`scripts/qualify-installer.ps1` checks actual install, deletion/repair and uninstall
with payload hashes and explicit logs. The corrected backend candidate passed that script: installation, repair after
deleting the installed file, and uninstall all returned zero, with the expected
payload hash. See [the lifecycle result](evidence/package-authoring-20261008/result.json)
and retained install, repair and uninstall logs in the same directory. The
qualified candidate package SHA-256 is
`3529c365ed0e09b297a0daa1c90151868e64354c4b935524a28ddca70216c36f`.

The failure was isolated to physical row ordering: primary keys containing
strings must follow numeric string-pool references in serialized table streams.
The old backend sorted decoded strings. The candidate sorts encoded primary
keys while preserving logical ordering for public queries; see the
[candidate patch](caddy-msi-physical-key-order.patch).

A canonical payload editor rebuilt the candidate with replacement, rename, add
and remove operations and a new explicit PackageCode. That edited package also
passed install, deletion/repair and uninstall; see its
[lifecycle result](evidence/package-authoring-20261008/edited/result.json).
Package SHA-256:
`7a6dca4af5814629f336a060cb214c24518fab5026ae833ca4e785bf0db07692`.
No upgrade behavior was tested or claimed.

The qualified fix is published as `caddy-msi` 0.10.1 and pushed to the GitHub
fork as commit `5cea02ecba35c02bfc59ff2547c33a94695628f6`, tagged
`caddy-msi-v0.10.1`. The backend is consumed through
the exact registry requirement `=0.10.1`. The production-registry example emitted byte-identical MSI output to the
natively qualified candidate. Its direct regressions, retained parser
tests, FFI checks and verified package passed before publication. The
[backend release record](caddy-msi-authoring-candidate.md) retains patch and
source provenance. Local overrides stay confined to isolated qualification
checkouts; the package dependency resolves from crates.io.

## Release boundary

Native format regressions, WASM compilation and Worker parity are separate from
Windows Installer validation, lifecycle and upgrade behavior. Compression, ZIP64,
signed MSI rebuilding, external/multiple cabinet authoring, arbitrary installer
schemas and advanced servicing profiles remain excluded. Source-release preview
verification does not authorize publication or replace a clean source release.

## Executed repository and source checks

Rust 1.99.0 passed formatting, all-target/all-feature locked tests, both reader
and writer doctests, strict all-feature Clippy, reader-only checks, WASM checks
and the Windows test binaries. The existing archive-rs Worker suite also passed
with current package-enabled bindings; its results and the authoring Worker
results are retained in this evidence directory.

The dirty-worktree source preview was built and verified in a fresh extracted
location with a fresh Cargo home, vendored crates and offline execution. It
passed native all-feature tests, reader/writer doctests, a reader-only check and
the standalone browser helper native compilation.
This verifies the production registry dependency set. Browser execution is a separate gate; the extracted preview
was not independently run in a browser. Historical release artifacts were not
modified. The qualified `caddy-msi` backend was published with user approval;
`ms-package` itself has not been published.

The final registry-backed [source preview record](evidence/package-authoring-20261008/source-preview.json)
records artifact and implementation hashes; its [offline verification log](evidence/package-authoring-20261008/source-preview-verify.log)
contains the fresh-build results. The preview archive is
`/data/cache/ms-package-authoring-published-source-20261008.tar.gz`, SHA-256
`2f7053e2d108428c55da6843c80126c302358fbb1a234d7679d513c47966762d`.
