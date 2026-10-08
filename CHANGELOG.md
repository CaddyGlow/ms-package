# Changelog

## 0.2.1 — 2026-10-09

- Update the published `caddy-archive-core` dependency to 0.2.1.
- Add bounded APPX/MSIX Deflate output using `ms-compress`, with independently
  restartable block-map boundaries and compressed-source validation.
- Add caller-managed external cabinets, loose files and multiple nonspanning
  cabinets to the canonical MSI builder and payload editor.
- Add detailed MSI identity and media reports, transactional nested bundle edits,
  and stricter source, identity, resource and allocation checks.
- Expand native Microsoft-tool qualification and stateful authoring regression coverage.

## 0.2.0 — 2026-10-09

- Use published `caddy-msi` 0.10.1 for correct physical MSI primary-key serialization.

- Add an optional `write` feature for bounded unsigned stored APPX/MSIX creation,
  payload rebuilding, and narrowly supported bundles.
- Add MSI database creation and copy editing with typed schemas, rows, streams,
  summary properties, and explicit finalization.
- Add an experimental flat file-only embedded-cabinet MSI builder with explicit
  product, package, upgrade, and component identities.
- Preserve existing reader APIs, malformed-metadata regressions, and historical
  fixtures. Authoring qualification and remaining gates are recorded separately.

## 0.1.0

Initial preview release of the read-only `ms-package` library.

- Inspect APPX/MSIX manifests and architecture/resource bundles.
- Validate supported package block maps and nested package identity bindings.
- Read MSI tables and embedded cabinets; resolve external cabinets and loose files explicitly.
- Bound package storage, metadata streams and decoded payloads; reject inconsistent MSI string lengths before allocation.
- Provide native and browser regression coverage, malformed-input fixtures and an offline-buildable source bundle.

Integrity checks do not verify publisher signatures or trust. Encryption, upload
containers, sparse packages, transforms and patches are unsupported. Installation
and custom action execution are outside the library's scope.

This preview does not claim complete Microsoft-tool interoperability or a strict
process-wide heap limit. Real-package differential testing and sustained fuzzing
remain separate qualification gates; see `docs/release.md`.
