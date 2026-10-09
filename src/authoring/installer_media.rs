//! Explicit, bounded nonspanning MSI media preparation and caller-owned publication.
use super::{WriteError, WriteLimits};
use std::{
    collections::BTreeSet,
    io::{self, Write},
};

/// A contiguous group of file sequences stored in one independent cabinet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallerCabinetSpec {
    /// Safe ASCII cabinet leaf name, without the embedded-stream `#` marker.
    pub name: String,
    /// Number of successive files assigned to this cabinet; must be nonzero.
    pub file_count: u64,
    /// Embed the cabinet in the MSI; otherwise publish it through the media sink.
    pub embedded: bool,
}

/// Source media for the flat, unversioned file-only MSI profile.
/// Cabinet groups follow file insertion order and never span cabinets.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum InstallerMediaLayout {
    /// One embedded stored `payload.cab` (the existing default).
    #[default]
    Embedded,
    /// One external stored cabinet, resolved by its explicit name when reading.
    ExternalCabinet {
        /// Safe ASCII `.cab` leaf name.
        name: String,
    },
    /// Loose source files under `directory_name/target_filename`.
    Loose,
    /// Explicit successive embedded and/or external, nonspanning cabinets.
    Cabinets {
        /// Ordered groups covering every file exactly once.
        cabinets: Vec<InstallerCabinetSpec>,
    },
}

/// Caller-controlled destinations for external cabinets and loose files.
///
/// The library opens each explicit name, writes bounded bytes, and flushes the
/// writer before calling `finish`. Implement `finish` to finalize any additional
/// storage operation and propagate its failures. Atomic publication and cleanup
/// of incomplete artifacts are the caller's responsibility.
pub trait InstallerMediaSink {
    /// Writer for one explicitly named artifact.
    type Writer: Write;
    /// Open a separate destination for the supplied portable relative name.
    fn create(&mut self, name: &str) -> io::Result<Self::Writer>;
    /// Finalize the flushed writer; success records this artifact as completed.
    fn finish(&mut self, name: &str, writer: Self::Writer) -> io::Result<()>;
}

/// A fully finalized external artifact, without a durability or trust claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallerMediaArtifact {
    /// Explicit relative media name supplied to the sink.
    pub name: String,
    /// Actual artifact bytes written and finalized.
    pub bytes: u64,
}

/// External artifacts completed by a successful emission operation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InstallerMediaReport {
    /// Artifacts in emission order, after successful flush and sink finalization.
    pub completed: Vec<InstallerMediaArtifact>,
    /// Aggregate actual output bytes.
    pub bytes_written: u64,
}

pub(super) struct PreparedMedia {
    pub rows: Vec<(i32, Option<String>)>,
    pub embedded: Vec<(String, Vec<u8>)>,
    pub external: Vec<(String, Vec<u8>)>,
    pub compressed: bool,
}

pub(super) struct RejectMediaSink;
impl InstallerMediaSink for RejectMediaSink {
    type Writer = io::Sink;
    fn create(&mut self, _: &str) -> io::Result<Self::Writer> {
        Err(io::Error::other(
            "external MSI media requires an explicit sink",
        ))
    }
    fn finish(&mut self, _: &str, _: Self::Writer) -> io::Result<()> {
        Ok(())
    }
}

fn invalid(message: &str) -> WriteError {
    WriteError::InvalidInput(message.into())
}

fn leaf(name: &str) -> Result<(), WriteError> {
    if name.is_empty()
        || !name.is_ascii()
        || name.len() > 128
        || name.ends_with(['.', ' '])
        || matches!(name, "." | "..")
        || name
            .bytes()
            .any(|b| b.is_ascii_control() || b"\\/:*?\"<>|".contains(&b))
    {
        return Err(invalid("MSI media requires safe ASCII leaf names"));
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(invalid("reserved Windows media name"));
    }
    Ok(())
}

fn relative_name(name: &str) -> Result<(), WriteError> {
    for part in name.split('/') {
        leaf(part)?;
    }
    if name.len() > 260 {
        return Err(invalid("MSI media path length"));
    }
    Ok(())
}

pub(super) fn prepare(
    files: &[(&str, &str, &[u8])],
    layout: &InstallerMediaLayout,
    directory_name: &str,
    limits: &WriteLimits,
) -> Result<PreparedMedia, WriteError> {
    if files.is_empty() || files.len() > 32767 {
        return Err(invalid("MSI media requires 1..32767 files"));
    }
    if files.len() as u64 > limits.max_entries {
        return Err(WriteError::LimitExceeded("MSI media files"));
    }
    leaf(directory_name)?;
    let mut names = BTreeSet::new();
    let mut identifiers = BTreeSet::new();
    let mut source_bytes = 0u64;
    let mut metadata = directory_name.len() as u64;
    for (id, name, bytes) in files {
        if id.is_empty()
            || id.len() > 60
            || !id.as_bytes()[0].is_ascii_alphabetic()
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(invalid("invalid MSI cabinet file identifier"));
        }
        leaf(name)?;
        if !names.insert(name.to_ascii_lowercase()) || !identifiers.insert(id.to_ascii_lowercase())
        {
            return Err(invalid("duplicate MSI media file names or identifiers"));
        }
        if bytes.len() as u64 > limits.max_file_bytes {
            return Err(WriteError::LimitExceeded("MSI media payload"));
        }
        source_bytes = source_bytes
            .checked_add(bytes.len() as u64)
            .ok_or(WriteError::LimitExceeded("MSI media bytes"))?;
        metadata = metadata
            .checked_add(id.len() as u64)
            .and_then(|n| n.checked_add(name.len() as u64))
            .ok_or(WriteError::LimitExceeded("MSI media metadata"))?;
    }
    if source_bytes > limits.max_total_bytes
        || source_bytes > limits.max_scratch_bytes
        || metadata > limits.max_metadata_bytes
    {
        return Err(WriteError::LimitExceeded("MSI media source resources"));
    }
    let backend_layout = match layout {
        InstallerMediaLayout::Embedded => msi::media::Layout::Embedded,
        InstallerMediaLayout::Loose => msi::media::Layout::Loose,
        InstallerMediaLayout::ExternalCabinet { name } => {
            cabinet_name(name)?;
            msi::media::Layout::ExternalCabinet { name: name.clone() }
        }
        InstallerMediaLayout::Cabinets { cabinets } => {
            if cabinets.len() as u64 > limits.max_entries || cabinets.len() > 32767 {
                return Err(WriteError::LimitExceeded("MSI cabinet count"));
            }
            for cabinet in cabinets {
                cabinet_name(&cabinet.name)?;
                metadata = metadata
                    .checked_add(cabinet.name.len() as u64)
                    .ok_or(WriteError::LimitExceeded("MSI media metadata"))?;
            }
            if metadata > limits.max_metadata_bytes {
                return Err(WriteError::LimitExceeded("MSI media metadata"));
            }
            msi::media::Layout::Cabinets {
                cabinets: cabinets
                    .iter()
                    .map(|cabinet| msi::media::CabinetSpec {
                        name: cabinet.name.clone(),
                        file_count: cabinet.file_count,
                        embedded: cabinet.embedded,
                    })
                    .collect(),
            }
        }
    };
    let prepared = msi::media::prepare(
        files,
        &backend_layout,
        directory_name,
        &backend_limits(limits),
    )
    .map_err(backend_error)?;
    let (rows, embedded, external, compressed) = prepared.into_parts();
    Ok(PreparedMedia {
        rows,
        embedded,
        external,
        compressed,
    })
}

fn cabinet_name(name: &str) -> Result<(), WriteError> {
    leaf(name)?;
    if name.starts_with('#') || name.len() > 60 || !name.to_ascii_lowercase().ends_with(".cab") {
        return Err(invalid("invalid MSI cabinet specification"));
    }
    Ok(())
}

fn backend_limits(limits: &WriteLimits) -> msi::media::Limits {
    msi::media::Limits {
        max_entries: limits.max_entries,
        max_metadata_bytes: limits.max_metadata_bytes,
        max_file_bytes: limits.max_file_bytes,
        max_total_bytes: limits.max_total_bytes,
        max_output_bytes: limits.max_output_bytes,
        max_scratch_bytes: limits.max_scratch_bytes,
    }
}

fn backend_error(error: msi::media::Error) -> WriteError {
    match error {
        msi::media::Error::Io(error) => WriteError::Io(error),
        msi::media::Error::Limit(limit) => WriteError::LimitExceeded(limit),
        msi::media::Error::Invalid(message) => WriteError::InvalidInput(message),
        msi::media::Error::Integrity(message) => WriteError::InvalidInput(message),
        msi::media::Error::Unsupported(message) => WriteError::Unsupported(message),
        msi::media::Error::Emission {
            completed,
            incomplete,
            bytes_written,
            source,
        } => WriteError::Media {
            completed,
            incomplete,
            bytes_written,
            source: Box::new(backend_error(*source)),
        },
        error => WriteError::Unsupported(error.to_string()),
    }
}

struct SinkAdapter<'a, S>(&'a mut S);
impl<S: InstallerMediaSink> msi::media::Sink for SinkAdapter<'_, S> {
    type Writer = S::Writer;
    fn create(&mut self, name: &str) -> io::Result<Self::Writer> {
        self.0.create(name)
    }
    fn finish(&mut self, name: &str, writer: Self::Writer) -> io::Result<()> {
        self.0.finish(name, writer)
    }
}

/// Finalize explicitly named media artifacts in caller-provided destinations.
/// Preflight rejects conflicting names and budgets before opening any sink.
/// `WriteError::Media` records completed artifacts and the partial current name.
pub fn write_installer_media<S: InstallerMediaSink>(
    artifacts: &[(String, Vec<u8>)],
    sink: &mut S,
    limits: &WriteLimits,
) -> Result<InstallerMediaReport, WriteError> {
    if artifacts.len() as u64 > limits.max_entries {
        return Err(WriteError::LimitExceeded("MSI media artifacts"));
    }
    // The public wrapper retains the qualified ASCII admission profile. The
    // backend owns duplicate/sequence checks, budgets, writes and finalization.
    for (name, _) in artifacts {
        relative_name(name)?;
    }
    let report =
        msi::media::write_media(artifacts, &mut SinkAdapter(sink), &backend_limits(limits))
            .map_err(backend_error)?;
    Ok(InstallerMediaReport {
        completed: report
            .completed
            .into_iter()
            .map(|artifact| InstallerMediaArtifact {
                name: artifact.name,
                bytes: artifact.bytes,
            })
            .collect(),
        bytes_written: report.bytes_written,
    })
}
