# Repository guidelines

This repository owns the ms-package crate and uses the commit-pinned
CaddyGlow/rust-msi fork for the patched MSI reader.
Preserve public APIs, crate names, licensing notices, fixtures and historical evidence.
Published dependencies resolve from crates.io.
Use rustfmt defaults. Validate with cargo test --all-targets --all-features --locked and cargo test --doc --locked and
cargo clippy --all-targets --all-features --locked -- -D warnings.
Browser integration is validated by archive-rs Worker checks.
