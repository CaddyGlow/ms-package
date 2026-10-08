# Package creation and editing plan

Status: initial profiles and follow-up compression/media profiles implemented,
2026-10-09. See [follow-up qualification](package-authoring-validation-20261009.md).

Implementation work is recorded in [the backend audit](package-authoring-audit.md),
[API/profile documentation](package-authoring.md) and
[qualification evidence](package-authoring-validation.md). Stored package and
bundle authoring, database editing and an experimental minimal MSI profile have
been implemented. Representative stored packages and bundles passed Microsoft
format tooling; the x64 per-user MSI profile passed installation, repair and
uninstall with the qualified `caddy-msi` 0.10.1 backend. Qualification boundaries
and the exact dependency/source checks are tracked separately.

Extend `ms-package` with portable creation and editing of APPX/MSIX packages,
bundles, and MSI packages while preserving its existing inspection APIs. Deliver
single-package APPX/MSIX authoring first, then bundles, MSI database editing, and
a qualified minimal MSI installer profile. Changes to the owned `caddy-msi` fork
are part of the implementation scope whenever its existing primitives are
insufficient.

## Current foundations

- `src/appx.rs` implements package and bundle inspection, manifest parsing,
  payload extraction, identity binding, and block-map integrity validation.
- `src/installer.rs` implements MSI inspection and explicit media resolution.
  `src/installer_metadata.rs` provides bounded metadata preflight checks.
- `examples/browser_fixtures.rs` already creates synthetic ZIP packages,
  bundles, MSI databases, and cabinets using `zip`, `caddy-msi`, and `ms-cabinet`.
  These are fixture producers, not qualified public authoring APIs.
- `caddy-msi = 0.10.1` is the published fork dependency, imported as `msi`.
  Its provenance and parser regressions are documented in [msi-fork.md](msi-fork.md).
- Existing tests and release evidence establish inspection behavior for specific
  fixtures. They do not establish installation correctness for newly authored
  packages; retain those qualification boundaries.

## Scope and delivery order

| Milestone | Deliverable | Qualification gate |
| --- | --- | --- |
| M0 | API contracts, profile matrix, backend audit | Decisions and backend gaps recorded |
| M1 | Unsigned single-package APPX/MSIX creation | Independent format validation |
| M2 | APPX/MSIX rebuilding editor | Edit and preservation regressions |
| M3 | Bundle creation and editing | Nested identity and integrity checks |
| M4 | MSI database primitives and editor | Fork tests and independent database comparison |
| M5 | Minimal installable MSI creation and payload edits | Windows validation and lifecycle tests |
| M6 | Release qualification and documentation | Native, WASM, Worker, and source-release gates |

M1 and M2 form the first public authoring milestone. The MSI backend audit can
proceed independently after M0, but MSI support must ship according to its own
qualification gates. Expand compression, external media, and broader installer
profiles incrementally rather than treating all formats as one release gate.

Initial outputs are unsigned. Cryptographic signing and trust verification,
MSI transforms and patches, encrypted and sparse APPX profiles, upload
containers, and arbitrary installer authoring remain subsequent projects.
Existing custom actions may be preserved by the database editor where supported;
the portable library must never execute them.

## Repository ownership

| Repository or crate | Responsibility |
| --- | --- |
| `ms-package` | Package authoring APIs, profiles, edit planning, resource limits, and coordinated metadata and payload updates |
| `caddy-msi` | MSI database schemas, tables, rows, streams, string pools, code pages, summary information, and serialization correctness |
| `ms-cabinet` | Cabinet encoding and any necessary cabinet writer fixes |
| `archive-rs` | Archive integration, WASM bindings, and browser Worker validation |

Fix MSI database correctness in the fork instead of duplicating its internals in
`ms-package`. Preserve public APIs, crate names, licensing notices, parser
regressions, fixtures, and historical evidence in both repositories.

## API and execution contracts

### Separate reading from writing

Keep `AppxPackage`, `AppxBundle`, and `InstallerPackage` compatible. Add dedicated
authoring types with provisional names:

- `AppxBuilder` and `AppxEditor`.
- `AppxBundleBuilder` and `AppxBundleEditor`.
- `InstallerDatabaseBuilder` and `InstallerEditor` for database operations.
- `InstallerBuilder` for the qualified installation profile.
- `WriteOptions`, `WriteLimits`, `WriteReport`, and `WriteError` where genuinely
  shared across formats; keep format-specific choices on their own options.

Finalize signatures in M0 after backend experiments. Prefer standard Rust I/O
traits and concrete types; avoid introducing a universal editable-package model
that hides MSI tables or APPX manifest semantics. A separate writer error type
avoids breaking exhaustive matches on the existing public `Error` enum.

Evaluate an additive `write` Cargo feature, disabled by default, to keep reader
users free of optional encoding dependencies. Confirm the feature layout against
the actual dependency graph before committing to it. Test both the existing
reader configuration and the authoring configuration.

### Create and edit lifecycle

1. Supply metadata and explicitly opened payload sources.
2. Build an edit or creation plan and check names, profiles, references, and limits.
3. Emit a new package into a caller-supplied destination.
4. Explicitly finalize all archive, cabinet, and database writers.
5. Return a report only after successful finalization.

Support payload add, replace, remove, and rename operations. Separate raw MSI
table edits from higher-level payload operations so callers can understand which
invariants each operation maintains. Define conflicts such as replacing a removed
entry or renaming onto an existing entry before output begins.

The initial editor rebuilds or copies into a distinct destination; it never
modifies the source in place. No-op editing promises semantic preservation of
supported content, not byte-identical container layout. Reject unsupported
structures that cannot be preserved rather than silently discarding them.

### I/O and limits

- Determine required `Read`, `Write`, and `Seek` bounds per backend. Seekable
  output is acceptable initially; do not promise forward-only writing before
  proving it for each format.
- Stream payloads where possible. Where multiple passes are necessary, use
  explicitly replayable sources or caller-provided scratch storage.
- Keep temporary filesystem access optional. Browser callers must be able to
  use bounded memory or a supplied storage adapter.
- Bound entry counts, metadata, per-file and aggregate decoded bytes, output
  bytes, scratch usage, and MSI row counts. Apply checked arithmetic throughout.
- Enforce actual bytes consumed, not just declared sizes. Document the limits
  that cannot bound backend allocations during opening or serialization.
- Propagate read, write, flush, and finalization failures. A failed write may
  leave partial destination data; generic stream output is not atomic.
- Any future filesystem convenience API should write to a temporary sibling and
  replace the target only on success, with platform-specific guarantees stated.

### Signatures and reproducibility

Default to rejecting modification of signed inputs. An explicit unsigned-rebuild
policy may remove signatures and associated metadata after format-specific
auditing. Never retain a stale signature while reporting a successfully signed
output. Include the resulting signature state in `WriteReport`.

Allow callers to fix timestamps, ordering, compression settings, and generated
identifiers. Deterministic output applies to identical content, options,
identifiers, and backend versions. For MSI, reproducibility must not silently
reuse identity values where a changed distributable requires a new package code.

## M0 Backend audit and design

1. Inventory existing ZIP, cabinet, and MSI writer APIs and their feature flags,
   target support, memory behavior, seek requirements, and finalization errors.
2. Trace fixture generation and identify assumptions that production writers
   cannot inherit, including simplified manifests and incomplete installer tables.
3. Audit `caddy-msi` create/open-for-write, row insert/update/delete, schema
   mutation, stream replacement/removal, summary mutation, and explicit flush APIs.
4. Build a preservation matrix for custom tables, unknown streams, code pages,
   summary properties, XML extensions, and signed package metadata.
5. Specify supported input/output profiles, operations, limits, and failure
   behavior. Unknown signatures or unsupported storage profiles must fail closed.
6. Select the ZIP writer based on measured APPX requirements. Promote the current
   test dependency only if it meets those requirements; do not duplicate an
   appropriate archive backend without need.
7. Review public API sketches with native and WASM call sites before implementation.

Deliverables: API sketches, backend capability matrix, concrete fork tasks, and
small compatibility experiments. Exit when each promised operation has a known
backend path or an explicitly scoped implementation task.

## M1 Single package APPX and MSIX creation

Start with unsigned packages containing stored entries. Accept caller-authored
manifest XML and payload sources; provide a typed manifest authoring layer later
if a clear supported schema subset justifies it.

Implementation work:

1. Add writer-specific package path validation, including traversal, rooted
   paths, separators, reserved metadata names, Windows path rules, and collisions.
   Establish format-specific case and Unicode behavior through authoritative
   requirements and compatibility tests; do not blindly normalize names.
2. Validate manifest structure and relevant references for the supported profile.
   Existing reader checks alone do not establish full schema validity.
3. Generate `[Content_Types].xml` and `AppxBlockMap.xml`, with required coverage
   and exclusions defined by the format.
4. Compute SHA-256 over decoded 64-KiB blocks while consuming payloads. Handle
   empty files, exact boundaries, partial final blocks, and large sizes.
5. Derive local file header sizes from the actual ZIP representation, including
   extra fields. Audit ZIP64, data descriptors, entry ordering, and filename
   encoding instead of assuming a generic ZIP is a valid APPX container.
6. Reopen generated output using the current reader and compare metadata and
   payload hashes against expected values.
7. Validate independently with Microsoft packaging tooling. Retain representative
   outputs and diagnostics as evidence.

Add Deflate only after stored output passes the gate. Prove the block flushing
and compressed-size accounting required by the package format; test against
independent tools so a shared reader/writer mistake cannot pass unnoticed.

Exit: documented stored-entry profile, public creation example, boundary and
failure tests, and independent validation. Compressed output has its own gate.

## M2 Single package editing

1. Open and preflight the source under explicit limits. Require integrity
   validation before preserving existing payloads, performed during a preliminary
   pass or as data is copied; document when an error can leave partial output.
2. Build an immutable operation plan resolving entry identities and conflicts.
3. Preserve unchanged manifest bytes and unrelated supported content. Metadata
   edits must retain supported extension elements and attributes or reject the
   edit when preservation is impossible.
4. Apply add, replace, remove, rename, and explicit manifest replacement operations.
5. Rebuild content types and integrity metadata through the M1 writer. Initially
   decode and re-encode payloads; defer raw compressed copying until proven safe.
6. Apply the explicit signed-input policy and report changes and output state.

Exit: create-edit-reopen tests cover each operation, combinations, unchanged
content, bad source integrity, extension preservation, signed-input handling,
resource exhaustion, and destination failures.

## M3 Bundle creation and editing

1. Accept explicitly supplied nested packages and resource packages.
2. Validate each nested package and bind its identity, version, architecture,
   publisher, and resource identity to the bundle declaration.
3. Generate the bundle manifest, content types, and outer block map according to
   bundle-specific rules. Derive recorded sizes and other required location data
   from emitted bytes.
4. Support adding, replacing, and removing nested packages. Editing a nested
   package invokes its own editor and then rebuilds the containing bundle.
5. Preserve supported bundle metadata and reject inconsistent declarations,
   duplicate identities, and unsupported nested profiles.
6. Budget nested processing and aggregate decoded bytes explicitly. Outer
   validation must not substitute for nested package validation.

Exit: architecture and resource fixtures pass existing selection checks and
independent tooling; edits update both nested and outer integrity metadata.

## M4 MSI database creation and editing

### Work in caddy-msi

Use audit findings to add or repair the minimum necessary primitives:

- Create, drop, and inspect tables with faithful column and validation metadata.
- Insert, update, and delete typed rows, with clear key and schema errors.
- Create, replace, read, and remove streams without truncation or stale tails.
- Read and update summary information while preserving untouched properties.
- Correctly serialize string pools, reference counts, code pages, nulls, integer
  encodings, and long names across mutation and reopening.
- Preserve custom schemas and unknown streams for the supported database profile.
- Report finalization errors explicitly rather than relying on destructor work.

Treat these as audit targets, not claims that the current fork lacks every API.
Keep changes additive where possible and isolate backend bugs with direct fork
regressions. Run retained malformed-metadata parser regressions after writer
changes. Check the fork workspace and FFI compatibility where affected.

### Work in ms-package

1. Wrap database operations with bounded opening, supported-profile checks, and
   an explicit edit plan.
2. Edit a copy in a separate seekable destination or caller-provided scratch
   storage. Choose copying versus logical reconstruction based on preservation
   evidence from the fork audit.
3. Validate row types, primary keys, applicable foreign keys, and known package
   relationships. Distinguish database validity from installability.
4. Expose deliberate policies for PackageCode, ProductCode, UpgradeCode, product
   version, and component identity. Do not regenerate all GUIDs on every save.
5. Audit MSI signature streams and signature-related tables before enabling
   unsigned rebuilding of signed databases or signed external media.
6. Produce a report naming changed tables, streams, identities, and validation
   scope. A database-only write must not claim a valid installable product.

Exit: new and edited databases round-trip through the fork and independent
database tooling, preserving custom data for the declared profile.

## M5 MSI payloads and minimal installable packages

Begin with a narrow file-only installer profile: explicit product identity,
architecture and installation context, a documented directory tree, features and
components, and one embedded cabinet. Exclude custom-action authoring from this
initial high-level builder.

1. Define complete table and summary requirements for this profile, including
   Property, Directory, Component, Feature, FeatureComponents, File, Media,
   applicable sequencing, validation metadata, and necessary standard actions.
2. Establish component GUID and key-path rules. Reject edits that would violate
   supported servicing semantics instead of silently changing component identity.
3. Build cabinet members and coordinate file identifiers, sequence numbers,
   compression flags, cabinet references, and Media.LastSequence boundaries.
4. Populate file sizes, version/language data, and hashes where required. Use
   explicit caller metadata or a qualified portable inspection path; do not
   fabricate version information for versioned binaries.
5. Implement replace, add, remove, and rename operations that update all affected
   metadata and rebuild the required cabinet. Audit known dependencies such as
   shortcuts, key paths, and file references; reject unsupported reference edits.
6. Add external cabinet output and loose-file media only after embedded payloads
   pass. Use explicit input resolvers and output sinks; define partial-failure
   behavior for a package plus multiple media artifacts.
7. Extend to multiple cabinets with validated ordering and sequence boundaries.
   Spanning cabinets and other advanced layouts require separate qualification.

Exit: independent extraction and payload hashes match; Windows Installer
validation passes; installation, repair, and uninstall succeed in disposable
Windows test environments. Any claimed upgrade behavior requires its own
upgrade tests. Preserve tool versions, logs, fixture hashes, and failures.

## Validation strategy

### Automated library coverage

- Public API creation/editing examples and doctests.
- Semantic preservation of untouched metadata, custom tables, and streams.
- Empty files, 64-KiB boundaries, Unicode names, large declared sizes, and the
  chosen ZIP64 profile; test code-page representability for MSI strings.
- Duplicate names, invalid references, malformed metadata, broken source
  integrity, unsupported profiles, and signed-input policy.
- Short reads, errors after partial writes, flush/finalization failures, and
  exact resource-budget boundaries.
- Deterministic builds with explicit timestamps and identities.
- Property tests or focused fuzz targets for operation sequences and writer
  reopening, alongside retained parser fuzz cases.

Do not replace historical fixtures with newly generated output. Add authoring
fixtures separately so older regressions retain their original evidence.

### Independent compatibility coverage

Compare authored outputs with Microsoft packaging and Installer tools and, where
useful, `msitools`/`msiextract`. Compare normalized metadata and decoded payloads
when byte identity is not meaningful. Keep structural validation, cryptographic
signature validation, installation, and trust as distinct recorded results.

For APPX deployment checks that require signing, sign test artifacts externally
with test credentials inside the test environment. This does not add signing
or trust claims to the portable authoring API.

### Required repository gates

Run in the repository's supported development environment:

```sh
cargo fmt --all -- --check
cargo test --all-targets --all-features --locked
cargo test --doc --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
```

Also verify the supported minimum Rust version, reader-only builds, authoring
builds for `wasm32-unknown-unknown`, relevant fork workspace tests, and Windows
tests. Integrate browser authoring through archive-rs Worker checks, covering
native/browser output parity, reopening, payload hashes, limit failures, and
cancellation behavior supported by the binding. A WASM compile alone is not
browser qualification.

## Proposed code organization

Keep existing reader modules intact unless a narrowly scoped shared helper is
justified. Add an `authoring` module with format-specific implementations, shared
limits/reporting only where appropriate, and internal path/metadata validation.

Add public integration tests under `tests/`, examples for creation and editing,
and separate authoring fixtures with provenance in `tests/fixtures/README.md`.
Place database primitive regressions in `caddy-msi`; package coordination tests
belong in `ms-package`; Worker integration tests belong in archive-rs.

Avoid moving existing reader code as part of the first authoring commit. Any
shared validation refactor should preserve existing reader behavior and have
independent regression coverage before writer logic depends on it.

## Reviewable implementation batches

1. API contracts, backend audit, profile matrix, and compatibility experiments.
2. Shared writer limits/errors and stored APPX creation.
3. Independent APPX qualification and optional compressed output.
4. APPX editing and signature policy.
5. Bundle creation and editing.
6. Focused `caddy-msi` primitive fixes with direct regressions.
7. MSI database builder/editor and preservation tests.
8. Minimal embedded-cabinet installer builder and Windows qualification.
9. Coordinated MSI payload editing, then external and multiple media profiles.
10. Browser integration, complete release evidence, and authoring documentation.

Each batch should include its relevant tests and documentation. Do not defer
interoperability experiments until the final integration batch.

## Dependency and release process

During development, use a temporary local Cargo override to test fork changes
together. Keep local path overrides out of the published dependency contract.
Before consuming a new fork release, run both its direct regressions and the
`ms-package` compatibility suite against that exact source.

Publish the required `caddy-msi` version first, then update the exact dependency
pin and Cargo lockfiles in `ms-package`. Apply the same order to other changed
backends. Preserve crates.io resolution for published dependencies and update
fork provenance and source-release dependency records.

Update README support claims, crate documentation, examples, changelog, and release
documentation only for qualified capabilities. Preserve historical release
evidence rather than rewriting older inspection-only records. Verify source
bundling and a fresh offline extracted build with the new dependencies.

## Risks and decisions to settle

| Risk or decision | Required resolution |
| --- | --- |
| Generic ZIP output differs from valid APPX layout | Validate stored output independently before adding compression |
| Rebuilding loses metadata or custom schemas | Preservation matrix and rejection of unsupported edits |
| MSI database validity is mistaken for installability | Separate APIs, reports, and Windows lifecycle gates |
| Payload edits break MSI servicing rules | Narrow initial profile and explicit identity/key-path policies |
| Rebuilds require excessive memory | Streaming, explicit scratch storage, and measured backend limits |
| Signature data survives content changes | Reject by default; audited explicit unsigned rebuild |
| Multiple external media outputs fail partway | Document per-artifact completion and caller-managed publication |
| Optional writers increase reader dependency cost | Audit feature layout and retain reader-only build checks |

M0 must settle feature names, concrete I/O bounds, scratch storage ownership,
minimum supported profiles, and validation/report semantics. Later milestones
may expand those profiles only with matching independent evidence.

## Completion criteria

A supported authoring profile is complete when its public API is documented,
creation and editing maintain its declared invariants, unsupported inputs fail
explicitly, and independent tools accept its outputs. Installable MSI claims
additionally require Windows lifecycle evidence. All required repository and
browser gates must pass for the profiles exposed in that release.

## References

- [Existing repository scope](../README.md).
- [MSI fork provenance](msi-fork.md).
- [Release requirements](release.md).
- [Recorded release validation](release-validation.md).
- [Microsoft APPX block-map file schema](https://learn.microsoft.com/en-us/uwp/schemas/blockmapschema/element-file).
- [Microsoft Windows Installer package validation](https://learn.microsoft.com/en-us/windows/win32/msi/package-validation).

Consult current authoritative format and identity rules during each backend
audit and record the specific revisions used in its compatibility evidence.
