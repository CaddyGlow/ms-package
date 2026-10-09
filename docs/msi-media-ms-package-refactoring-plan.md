# ms-package MSI media migration plan

Status: implemented for ms-package 0.2.2 and caddy-msi 0.10.2; see [qualification](msi-media-refactoring-validation-20261009.md).

## Objective and ownership

Delegate reusable media mechanics to the published caddy-msi backend while
preserving every current reader and authoring API. Keep the flat-file installer
profile, GUID policies, canonical source admission, aggregate package budgets,
publication order and package reports in this crate.

The cross-repository design is in [the ownership plan](msi-media-refactoring-plan.md).
Backend and cabinet code must not depend on ms-package.

## Work batches

1. Capture public signatures and baseline bytes for embedded, external, loose,
   partitioned and mixed media. Preserve the dated 0.2.1 evidence and fixtures.
2. Develop against an isolated caddy-msi checkout with a temporary override.
   Convert WriteLimits and layout inputs to backend-owned contracts. Maintain
   wrappers for InstallerMediaLayout, InstallerCabinetSpec, InstallerMediaSink,
   InstallerMediaArtifact, InstallerMediaReport and write_installer_media.
3. Delegate installer_media.rs planning and emission, plus reusable Media/File
   relationships and cabinet stream coordination in installer_builder.rs.
   Keep profile-specific ASCII names, one-feature/component rules and canonical
   stream comparison here. Apply aggregate budgets across MSI and sidecars.
4. Adapt errors into WriteError::Media without losing completed names, failing
   artifact, partial byte count or underlying cause. Preserve write-all, flush,
   finalization and sidecars-before-MSI behavior. Existing write stays embedded.
5. Delegate reusable reader relationships/extraction in installer.rs separately.
   Keep MediaResolver and existing extraction signatures; use bounded adapters.
   Preserve explicit missing-media errors and reported logical paths.
6. Remove duplicate internals after parity tests pass. Update the registry fork
   requirement, root/fuzz/browser locks, provenance and source-release records
   only after the backend is published. Release the package after integration.

## Compatibility and qualification

Compile existing caller sink/resolver implementations against the wrappers.
Do not replace public types with reexports unless type identity and downstream
trait implementations are proven compatible. Preserve write_with_media,
open_with_media, detailed reports and canonical editor admission rules.

Compare all 63 qualified MSI/media artifacts byte-for-byte. Investigate any
change and repeat the native gate if an intentional difference is accepted.
Test exact budgets, source failures, partial writes, flush/finalization errors,
media ordering and missing/extra source artifacts. Run stateful edit fuzzing.

Required gates: rustfmt; cargo test --all-targets --all-features --locked;
cargo test --doc --locked; writer doctests; strict all-target/all-feature Clippy;
reader-only and WASM builds; Windows tests; archive-rs baseline and authoring
Worker parity. Repeat the 18-case Windows install/delete/repair/uninstall matrix
with all payload hashes. Verify a clean, freshly extracted offline source bundle.

## Completion

Reusable media behavior is delegated, public callers remain compatible, all
qualified outputs and error semantics are preserved, and the published backend
and package versions have recorded provenance. Signing, spanning cabinets,
versioned PE files and servicing features remain separate work.
