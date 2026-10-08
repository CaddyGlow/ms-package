# Experimental authoring

Enable `ms-package`'s `write` feature and import `ms_package::authoring`.
The default feature set retains the inspection dependency graph. Published
dependencies continue to resolve from crates.io, including `caddy-msi = 0.10.1`.
These APIs do not sign packages, validate publisher trust, or execute custom actions.
Authoring XML uses `roxmltree` 0.21.1 and `woxml` 0.6.0 with
default features disabled. Both backends support `no_std` with allocation;
generated XML is written through a bounded sink. Package I/O still uses `std`.

`AppxBuilder::new(manifest, options)` accepts caller-authored manifest bytes.
Use `add_file(name, opened_reader)` to supply payloads, then `write(destination)`.
`AppxEditor::open(source, options)` verifies source integrity before accepting
add, replace, remove, rename, and explicit manifest replacement operations.
Payload-only edits preserve original manifest bytes and effective MIME mappings.
Renaming or removing a manifest-referenced file requires an explicit corresponding
manifest replacement. See [the creation/editing example](../examples/author_appx.rs).

The initial ZIP profile uses stored entries, fixed DOS timestamps, deterministic
ordering, UTF-8 filenames, and ZIP32 output. Bounded stored ZIP64 sources may
be decoded and rebuilt as ZIP32. ZIP64 output, compression, directory members,
unsupported reserved metadata, and ambiguous path structures fail explicitly.
Paths use forward slashes; rooted paths, traversal, Windows reserved characters,
reserved device names, case collisions and file/directory collisions are rejected.
Unicode names retain their bytes; lowercase collision checks are conservative
and do not establish complete Windows Unicode equivalence. Caller manifests must
meet the deployment schema: portable structural/reference checks are not full XSD
validation. Manifest extension bytes are retained when not explicitly replaced.

`AppxBundleBuilder` takes explicit bundle name, publisher, version and options.
Each nested package must be an unsigned stored package with verified integrity
and matching name/publisher. Architecture and resource identities cannot collide.
Bundle offsets and sizes describe emitted bytes. Known Language, Scale and
DXFeatureLevel resource qualifiers are transferred into bundle declarations;
unknown qualifiers fail explicitly. The editor accepts canonical manifests from
this writer so it cannot silently discard bundle extensions. Nested edits use
`AppxEditor` followed by explicit nested-package replacement in the bundle editor.

`InstallerDatabaseBuilder` exposes typed table, row, stream, code-page and summary
operations. `InstallerEditor` copies the input into private scratch storage and
preserves supported custom tables, summary properties and unknown streams.
Changes report their affected tables/streams. Schema types, keys, representability
and row/byte limits are checked. Declared single-column foreign keys are checked
before emission; installation semantics remain caller-owned. Explicit identity edits use rows and summary
properties; saving does not regenerate GUIDs. Signed databases fail under both
signature policies until MSI signature removal is audited. A failed backend
mutation poisons the writer, preventing accidental emission of partial changes.

`InstallerBuilder` coordinates a flat unversioned ASCII data-file profile, one
feature, one file per component, one stored embedded cabinet, and execute actions.
Choose architecture and per-user/per-machine context explicitly. Supply distinct
canonical product/package/upgrade GUIDs and stable component GUIDs. PE payloads
are rejected until portable version/language inspection is qualified. Database
validity and payload extraction do not establish installation or servicing;
consult the [qualification record](package-authoring-validation.md) before use.
`InstallerPayloadEditor` admits only canonical builder output: regeneration and
comparison of every CFB stream rejects custom data it cannot preserve. Payload
replacement retains component identity and key paths; renaming requires a new
component GUID. Every payload edit requires an explicit distinct PackageCode.

All writers use caller-owned destinations and optional bounded memory, with no
implicit filesystem scratch. APPX output requires `Write + Seek`; MSI final
destinations require `Write`. Generic destinations must be empty/distinct and
their publication is caller-owned. Finalization and flush errors propagate; a
destination error can leave partial bytes. Reports are returned only on success.
The API's sequential edits determine the final plan before output begins and
reject missing sources or colliding targets.

`WriteLimits` bounds entries, metadata, per-file and aggregate decoded bytes,
output, logical scratch and MSI rows. The implementations account for retained
payload and container buffers; allocator capacity, object overhead, ZIP directory
and MSI/CFB parser allocations are not a strict process-wide heap budget. Opening
hostile databases uses bounded storage and encoded-metadata preflight before MSI
parsing. Sources are checked against actual consumed bytes. Read/write failures
never produce a successful report. APPX signed inputs reject by default;
explicit `SignaturePolicy::Remove` removes the supported signature footprint
and rebuilds content types, reporting unsigned output. Other signed structures
still fail closed.

Determinism applies to identical content/options/identifiers/backend versions.
MSI identities are caller-controlled: changed distributables generally need a
new PackageCode. No upgrade behavior is implied by preserving an UpgradeCode.

Native and Worker checks, independent tool observations and outstanding release
gates are tracked in [authoring validation](package-authoring-validation.md).
The [implementation plan](package-authoring-plan.md) remains the broader roadmap;
unsupported profiles cannot be inferred from its milestone list.
