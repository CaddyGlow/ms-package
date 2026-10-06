//! Read-only portable APPX/MSIX and Windows Installer inspection.
//! Integrity validation is distinct from publisher signature or trust validation.
//!
//! # Example
//!
//! ```no_run
//! use ms_package::InstallerPackage;
//! use std::fs::File;
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let package = InstallerPackage::open_with_metadata_limit(
//!     File::open("example.msi")?, 10_000, 512 << 20, 16 << 20,
//! )?;
//! println!("{:?}", package.tables());
//! # Ok(())
//! # }
//! ```

/// Version of this library, as declared in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

mod appx;
mod installer;
mod installer_metadata;

pub use appx::{AppxBundle, AppxPackage, BlockMapFile, BundlePackage, PackageIntegrity};
pub use installer::{InstallerFile, InstallerPackage, MediaResolver, TableData};

/// Errors reported while interpreting package metadata or payloads.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An archive container operation failed.
    #[error("{0}")]
    Archive(#[from] archive_core::Error),
    /// A compound-storage operation failed.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// Metadata is invalid or inconsistent.
    #[error("malformed package: {0}")]
    Malformed(String),
    /// The package uses an unsupported profile.
    #[error("unsupported package profile: {0}")]
    Unsupported(String),
    /// Decoded bytes do not match package integrity metadata.
    #[error("package integrity failure: {0}")]
    Integrity(String),
    /// Explicitly supplied media is missing.
    #[error("missing package media: {0}")]
    MissingMedia(String),
    /// A caller's resource budget was exceeded.
    #[error("package resource limit exceeded: {0}")]
    Limit(&'static str),
}

/// Package operation result.
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn safe_name(name: &str) -> Result<String> {
    let name = name.replace('\\', "/");
    if name.starts_with('/')
        || name.contains(':')
        || name.split('/').any(|p| p == ".." || p.is_empty())
    {
        return Err(Error::Malformed(format!("invalid payload name {name:?}")));
    }
    Ok(name)
}
