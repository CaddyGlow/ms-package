# Package authoring backend audit

This initial audit is retained as historical evidence. The follow-up uses
`caddy-archive-core` 0.2.1, `ms-compress` 0.1.2 raw Deflate, and caller-managed
stored MSI media. Current contracts are in [the API documentation](package-authoring.md),
with [separate follow-up qualification](package-authoring-validation-20261009.md).

Implementation snapshot: 2026-10-08. This records the experimental `write`
feature introduced for [the authoring plan](package-authoring-plan.md); it does
not establish Microsoft deployment or Windows Installer qualification.

## API and execution decisions

The additive `write` feature is disabled by default. Existing reader APIs and
their error enum remain unchanged. Authoring has a separate `WriteError` and
explicit `WriteOptions`, `WriteLimits`, and completed `WriteReport`.

APPX and bundle destinations implement `Write + Seek`; MSI database destinations
implement `Write`. Payload sources implement `Read`. Editors consume a source
into bounded private memory and emit a distinct destination. No temporary
filesystem, automatic GUID generation, signature trust verification, or custom
action execution is involved. A destination write or flush failure can leave
partial bytes; publication and atomic replacement belong to the caller.

The current scratch adapter is bounded memory. Counts describe logical data,
not process-wide allocator usage: ZIP/MSI opening can allocate parser metadata,
and MSI serialization and compound-file bookkeeping allocate internal buffers.
Callers cannot assume these options constitute an operating-system memory cap.

## Backend capabilities and selected paths

| Backend | Existing capability used | Constraints and audit boundary |
| --- | --- | --- |
| `zip` 2.4, encoding enabled by `write` | Seekable stored ZIP serialization, explicit `finish`, deterministic default DOS timestamp | ZIP32 only; no Deflate, ZIP64, data descriptors, encryption, or signing in the authored profile. Local headers have 30 fixed bytes plus UTF-8 filename bytes. |
| `caddy-archive-core` 0.1.0 | Bounded ZIP opening, member decoding, metadata inspection | Reader limits cannot bound every opening allocation. Existing package integrity checks remain independent of signing/trust. |
| `roxmltree` 0.21.1 and `woxml` 0.6.0, default features disabled | Authoring DOM validation and bounded generated metadata via alloc-compatible no_std XML backends | Package I/O remains std; this does not make the crate no_std. Package reading retains quick-xml. Focused authoring checks are not complete Windows manifest schema validation. |
| SHA-256 and Base64 | Decoded 64-KiB APPX block hashes | Stored output records no compressed block lengths. Empty files have no blocks. |
| `caddy-msi` =0.10.1 | Create/open, create/drop tables, typed insert/update/delete, stream replace/remove, summary/code-page editing, explicit flush/finalization | Existing primitives suffice for the experimental database API. Full serialization preservation and independent comparison remain qualification work; no local path override enters published dependencies. |
| `cfb` 0.14 | MSI compound-file signature-stream preflight and copied-container preservation | Copying preserves untouched compound content but does not prove installability or all custom-schema serialization behavior. |
| `ms-cabinet` 0.1.2 | One embedded stored cabinet, explicit `write` and flush | Borrowed payloads and one 32-KiB framing buffer; unsupported external/spanning media, versioned binaries and custom-action authoring remain excluded. |

Fork provenance and retained malformed-input evidence remain in
[msi-fork.md](msi-fork.md). No authoring fixture replaces historical reader
fixtures.

## Experimental profile and preservation matrix

| Format/operation | Implemented profile | Explicit exclusions or preservation boundary |
| --- | --- | --- |
| Single APPX/MSIX creation | Caller-authored manifest, stored file entries, generated content types and block map, deterministic member order | Focused path/reference/integrity validation; no claim of full manifest-schema or deployment correctness. |
| Single APPX/MSIX editing | Add/replace/remove/rename files; explicit manifest replacement; unchanged manifest bytes retained | Unsupported entries fail closed. Signed inputs rejected by default; only the audited APPX removal policy may rebuild unsigned. |
| Bundle creation | Root `.appx`/`.msix` members, matching Name/Publisher, checked version/architecture/resource identity, nested integrity verification, generated outer manifest and metadata | Stored unsigned nested packages only. Duplicate architecture/resource identities rejected. Known Language/Scale/DXFeatureLevel qualifiers are transferred to bundle declarations; unknown qualifiers fail closed. Language tags use a conservative BCP-47 subset (no extensions or private-use tags). Resource package type follows `Properties/ResourcePackage`, not the presence of `ResourceId`. |
| Bundle editing | Add/replace/remove nested packages; rebuild outer metadata; nested packages may first be edited using `AppxEditor` | Only this writer's canonical bundle manifests are accepted, so extension-bearing or differing manifests fail closed. Signed bundles and signed nested packages are rejected even under removal policy. |
| MSI database creation/editing | Typed raw database operations, separate scratch copy, explicit summary/code-page changes, changed table/stream reporting | No automatic identity changes or installability claim. Declared single-column foreign keys from `_Validation` are checked before emission; conditional and implicit installer references still require caller analysis. Transforms, patches and signature metadata are rejected. Failed database mutations poison the builder instead of emitting partially applied changes. |
| File-only MSI coordination | Experimental `InstallerBuilder`: explicit product/package/upgrade/component GUIDs, x86/x64, per-user/per-machine, flat directory, one feature, one file per component, embedded stored cabinet, standard execute actions | Data-only ASCII leaf names; PE/versioned payloads rejected; no authored custom actions, UI, upgrade promise, external media or spanning cabinets. Native lifecycle qualified for representative x64 per-user creation and canonical editing; other configurations remain experimental. |
| MSI payload rebuilding | `InstallerPayloadEditor` admits only canonical writer output through full CFB stream comparison; add/replace/remove/rename reconstruct related tables and cabinet | Explicit new PackageCode is mandatory. Replacement preserves the component GUID/key path; rename requires a new component GUID. Custom schema/summary/unknown stream/reference deviations are rejected before rebuilding. |

Bundle member `Offset` values describe actual ZIP payload locations. Nested
packages are emitted first in filename order, making these locations independent
of generated XML lengths. The outer block map covers bundle metadata; each
nested package has its own separately validated block map. A successful outer
check alone is never treated as nested integrity validation.

Bundle creation budgets nested container bytes and aggregate decoded package
bytes. It reserves memory for staged serialization and checks the completed
bundle, selects every nested package, and validates each before copying bytes to
the destination. Bundle editor opening additionally reserves memory for the
source copy. These conservative bounds may reject a workload that a streaming
implementation could eventually accommodate.

## Qualification scope

The native authoring regressions exercise creation/editing, actual ZIP offsets,
nested identity and integrity rejection, resource type declarations, and resource
limits. Representative stored packages and bundles have also passed independent
MakeAppx unpacking/repacking checks; see the [qualification record](package-authoring-validation.md).
These results do not imply complete manifest-schema or deployment validation.

The following are separate gates, with executed results recorded in the
[qualification record](package-authoring-validation.md), for broader support claims:

- Microsoft MakeAppx validation of representative stored package and bundle
  outputs, retaining tool versions, diagnostics and output hashes.
- Independent MSI database comparison and exact custom schema, summary property,
  stream, code-page, and string-pool preservation experiments; direct fork
  regressions if a serialization defect is found.
- Windows Installer lifecycle tests before exposing an installable MSI builder.
- Native minimum-version, reader-only, doctest, Clippy, WASM, and extracted
  source-release checks for the exact dependency set.
- `archive-rs` Worker authoring integration, including native/browser parity,
  reopening, payload hashes, limits and supported cancellation. A WASM compile
  does not satisfy this browser gate.

Authoritative bundle package attribute reference reviewed for this implementation:
[Microsoft bundle Package schema](https://learn.microsoft.com/en-us/uwp/schemas/bundlemanifestschema/element-package),
including required `Offset` and `Size` attributes. The stored bundle subset is
intentionally narrower than that schema.

Resource qualifier names and scale/DirectX values follow
[Microsoft package Resource schema](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-f-resource)
and are transferred to the corresponding bundle declarations. Unsupported
language-tag extensions, private-use tags and generated placeholders fail
explicitly.

## MSI serialization finding and published backend fix

During 2026-10-08 qualification, the `caddy-msi` 0.10.0 writer emitted an MSI
that native Windows Installer rejected with error 1620 before installing files.
Changing code pages to Windows-1252 did not resolve the failure. A version-3 CFB
storage experiment also failed; native-created MSI uses version 4, disproving
version 4 as the sole cause. Removing summary information or transplanting the
streams into a native container still failed. The [rejected V3 patch](caddy-msi-v3-authoring.patch)
is retained only as diagnostic history.

The minimum failure was isolated without installer actions or payloads: a fresh
`Custom` table failed native opening, while the identical `zCustom` schema opened.
Both used fixed-length strings. The backend ordered rows by decoded string keys;
native MSI catalogs require numeric string-reference ordering. The
[physical-key-order patch](caddy-msi-physical-key-order.patch) sorts serialized
primary keys across all tables and preserves decoded logical ordering for public
Select results. Direct regressions inspect catalog keys, ordinary string
references, numeric secondary keys and logical ordering after reopening.

The corrected `caddy-msi` 0.10.1 backend was qualified and published to crates.io
after explicit authorization. Its matching source was subsequently pushed on
`ms-package-0.10.1` with annotated tag `caddy-msi-v0.10.1`; the registry archive
predates the Git commit. The main dependency now pins `=0.10.1`, with the
verified registry package checksum and source provenance recorded in
[caddy-msi-authoring-candidate.md](caddy-msi-authoring-candidate.md). No local
path override enters the published dependency contract. Historical 0.10.0
source, licensing and malformed-input evidence remain intact.

Native tests of the explicit x64, per-user, single unversioned data-file profile
passed installation, deletion/repair and uninstall with exact payload hashes and
subsequent removal. A canonical payload edit exercising replacement, rename, add
and remove operations under a new PackageCode passed the same lifecycle. The
[original result](evidence/package-authoring-20261008/result.json) and
[edited result](evidence/package-authoring-20261008/edited/result.json) retain
those checks. These results do not qualify x86, per-machine, upgrades, versioned
binaries or arbitrary database profiles.

The file-only builder uses Windows-1252 and rejects unrepresentable metadata.
It does not populate optional `MsiFileHash` rows or fabricate file versions and
language information. Raw database edits support typed schemas, nulls, primary
keys, declared single-column foreign keys, summary updates, stream truncation
and removal, bounded preflight and explicit finalization. Broader implicit or
conditional references and servicing rules remain outside this narrow profile.

Schema/action references consulted on 2026-10-08:
[InstallExecuteSequence](https://learn.microsoft.com/en-us/windows/win32/msi/installexecutesequence-table),
[compressed source flags](https://learn.microsoft.com/en-us/windows/win32/msi/word-count-summary),
and [MsiFileHash](https://learn.microsoft.com/en-us/windows/win32/msi/msifilehash-table).
