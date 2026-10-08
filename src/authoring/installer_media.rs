//! Explicit, bounded nonspanning MSI media preparation and caller-owned publication.
use super::{WriteError, WriteLimits};
use std::{
    borrow::Cow,
    collections::BTreeSet,
    io::{self, Cursor, Write},
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
    let mut prepared = PreparedMedia {
        rows: Vec::new(),
        embedded: Vec::new(),
        external: Vec::new(),
        compressed: !matches!(layout, InstallerMediaLayout::Loose),
    };
    if matches!(layout, InstallerMediaLayout::Loose) {
        if source_bytes
            .checked_mul(2)
            .is_none_or(|n| n > limits.max_scratch_bytes)
            || source_bytes > limits.max_output_bytes
        {
            return Err(WriteError::LimitExceeded("loose MSI media scratch/output"));
        }
        for (_, name, bytes) in files {
            prepared
                .external
                .push((format!("{directory_name}/{name}"), bytes.to_vec()));
        }
        prepared.rows.push((files.len() as i32, None));
        return Ok(prepared);
    }
    let specs: Cow<'_, [InstallerCabinetSpec]> = match layout {
        InstallerMediaLayout::Embedded => Cow::Owned(vec![InstallerCabinetSpec {
            name: "payload.cab".into(),
            file_count: files.len() as u64,
            embedded: true,
        }]),
        InstallerMediaLayout::ExternalCabinet { name } => {
            leaf(name)?;
            Cow::Owned(vec![InstallerCabinetSpec {
                name: name.clone(),
                file_count: files.len() as u64,
                embedded: false,
            }])
        }
        InstallerMediaLayout::Cabinets { cabinets } => Cow::Borrowed(cabinets),
        InstallerMediaLayout::Loose => return Err(invalid("unexpected cabinet layout")),
    };
    if specs.is_empty() || specs.len() > 32767 || specs.len() as u64 > limits.max_entries {
        return Err(WriteError::LimitExceeded("MSI cabinet count"));
    }
    let mut media_names = BTreeSet::new();
    let mut file_count = 0u64;
    for spec in specs.iter() {
        leaf(&spec.name)?;
        if !spec.name.to_ascii_lowercase().ends_with(".cab")
            || spec.name.starts_with('#')
            || spec.name.len() > 60
            || spec.file_count == 0
            || !media_names.insert(spec.name.to_ascii_lowercase())
        {
            return Err(invalid("invalid or duplicate MSI cabinet specification"));
        }
        file_count = file_count
            .checked_add(spec.file_count)
            .ok_or(WriteError::LimitExceeded("MSI cabinet files"))?;
        metadata = metadata
            .checked_add(spec.name.len() as u64)
            .ok_or(WriteError::LimitExceeded("MSI media metadata"))?;
    }
    if file_count != files.len() as u64 {
        return Err(invalid("cabinet groups must cover every file exactly once"));
    }
    if metadata > limits.max_metadata_bytes {
        return Err(WriteError::LimitExceeded("MSI media metadata"));
    }
    let mut index = 0usize;
    let mut staged = 0u64;
    for spec in specs.iter() {
        let end = index
            .checked_add(
                usize::try_from(spec.file_count)
                    .map_err(|_| WriteError::LimitExceeded("MSI cabinet files"))?,
            )
            .ok_or(WriteError::LimitExceeded("MSI cabinet files"))?;
        let mut cabinet = cabinet::CabinetBuilder::new(cabinet::WriteCompression::None);
        for (id, _, bytes) in &files[index..end] {
            cabinet.add_file(id, bytes)?;
        }
        let available = limits
            .max_scratch_bytes
            .checked_sub(source_bytes)
            .and_then(|n| n.checked_sub(staged))
            .and_then(|n| n.checked_sub(32768))
            .ok_or(WriteError::LimitExceeded("MSI cabinet scratch"))?;
        let mut output = super::installer::BoundedCursor {
            inner: Cursor::new(Vec::new()),
            limit: available
                .min(limits.max_file_bytes)
                .min(limits.max_output_bytes.saturating_sub(staged)),
        };
        cabinet.write(&mut output)?;
        output.flush()?;
        let bytes = output.inner.into_inner();
        staged = staged
            .checked_add(bytes.len() as u64)
            .ok_or(WriteError::LimitExceeded("MSI media output"))?;
        prepared.rows.push((
            end as i32,
            Some(if spec.embedded {
                format!("#{}", spec.name)
            } else {
                spec.name.clone()
            }),
        ));
        if spec.embedded {
            prepared.embedded.push((spec.name.clone(), bytes));
        } else {
            prepared.external.push((spec.name.clone(), bytes));
        }
        index = end;
    }
    Ok(prepared)
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
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    let mut metadata = 0u64;
    for (name, bytes) in artifacts {
        relative_name(name)?;
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(invalid("duplicate MSI media artifact name"));
        }
        if bytes.len() as u64 > limits.max_file_bytes {
            return Err(WriteError::LimitExceeded("MSI media artifact bytes"));
        }
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or(WriteError::LimitExceeded("MSI media output"))?;
        metadata = metadata
            .checked_add(name.len() as u64)
            .ok_or(WriteError::LimitExceeded("MSI media names"))?;
    }
    for name in &names {
        for (index, _) in name.match_indices('/') {
            if names.contains(&name[..index]) {
                return Err(invalid("MSI media artifact file/directory collision"));
            }
        }
    }
    if total > limits.max_output_bytes
        || total > limits.max_scratch_bytes
        || metadata > limits.max_metadata_bytes
    {
        return Err(WriteError::LimitExceeded("MSI media emission resources"));
    }
    let mut report = InstallerMediaReport::default();
    for (name, bytes) in artifacts {
        let mut written = 0u64;
        let result = (|| -> Result<(), WriteError> {
            let mut output = sink.create(name)?;
            while (written as usize) < bytes.len() {
                match output.write(&bytes[written as usize..]) {
                    Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero).into()),
                    Ok(count) if count <= bytes.len() - written as usize => written += count as u64,
                    Ok(_) => {
                        return Err(
                            io::Error::other("media sink returned an invalid write count").into(),
                        );
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error.into()),
                }
            }
            output.flush()?;
            sink.finish(name, output)?;
            Ok(())
        })();
        report.bytes_written += written;
        if let Err(source) = result {
            return Err(WriteError::Media {
                completed: report
                    .completed
                    .iter()
                    .map(|artifact| artifact.name.clone())
                    .collect(),
                incomplete: name.clone(),
                bytes_written: report.bytes_written,
                source: Box::new(source),
            });
        }
        report.completed.push(InstallerMediaArtifact {
            name: name.clone(),
            bytes: written,
        });
    }
    Ok(report)
}
