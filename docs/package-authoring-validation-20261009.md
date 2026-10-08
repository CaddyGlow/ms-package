# Authoring follow-up qualification, 2026-10-09

This extends the [initial qualification](package-authoring-validation.md).
Historical fixtures, rejected experiments and release evidence remain intact.
The new work qualifies the `ms-package` 0.2.1 follow-up to 0.2.0.

## Dependencies and supported profiles

Published dependencies resolve from crates.io: `caddy-archive-core` 0.2.1,
`caddy-msi` =0.10.1, and `ms-compress` 0.1.2. The archive release source is
`cb2394c1bc5faf518a28b6edaa739c11e816cb2d`. XML requirements remain ordinary
caret-compatible `woxml` 0.6.0 and `roxmltree` 0.21.1, with default features disabled.

APPX/MSIX output now supports raw Deflate with independent 64 KiB full-flush
blocks, physical block lengths, and a final stream terminator outside the last
block's recorded length. Metadata remains stored. Decoded and physical block
checks run before compressed source editing. Microsoft's declared 2021 block-map
`FileHash` extension is accepted and its whole-file SHA-256 digest is verified.
Other unsupported extensions fail closed. This is a supported compressed profile,
not a claim that every arbitrary Deflate package can be rebuilt.

Canonical file-only MSI output additionally supports external stored cabinets,
loose files, and explicitly partitioned nonspanning embedded/external cabinets.
Caller sinks own sidecar destinations and finalization. Reports distinguish raw
database validation from the canonical profile and describe identity changes.
Partial media failures identify completed artifacts and bytes written.

## Microsoft format and lifecycle checks

Microsoft MakeAppx 10.0.26100.1742 successfully unpacked the original and edited
Deflate fixtures containing empty, small, 64 KiB boundary and multi-block payloads
with both repetitive and incompressible content. It repacked the extracted
original fixture. The editor then opened that Microsoft-generated compressed
package and rebuilt it; MakeAppx successfully unpacked the resulting package.
See [original unpack](evidence/package-authoring-20261009/deflate-unpack.log),
[edited unpack](evidence/package-authoring-20261009/deflate-edited-unpack.log),
and [native-source rebuild](evidence/package-authoring-20261009/deflate-native-rebuilt-unpack.log).
These are unsigned format checks, not signing, trust or deployment claims.

Resource bundles and their edited variants passed MakeAppx unbundling after
correcting the Windows 10 resource manifest: omit explicit architecture and
retain the schema's `TargetDeviceFamily` compatibility dependency. Package
dependencies remain forbidden. The initial and second rejected fixtures and
diagnostics are retained; see [successful resource results](evidence/package-authoring-20261009/resource-v3-results.json).

The Windows Installer lifecycle matrix passed all 18 cases: x86/x64 and
per-user/per-machine across embedded, external, loose and two-cabinet output,
plus canonical external and mixed-media payload edits. Every case installed with
matching payload hashes, repaired after deleting every installed payload, and
uninstalled with all payloads absent. See the
[matrix result](evidence/package-authoring-20261009/msi-matrix/matrix-result.json),
[edited result](evidence/package-authoring-20261009/msi-matrix/edited-result.json),
and [matrix provenance](evidence/package-authoring-20261009/msi-matrix/README.md).
The final archive-core/cabinet dependency update reproduced the qualified bytes;
the [equivalence receipt](evidence/package-authoring-20261009/msi-matrix/core021-equivalence.json)
retains both dependency sets without rewriting the original receipts.

## Stateful authoring checks

The fuzz harness compares an independent payload model after each add, replace,
remove or rename operation. Eight initial modes exercise Stored/Deflate APPX
and embedded/external/loose/multiple MSI output with in-memory sinks and bounded
resolvers. Corpus, truncation and mutation smoke checks passed. Three instrumented
targets completed 129 iterations each with no crashes or timeouts; see the
[campaign receipt](evidence/package-authoring-20261009/fuzz-media/summary.json)
and [reproduction notes](evidence/package-authoring-20261009/fuzz-media/README.md).
The first build lacked BFD headers; its setup failure is retained separately
from the successful instrumented run.

## Repository and browser gates

Locked all-target/all-feature tests, default and writer doctests, strict Clippy,
formatting, reader-only compilation, WASM compilation and strict generated API
documentation passed with the final registry dependency set. Windows ran 12 test
executables and 79 tests successfully; see the
[Windows test log](evidence/package-authoring-20261009/windows-rust-tests.log).

Both archive-rs Worker gates passed at the recorded 0.2.1 source revision. The
[authoring result](evidence/package-authoring-20261009/browser-core021/authoring-worker.json)
covers native/browser byte parity for three-block Deflate packages and nested
bundles, MSI external/loose/multiple media artifacts, stored authoring, database
preservation and limit failures. The
[archive baseline](evidence/package-authoring-20261009/browser-core021/baseline-worker.json)
also covers the existing format readers, encrypted ZIP/7z and Blob cancellation.
GitHub integration variables and the frozen dependency record now identify these
compatible sources. Integration reconciliation changes only staged checkouts;
published crate dependencies retain registry sources.

The [source preview receipt](evidence/package-authoring-20261009/source-preview/receipt.json)
records the exact dirty-source snapshot and archive hash. Extraction verified
every file, then passed all-target/all-feature tests, both doctest configurations,
reader-only compilation and browser helper compilation offline with fresh Cargo
and target directories. See the [verification log](evidence/package-authoring-20261009/source-preview/verify.log).
This is a verified preview; subsequent documentation receipts are outside its
exact snapshot. The supplied sibling repositories were preserved.

## Remaining boundaries

Output remains unsigned ZIP32. ZIP64 output, arbitrary manifest-schema validation,
signed MSI rebuilding, cabinet spanning, versioned PE payloads, transforms,
patches and general installer authoring remain unsupported. Native lifecycle
checks cover the flat data-file profile; they establish no upgrade or rollback
semantics. Scratch limits bound logical retained buffers and codec workspace,
not every allocator or parser allocation. Bounded fuzz campaigns are smoke checks,
not sustained fuzzing or security qualification.
