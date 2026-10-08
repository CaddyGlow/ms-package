# Core 0.2.1 source preview — 2026-10-09

The self-contained source preview at `/data/cache/ms-package-authoring-core021-source-20261009.tar.gz` passed extracted manifest verification and fresh offline Cargo builds/tests with both a fresh Cargo home and a separate fresh target directory. All targets/features, default and all-feature doctests, no-default-features compilation, and browser harness all-target compilation passed. Raw output is retained in `verify.log`.

Archive SHA-256: `62b8d504c41d32e60d7fd8caeb1a33db6eaabbc22480eb1b2fd75d65ab74b45e`; size: 43474506 bytes. This is an explicit dirty-source preview, not a published release or a clean commit/tag claim. `SOURCE-MANIFEST.json` records every included file hash, source revision, dirty status, and staged integration adjustment. Subsequent qualification documentation receipts are outside this exact snapshot.

The six source checkouts were isolated under `/data/cache/ms-package-authoring-source-20261009`. Actual sibling repositories were not modified. Archive CLI, WASM, and fuzz dependencies were reconciled only inside the staging tree; both archive workspace and standalone fuzz locks were updated there before vendoring. The first rejected creation attempts exposed the standalone fuzz lock update requirement; logs retain that failure and the successful retry.

Reproduce verification with Rust 1.99.0 and native C/C++ tools:

```sh
python3 scripts/source-release.py --verify /data/cache/ms-package-authoring-core021-source-20261009.tar.gz
```
