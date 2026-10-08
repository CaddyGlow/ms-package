//! Experimental portable package authoring.
//!
//! Writers finalize before returning a report. Destination errors can leave
//! partial output; callers control publication and must use a distinct destination.
//! Format validation does not establish installation, signatures, or trust.

mod appx;
mod bundle;
mod installer;
mod installer_builder;
mod xml;

pub use appx::{AppxBuilder, AppxEditor};
pub use bundle::{AppxBundleBuilder, AppxBundleEditor};
pub use installer::{InstallerDatabaseBuilder, InstallerEditor, InstallerWriteReport};
pub use installer_builder::{
    InstallationContext, InstallerArchitecture, InstallerBuilder, InstallerIdentity,
    InstallerPayloadEditor,
};

/// Authoring failures, separate from the existing reader error contract.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WriteError {
    /// Source, destination, or finalization I/O failed.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// ZIP serialization failed.
    #[error("{0}")]
    Zip(#[from] zip::result::ZipError),
    /// Existing content failed reader validation.
    #[error("{0}")]
    Reader(#[from] crate::Error),
    /// Metadata or an edit operation is inconsistent.
    #[error("invalid authoring input: {0}")]
    InvalidInput(String),
    /// The structure cannot be preserved by this profile.
    #[error("unsupported authoring profile: {0}")]
    Unsupported(String),
    /// An explicit resource bound was exceeded.
    #[error("authoring resource limit exceeded: {0}")]
    LimitExceeded(&'static str),
}

/// Authoring operation result.
pub type WriteResult<T> = std::result::Result<T, WriteError>;

/// Policy for signatures on edited inputs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SignaturePolicy {
    /// Reject signed inputs (the default).
    #[default]
    Reject,
    /// Remove audited signature metadata while rebuilding unsigned output.
    /// Formats without an audited removal path still reject signed inputs.
    Remove,
}

/// Resource limits. These bound logical data, not process-wide allocations.
#[derive(Clone, Copy, Debug)]
pub struct WriteLimits {
    /// Maximum archive members or database objects.
    pub max_entries: u64,
    /// Maximum individual generated or supplied metadata size.
    pub max_metadata_bytes: u64,
    /// Maximum decoded bytes in one payload.
    pub max_file_bytes: u64,
    /// Maximum aggregate decoded payload bytes.
    pub max_total_bytes: u64,
    /// Maximum completed container size.
    pub max_output_bytes: u64,
    /// Maximum logical in-memory scratch bytes.
    pub max_scratch_bytes: u64,
    /// Maximum aggregate MSI table rows.
    pub max_rows: u64,
}

impl Default for WriteLimits {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_metadata_bytes: 16 << 20,
            max_file_bytes: 256 << 20,
            max_total_bytes: 512 << 20,
            max_output_bytes: 640 << 20,
            max_scratch_bytes: 640 << 20,
            max_rows: 100_000,
        }
    }
}

/// Common resource and signature choices; output uses deterministic stored ZIPs.
#[derive(Clone, Copy, Debug, Default)]
pub struct WriteOptions {
    /// Logical resource budgets.
    pub limits: WriteLimits,
    /// Signed-input handling.
    pub signature_policy: SignaturePolicy,
}

/// Completed output accounting, without an installation or trust claim.
#[derive(Clone, Debug, Default)]
pub struct WriteReport {
    /// Number of emitted entries or database objects.
    pub entries: u64,
    /// Decoded source bytes represented in output.
    pub decoded_bytes: u64,
    /// Completed destination bytes.
    pub output_bytes: u64,
    /// Whether input signatures were deliberately removed.
    pub signature_removed: bool,
}
