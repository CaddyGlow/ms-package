# Changelog

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
