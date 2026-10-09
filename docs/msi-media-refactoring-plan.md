# MSI media ownership refactoring plan

Status: implemented for ms-package 0.2.2 and caddy-msi 0.10.2; see [qualification](msi-media-refactoring-validation-20261009.md).

Move reusable MSI media semantics into the owned `CaddyGlow/rust-msi` fork,
published as `caddy-msi`. Keep the existing `ms-package` APIs as compatible
wrappers. This is an ownership refactor with unchanged supported profiles;
compression algorithms, cabinet spanning and broader installer authoring are
separate work.

## Ownership

Repository-specific execution plans accompany this design:

- `ms-package`: [wrapper migration](msi-media-ms-package-refactoring-plan.md).
- `caddy-msi`: `docs/msi-media-refactoring-plan.md` in the owned rust-msi fork.
- `ms-cabinet`: `docs/msi-media-refactoring-plan.md` in the cabinet repository;
  code changes are conditional on the backend primitive audit.
- `archive-rs`: `docs/msi-media-refactoring-plan.md` in the archive workspace.

`ms-compress`, `wim-rs` and `mkiso-rs` need no implementation changes for this
ownership refactor. Cabinet codecs and unrelated image formats remain intact.

| Responsibility | Owner after migration |
| --- | --- |
| File sequence mapping, Media boundaries and cabinet references | `caddy-msi` |
| Reusable media layout planning and validation | `caddy-msi` |
| Embedded cabinet stream insertion and retrieval | `caddy-msi` |
| MSI compressed/uncompressed media flags and table coordination | `caddy-msi` |
| Cabinet serialization, extraction and compression algorithms | `ms-cabinet` and its codec dependencies |
| Bounded media emission, explicit resolver/sink contracts and completion accounting | `caddy-msi` media layer |
| Flat-file installer profile, component identity policy and execute sequences | `ms-package` |
| Canonical builder-output admission and payload-edit preservation policy | `ms-package` |
| Aggregate package limits, MSI/sidecar publication order and package-level reports | `ms-package` |
| APPX/MSIX and bundle authoring | `ms-package`, unchanged |

The dependency direction stays `ms-package -> caddy-msi -> ms-cabinet` for the
optional media backend. Neither backend imports `ms-package` or its errors,
limits, reports or authoring profile. Archive and browser integration remain
outside the MSI backend.

## Existing code to migrate

- `src/authoring/installer_media.rs`: split reusable layout preparation,
  cabinet grouping, media-name checks, bounded emission and failure accounting
  from profile-specific filename/directory restrictions.
- `src/authoring/installer_builder.rs`: delegate Media row construction,
  sequence partitioning, cabinet stream handling and reusable source-media
  interpretation. Keep canonical stream comparison, GUID policy and the flat
  installer schema/actions here.
- `src/installer.rs`: audit reusable Media/File relationships, embedded cabinet
  lookup and loose-file resolution. Move these incrementally after the writer
  migration; preserve the public reader and `MediaResolver` interface.

Do not move the entire installer builder into the fork. Its one-feature,
one-file-per-component and flat ASCII data-file restrictions are a package
authoring profile, not universal MSI rules. Backend validation must distinguish
MSI representation constraints from those narrower restrictions.

## Backend contracts

Add an additive media module to `caddy-msi`, using an optional feature where
cabinet encoding/decoding dependencies are required. Database-only consumers
should retain their existing default dependency graph. Determine whether pure
table planning can remain available without the cabinet feature during the audit.

The backend should expose typed file-sequence inputs, embedded/external/loose
layout descriptions, a validated media plan and explicit execution methods.
Planning must validate complete sequence coverage, ordered boundaries, safe
relative media names, duplicate/conflicting artifacts, schema representability
and resource budgets before opening a destination. A validated plan must not
allow callers to mutate its inputs into an inconsistent execution state.

Use backend-owned limits and errors. Bound actual resolver reads and emitted
bytes; do not rely only on declared sizes. Account for retained payload,
prepared media and cabinet workspace without claiming a process-wide heap cap.
Accept explicit caller-controlled I/O; do not add automatic filesystem lookup,
temporary files, installation, custom-action execution or signature/trust checks.

Preserve sink behavior: write all bytes, flush, then finalize; record completion
only after finalization succeeds. Failure information must include completed
artifact names, the incomplete artifact, actual partial bytes and the underlying
cause. Document that completion does not imply durable or atomic publication.
The package wrapper still emits finalized sidecars before its final MSI output.

## Public compatibility

Preserve these `ms-package` symbols and their existing signatures:

- `InstallerMediaLayout`, `InstallerCabinetSpec`, `InstallerMediaSink`,
  `InstallerMediaArtifact`, `InstallerMediaReport`, and `write_installer_media`.
- Builder/editor `write`, `write_detailed` where available, `write_with_media`,
  and payload-editor `open_with_media`.
- `MediaResolver`, reader extraction methods, `WriteError::Media`, and detailed
  package/identity reports.

Prefer thin local adapters initially. Convert local limits, layouts, sinks,
resolvers and backend failures without changing public types or their trait
implementations. Direct reexports are acceptable only after proving that type
identity, signatures and downstream implementations remain compatible.
Existing embedded output remains the default. Existing output bytes, ordering,
error completion context and supported admission rules remain regression targets.

## Implementation batches

1. **Audit and baseline.** Inventory duplicated reader/writer media logic, record
   current API signatures and ownership, and capture deterministic artifacts for
   every supported layout. Use the existing dated evidence as the qualification
   baseline; preserve it unchanged.
2. **Backend planning.** Add backend-owned sequence/layout types, bounded plans
   and direct validation regressions. Separate profile restrictions from general
   MSI constraints. Do not change package callers yet.
3. **Backend execution.** Add optional cabinet integration, embedded stream
   operations and explicit sink/resolver adapters. Test partial writes, flush,
   resolver and finalization failures at the backend boundary.
4. **Writer delegation.** Replace package-level implementations with adapters.
   Keep aggregate budgets, canonical admission and installer identity policy in
   `ms-package`. Remove duplicated internals only after equivalence checks pass.
5. **Reader delegation.** Migrate reusable media interpretation/extraction in a
   separate reviewable change. Preserve bounded reads, path reporting and the
   explicit missing-media behavior of the existing reader.
6. **Qualification and release.** Run both repositories' gates, validate the
   browser integration and official offline source bundle, then publish the fork
   before updating the package's registry dependency and releasing its wrappers.

Each batch must be independently reviewable. Use isolated fork checkouts and
temporary local overrides for development; published manifests use crates.io.
Preserve fork licensing notices, FFI APIs, parser regressions and historical
fixtures throughout.

## Validation and acceptance

Backend tests cover empty/invalid partitions, missing/duplicate sequences,
boundary overflow, invalid cabinet references, embedded stream consistency,
loose-file resolution, path collisions, exact limits and actual I/O failures.
Include a standalone backend example proving media operations work without
`ms-package`. Run fork workspace tests, FFI checks, formatting and strict Clippy.

Package tests must preserve the current public callers and failure semantics.
Compare MSI and sidecar bytes against the 63 artifacts recorded in the
[0.2.1 equivalence receipt](evidence/package-authoring-20261009/msi-matrix/core021-equivalence.json).
If an intentional representation change prevents byte equality, investigate and
document it and repeat the independent native gate before release.

Run the repository's locked all-target/all-feature tests, default and writer
doctests, strict Clippy, reader-only and WASM checks. Run archive-rs baseline and
authoring Worker checks, including native/browser MSI media parity and limits.
Exercise the stateful edit fuzz model and bounded instrumented smoke campaign.
Repeat the qualified x86/x64, per-user/per-machine lifecycle matrix for embedded,
external, loose, multiple and edited mixed media, checking all installed payload
hashes, deletion/repair and uninstall. Verify an extracted source archive offline
with fresh Cargo and build directories.

The refactor is complete when reusable media behavior is implemented and directly
tested in `caddy-msi`, package code delegates through compatible wrappers, all
qualification gates pass, and the published dependency/source provenance is
updated. No new format or servicing support is implied by moving the code.
