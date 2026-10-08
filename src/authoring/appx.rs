//! Bounded unsigned APPX creation and rebuilding.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek, Write};

use base64::Engine;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

use super::xml::{MetadataWriter, parse};
use super::{SignaturePolicy, WriteError, WriteLimits, WriteOptions, WriteReport};

#[path = "appx_deflate.rs"]
mod deflate;

/// Compression for single-package payloads; package metadata remains stored.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AppxCompression {
    /// Store decoded bytes unchanged (the default).
    #[default]
    Stored,
    /// Raw DEFLATE, independently flushed at each 64-KiB decoded block.
    Deflate,
}

/// Creates unsigned packages from caller-authored manifest bytes.
/// Payloads are buffered within the explicit scratch budget. Output uses fixed
/// DOS timestamps, sorted UTF-8 names, and no ZIP64. Payloads are stored by
/// default; [`AppxCompression::Deflate`] enables independent compressed blocks.
///
/// ```
/// # #[cfg(feature = "write")]
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use ms_package::{AppxPackage, authoring::{AppxBuilder, WriteOptions}};
/// use std::io::Cursor;
/// let manifest = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Example" Publisher="CN=Example" Version="1.0.0.0"/></Package>"#;
/// let mut builder = AppxBuilder::new(manifest.as_slice(), WriteOptions::default())?;
/// builder.add_file("hello.txt", Cursor::new(b"hello"))?;
/// let mut output = Cursor::new(Vec::new());
/// builder.write(&mut output)?;
/// let mut package = AppxPackage::open(output, Default::default(), 1 << 20)?;
/// assert_eq!(package.validate(1 << 20)?.files_verified, 2);
/// # Ok(()) }
/// # #[cfg(not(feature = "write"))]
/// # fn main() {}
/// ```
pub struct AppxBuilder {
    entries: BTreeMap<String, Vec<u8>>,
    options: WriteOptions,
    content_types: Option<BTreeMap<String, String>>,
    compression: AppxCompression,
}

impl AppxBuilder {
    /// Begin a package. Manifest schema/deployment qualification is caller owned.
    pub fn new(manifest: impl Into<Vec<u8>>, options: WriteOptions) -> Result<Self, WriteError> {
        if options.limits.max_entries < 3 {
            return Err(WriteError::LimitExceeded("entry count"));
        }
        let manifest = manifest.into();
        if manifest.len() as u64 > options.limits.max_metadata_bytes {
            return Err(WriteError::LimitExceeded("manifest bytes"));
        }
        if manifest.len() as u64 > options.limits.max_total_bytes {
            return Err(WriteError::LimitExceeded("decoded bytes"));
        }
        if manifest.len() as u64 > options.limits.max_scratch_bytes {
            return Err(WriteError::LimitExceeded("scratch bytes"));
        }
        let mut entries = BTreeMap::new();
        entries.insert("AppxManifest.xml".into(), manifest);
        Ok(Self {
            entries,
            options,
            content_types: None,
            compression: AppxCompression::Stored,
        })
    }

    /// Choose compression for all payloads. Manifest and integrity metadata stay stored.
    pub fn set_compression(&mut self, compression: AppxCompression) {
        self.compression = compression;
    }

    /// Consume an explicitly opened source, rejecting duplicate package paths.
    pub fn add_file(&mut self, name: &str, source: impl Read) -> Result<(), WriteError> {
        validate_payload_path(name)?;
        if self.entries.len() as u64 + 3 > self.options.limits.max_entries {
            return Err(WriteError::LimitExceeded("entry count"));
        }
        check_collision(&self.entries, name)?;
        let remaining = remaining_budget(&self.entries, &self.options.limits)?;
        let data = read_bounded(source, self.options.limits.max_file_bytes.min(remaining))?;
        self.entries.insert(name.into(), data);
        Ok(())
    }

    /// Finalize into a destination. I/O errors may leave a partial destination.
    pub fn write<W: Write + Seek>(self, mut output: W) -> Result<WriteReport, WriteError> {
        validate_manifest_references(&self.entries, self.options.limits.max_metadata_bytes)?;
        let mut scratch = Cursor::new(Vec::new());
        let report = emit_zip_with_types(
            self.entries,
            &self.options,
            &mut scratch,
            self.content_types,
            self.compression,
        )?;
        let mut package = crate::AppxPackage::open(
            Cursor::new(scratch.get_ref()),
            reader_limits(&self.options.limits),
            self.options.limits.max_metadata_bytes,
        )?;
        package.validate(self.options.limits.max_total_bytes)?;
        output.write_all(scratch.get_ref())?;
        output.flush()?;
        Ok(report)
    }
}

/// Rebuilds a validated package in a separate caller-owned destination.
pub struct AppxEditor {
    builder: AppxBuilder,
    signature_removed: bool,
}

impl AppxEditor {
    /// Validate source integrity before any destination bytes are emitted.
    /// Signed sources fail by default; `Remove` strips the signature footprint.
    pub fn open<R: Read + Seek>(mut source: R, options: WriteOptions) -> Result<Self, WriteError> {
        deflate::preflight(&mut source, &options)?;
        let mut package = crate::AppxPackage::open(
            source,
            reader_limits(&options.limits),
            options.limits.max_metadata_bytes,
        )?;
        validate_manifest_profile(package.manifest(), options.limits.max_metadata_bytes)?;
        if package
            .entries()
            .iter()
            .any(|entry| !matches!(entry.kind, archive_core::EntryKind::File))
        {
            return Err(WriteError::Unsupported(
                "APPX editor requires regular file entries".into(),
            ));
        }
        let source_entries = package.entries().to_vec();
        for entry in &source_entries {
            let metadata = package.entry_metadata(entry.id)?;
            if !matches!(
                metadata.format,
                Some(archive_core::EntryFormatMetadata::Zip {
                    compression_method: 0 | 8,
                    ..
                })
            ) {
                return Err(WriteError::Unsupported(format!(
                    "source compression {}",
                    entry.compression
                )));
            }
            let metadata = matches!(
                entry.name.as_str(),
                "AppxManifest.xml"
                    | "AppxBlockMap.xml"
                    | "[Content_Types].xml"
                    | "AppxSignature.p7x"
            );
            let maximum = if metadata {
                options.limits.max_metadata_bytes
            } else {
                options.limits.max_file_bytes
            };
            if entry.size > maximum {
                return Err(WriteError::LimitExceeded(if metadata {
                    "metadata bytes"
                } else {
                    "file bytes"
                }));
            }
            if !metadata {
                validate_payload_path(&entry.name)?;
            }
        }
        package.validate(options.limits.max_total_bytes)?;
        let signed = package
            .entries()
            .iter()
            .any(|e| e.name.eq_ignore_ascii_case("AppxSignature.p7x"));
        if signed && options.signature_policy == SignaturePolicy::Reject {
            return Err(WriteError::Unsupported("signed APPX input".into()));
        }
        let mut builder = AppxBuilder::new(package.manifest().to_vec(), options)?;
        builder.content_types = Some(resolve_content_types(
            package.content_types(),
            package.entries(),
        )?);
        let entries: Vec<_> = package
            .entries()
            .iter()
            .map(|e| (e.id, e.name.clone()))
            .collect();
        for (id, name) in entries {
            if matches!(
                name.as_str(),
                "AppxManifest.xml"
                    | "AppxBlockMap.xml"
                    | "[Content_Types].xml"
                    | "AppxSignature.p7x"
            ) {
                continue;
            }
            check_collision(&builder.entries, &name)?;
            let remaining = remaining_budget(&builder.entries, &builder.options.limits)?;
            let data =
                package.read_entry(id, builder.options.limits.max_file_bytes.min(remaining))?;
            builder.entries.insert(name, data);
        }
        Ok(Self {
            builder,
            signature_removed: signed,
        })
    }
    /// Choose output payload compression independently of the source layout.
    pub fn set_compression(&mut self, compression: AppxCompression) {
        self.builder.set_compression(compression);
    }

    /// Add a new payload.
    pub fn add_file(&mut self, name: &str, source: impl Read) -> Result<(), WriteError> {
        self.builder.add_file(name, source)
    }
    /// Replace an existing payload; failed reads preserve the old entry.
    pub fn replace_file(&mut self, name: &str, source: impl Read) -> Result<(), WriteError> {
        validate_payload_path(name)?;
        let old_size = self
            .builder
            .entries
            .get(name)
            .ok_or_else(|| WriteError::InvalidInput(format!("missing {name}")))?
            .len() as u64;
        let retained = decoded_size(&self.builder.entries)?;
        let limits = &self.builder.options.limits;
        // The old bytes remain live until the read succeeds, but no longer count
        // toward the final decoded package size.
        let scratch_remaining = limits
            .max_scratch_bytes
            .checked_sub(retained)
            .ok_or(WriteError::LimitExceeded("scratch bytes"))?;
        let decoded_remaining = limits
            .max_total_bytes
            .checked_sub(retained - old_size)
            .ok_or(WriteError::LimitExceeded("decoded bytes"))?;
        let remaining = scratch_remaining.min(decoded_remaining);
        let data = read_bounded(
            source,
            self.builder.options.limits.max_file_bytes.min(remaining),
        )?;
        self.builder.entries.insert(name.into(), data);
        Ok(())
    }
    /// Remove an existing payload.
    pub fn remove_file(&mut self, name: &str) -> Result<(), WriteError> {
        validate_payload_path(name)?;
        self.builder
            .entries
            .remove(name)
            .ok_or_else(|| WriteError::InvalidInput(format!("missing {name}")))?;
        if let Some(types) = &mut self.builder.content_types {
            types.remove(name);
        }
        Ok(())
    }
    /// Rename without overwriting an existing path. Manifest references are
    /// checked when writing; callers must replace affected manifest references.
    pub fn rename_file(&mut self, old: &str, new: &str) -> Result<(), WriteError> {
        validate_payload_path(old)?;
        validate_payload_path(new)?;
        if old == new {
            return if self.builder.entries.contains_key(old) {
                Ok(())
            } else {
                Err(WriteError::InvalidInput(format!("missing {old}")))
            };
        }
        check_collision_except(&self.builder.entries, new, Some(old))?;
        let data = self
            .builder
            .entries
            .remove(old)
            .ok_or_else(|| WriteError::InvalidInput(format!("missing {old}")))?;
        self.builder.entries.insert(new.into(), data);
        if let Some(types) = &mut self.builder.content_types
            && let Some(value) = types.remove(old)
        {
            types.insert(new.into(), value);
        }
        Ok(())
    }
    /// Replace manifest bytes explicitly, preserving bytes on payload-only edits.
    pub fn replace_manifest(&mut self, manifest: impl Into<Vec<u8>>) -> Result<(), WriteError> {
        let manifest = manifest.into();
        if manifest.len() as u64 > self.builder.options.limits.max_metadata_bytes {
            return Err(WriteError::LimitExceeded("manifest bytes"));
        }
        let retained = decoded_size(&self.builder.entries)?;
        let old_size = self
            .builder
            .entries
            .get("AppxManifest.xml")
            .ok_or_else(|| WriteError::InvalidInput("missing AppxManifest.xml".into()))?
            .len() as u64;
        let replacement_size = manifest.len() as u64;
        if retained
            .checked_add(replacement_size)
            .is_none_or(|bytes| bytes > self.builder.options.limits.max_scratch_bytes)
        {
            return Err(WriteError::LimitExceeded("scratch bytes"));
        }
        if (retained - old_size)
            .checked_add(replacement_size)
            .is_none_or(|bytes| bytes > self.builder.options.limits.max_total_bytes)
        {
            return Err(WriteError::LimitExceeded("decoded bytes"));
        }
        self.builder
            .entries
            .insert("AppxManifest.xml".into(), manifest);
        Ok(())
    }
    /// Rebuild content types and block hashes, finalize, and flush output.
    pub fn write<W: Write + Seek>(self, output: W) -> Result<WriteReport, WriteError> {
        let mut report = self.builder.write(output)?;
        report.signature_removed = self.signature_removed;
        Ok(report)
    }
}

pub(crate) fn read_bounded(mut source: impl Read, max: u64) -> Result<Vec<u8>, WriteError> {
    let mut data = Vec::new();
    let mut buffer = [0; 65536];
    loop {
        let available =
            ((max.saturating_sub(data.len() as u64)).min(buffer.len() as u64 - 1) + 1) as usize;
        let count = source.read(&mut buffer[..available])?;
        if count == 0 {
            break;
        }
        if data.len() as u64 + count as u64 > max {
            return Err(WriteError::LimitExceeded("source bytes"));
        }
        data.extend_from_slice(&buffer[..count]);
    }
    Ok(data)
}

pub(crate) fn reader_limits(limits: &WriteLimits) -> archive_core::Limits {
    archive_core::Limits {
        max_entries: limits.max_entries,
        max_metadata_bytes: limits.max_metadata_bytes,
        max_entry_bytes: limits.max_file_bytes.max(limits.max_metadata_bytes),
        max_total_bytes: limits.max_total_bytes,
        max_input_bytes: limits.max_output_bytes,
        ..Default::default()
    }
}

pub(crate) fn validate_path(name: &str) -> Result<(), WriteError> {
    if name.is_empty()
        || name.encode_utf16().count() > 260
        || name.contains('\\')
        || name.starts_with('/')
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"|?*".contains(c))
        || name.split('/').any(|p| {
            let stem = p.split('.').next().unwrap_or_default().to_ascii_uppercase();
            p.is_empty()
                || p == "."
                || p == ".."
                || p.ends_with(['.', ' '])
                || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(
                        &stem[3..],
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    ))
        })
    {
        return Err(WriteError::InvalidInput(format!(
            "invalid package path {name:?}"
        )));
    }
    Ok(())
}

fn validate_payload_path(name: &str) -> Result<(), WriteError> {
    validate_path(name)?;
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.split('/').next().unwrap_or_default(),
        "appxmanifest.xml"
            | "appxblockmap.xml"
            | "appxsignature.p7x"
            | "[content_types].xml"
            | "appxmetadata"
    ) {
        return Err(WriteError::Unsupported(format!(
            "reserved package metadata {name}"
        )));
    }
    Ok(())
}

fn check_collision(entries: &BTreeMap<String, Vec<u8>>, name: &str) -> Result<(), WriteError> {
    check_collision_except(entries, name, None)
}

fn check_collision_except(
    entries: &BTreeMap<String, Vec<u8>>,
    name: &str,
    excluded: Option<&str>,
) -> Result<(), WriteError> {
    let lower = name.to_lowercase();
    if entries.keys().any(|key| {
        if excluded == Some(key.as_str()) {
            return false;
        }
        let key = key.to_lowercase();
        key == lower
            || key.starts_with(&format!("{lower}/"))
            || lower.starts_with(&format!("{key}/"))
    }) {
        return Err(WriteError::InvalidInput(format!(
            "colliding package path {name}"
        )));
    }
    Ok(())
}

fn remaining_budget(
    entries: &BTreeMap<String, Vec<u8>>,
    limits: &WriteLimits,
) -> Result<u64, WriteError> {
    let total = decoded_size(entries)?;
    limits
        .max_scratch_bytes
        .min(limits.max_total_bytes)
        .checked_sub(total)
        .ok_or(WriteError::LimitExceeded("scratch bytes"))
}

fn decoded_size(entries: &BTreeMap<String, Vec<u8>>) -> Result<u64, WriteError> {
    entries.values().try_fold(0u64, |n, v| {
        n.checked_add(v.len() as u64)
            .ok_or(WriteError::LimitExceeded("decoded bytes"))
    })
}

fn uri_path(name: &str) -> String {
    let mut result = String::new();
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            result.push(byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

pub(crate) fn emit_zip<W: Write + Seek>(
    entries: BTreeMap<String, Vec<u8>>,
    options: &WriteOptions,
    output: W,
) -> Result<WriteReport, WriteError> {
    emit_zip_with_types(entries, options, output, None, AppxCompression::Stored)
}

fn emit_zip_with_types<W: Write + Seek>(
    mut entries: BTreeMap<String, Vec<u8>>,
    options: &WriteOptions,
    mut output: W,
    content_types: Option<BTreeMap<String, String>>,
    compression: AppxCompression,
) -> Result<WriteReport, WriteError> {
    let limits = &options.limits;
    remaining_budget(&entries, limits)?;
    if entries.len() as u64 + 2 > limits.max_entries || entries.len() + 2 >= u16::MAX as usize {
        return Err(WriteError::LimitExceeded("entry count"));
    }
    let bundle = entries.contains_key("AppxMetadata/AppxBundleManifest.xml");
    let mut seen = BTreeSet::new();
    let mut decoded = 0u64;
    let retained = decoded_size(&entries)?;
    let mut encoded_entries = BTreeMap::new();
    let mut encoded_total = 0u64;
    let mut largest_carrier = 0u64;
    let metadata_budget = |encoded: u64, other: u64| {
        limits
            .max_scratch_bytes
            .checked_sub(retained)
            .and_then(|n| n.checked_sub(encoded))
            .and_then(|n| n.checked_sub(other))
            .and_then(|n| {
                n.checked_sub(if compression == AppxCompression::Deflate {
                    deflate::WORKSPACE
                } else {
                    0
                })
            })
            .ok_or(WriteError::LimitExceeded(
                "generated metadata scratch bytes",
            ))
    };
    let mut blockmap = MetadataWriter::new(limits.max_metadata_bytes.min(metadata_budget(0, 0)?));
    blockmap.start("BlockMap")?;
    blockmap.attribute("xmlns", "http://schemas.microsoft.com/appx/2010/blockmap")?;
    blockmap.attribute("HashMethod", "http://www.w3.org/2001/04/xmlenc#sha256")?;
    let mut types = MetadataWriter::new(
        limits
            .max_metadata_bytes
            .min(metadata_budget(0, blockmap.len())?),
    );
    types.start("Types")?;
    types.attribute(
        "xmlns",
        "http://schemas.openxmlformats.org/package/2006/content-types",
    )?;
    for (name, data) in &entries {
        validate_path(name)?;
        if !seen.insert(name.to_lowercase()) {
            return Err(WriteError::InvalidInput("case-colliding entries".into()));
        }
        let file_limit = if matches!(
            name.as_str(),
            "AppxManifest.xml" | "AppxMetadata/AppxBundleManifest.xml"
        ) {
            limits.max_metadata_bytes
        } else {
            limits.max_file_bytes
        };
        if data.len() as u64 > file_limit || data.len() >= u32::MAX as usize {
            return Err(WriteError::LimitExceeded("file bytes"));
        }
        decoded = decoded
            .checked_add(data.len() as u64)
            .ok_or(WriteError::LimitExceeded("decoded bytes"))?;
        let encoded = if compression == AppxCompression::Deflate && name != "AppxManifest.xml" {
            let remaining = limits
                .max_scratch_bytes
                .checked_sub(retained)
                .and_then(|n| n.checked_sub(encoded_total))
                .and_then(|n| n.checked_sub(blockmap.len()))
                .and_then(|n| n.checked_sub(types.len()))
                .and_then(|n| n.checked_sub(deflate::WORKSPACE))
                .ok_or(WriteError::LimitExceeded("codec scratch bytes"))?;
            let encoded = deflate::encode(data, remaining.min(limits.max_output_bytes))?;
            encoded_total = encoded_total
                .checked_add(encoded.bytes.len() as u64)
                .ok_or(WriteError::LimitExceeded("compressed bytes"))?;
            largest_carrier =
                largest_carrier.max(encoded.bytes.len() as u64 + 98 + 2 * name.len() as u64);
            Some(encoded)
        } else {
            None
        };
        blockmap.set_limit(metadata_budget(encoded_total, types.len())?);
        if !(bundle && (name.ends_with(".appx") || name.ends_with(".msix"))) {
            blockmap.start("File")?;
            blockmap.attribute("Name", &name.replace('/', "\\"))?;
            blockmap.attribute("Size", &data.len().to_string())?;
            blockmap.attribute("LfhSize", &(30 + name.len()).to_string())?;
            for (index, block) in data.chunks(65536).enumerate() {
                blockmap.start("Block")?;
                blockmap.attribute(
                    "Hash",
                    &base64::engine::general_purpose::STANDARD.encode(Sha256::digest(block)),
                )?;
                if let Some(encoded) = &encoded {
                    blockmap.attribute("Size", &encoded.blocks[index].to_string())?;
                }
                blockmap.end()?;
            }
            blockmap.end()?;
        }
        let content_type =
            if let Some(value) = content_types.as_ref().and_then(|types| types.get(name)) {
                value.as_str()
            } else if name == "AppxManifest.xml" {
                "application/vnd.ms-appx.manifest+xml"
            } else if name == "AppxMetadata/AppxBundleManifest.xml" {
                "application/vnd.ms-appx.bundlemanifest+xml"
            } else {
                "application/octet-stream"
            };
        types.set_limit(metadata_budget(encoded_total, blockmap.len())?);
        types.start("Override")?;
        types.attribute("PartName", &format!("/{}", uri_path(name)))?;
        types.attribute("ContentType", content_type)?;
        types.end()?;
        if let Some(encoded) = encoded {
            encoded_entries.insert(name.clone(), encoded);
        }
    }
    blockmap.set_limit(metadata_budget(encoded_total, types.len())?);
    blockmap.end()?;
    types.set_limit(metadata_budget(encoded_total, blockmap.len())?);
    types.start("Override")?;
    types.attribute("PartName", "/AppxBlockMap.xml")?;
    types.attribute("ContentType", "application/vnd.ms-appx.blockmap+xml")?;
    types.end()?;
    types.end()?;
    entries.insert("AppxBlockMap.xml".into(), blockmap.finish()?);
    entries.insert("[Content_Types].xml".into(), types.finish()?);
    let estimated = entries.iter().try_fold(22u64, |n, (name, data)| {
        let size = encoded_entries
            .get(name)
            .map_or(data.len(), |entry| entry.bytes.len());
        n.checked_add(76 + 2 * name.len() as u64 + size as u64)
            .ok_or(WriteError::LimitExceeded("output bytes"))
    })?;
    let compression_scratch = if compression == AppxCompression::Deflate {
        encoded_total
            .checked_add(largest_carrier)
            .and_then(|n| n.checked_add(deflate::WORKSPACE))
            .ok_or(WriteError::LimitExceeded("codec scratch bytes"))?
    } else {
        0
    };
    if estimated > limits.max_output_bytes
        || estimated
            .checked_mul(2)
            .and_then(|n| n.checked_add(decoded))
            .and_then(|n| n.checked_add(compression_scratch))
            .is_none_or(|n| n > limits.max_scratch_bytes)
        || estimated >= u32::MAX as u64
    {
        return Err(WriteError::LimitExceeded("output bytes"));
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let file_options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default());
    let count = entries.len() as u64;
    let mut ordered: Vec<_> = entries.into_iter().collect();
    if bundle {
        ordered.sort_by_key(|(name, _)| {
            (
                !(name.ends_with(".appx") || name.ends_with(".msix")),
                name.clone(),
            )
        });
    }
    for (name, data) in ordered {
        if let Some(encoded) = encoded_entries.remove(&name) {
            deflate::copy_into(&mut zip, &name, &data, &encoded.bytes)?;
        } else {
            zip.start_file(name, file_options)?;
            zip.write_all(&data)?;
        }
    }
    let bytes = zip.finish()?.into_inner();
    if bytes.len() as u64 > limits.max_output_bytes || bytes.len() as u64 > limits.max_scratch_bytes
    {
        return Err(WriteError::LimitExceeded("output bytes"));
    }
    output.write_all(&bytes)?;
    output.flush()?;
    Ok(WriteReport {
        entries: count,
        decoded_bytes: decoded,
        output_bytes: bytes.len() as u64,
        signature_removed: false,
    })
}

fn validate_manifest_references(
    entries: &BTreeMap<String, Vec<u8>>,
    max_bytes: u64,
) -> Result<(), WriteError> {
    let manifest = entries
        .get("AppxManifest.xml")
        .ok_or_else(|| WriteError::InvalidInput("missing AppxManifest.xml".into()))?;
    let document = parse(manifest, max_bytes)?;
    reject_sparse(&document)?;
    let root = document.root_element();
    if root.tag_name().name() != "Package"
        || !matches!(
            root.tag_name().namespace(),
            Some(
                "http://schemas.microsoft.com/appx/manifest/foundation/windows10"
                    | "http://schemas.microsoft.com/appx/2010/manifest"
            )
        )
    {
        return Err(WriteError::InvalidInput(
            "package manifest root or namespace".into(),
        ));
    }
    for element in root.descendants().filter(|node| node.is_element()) {
        if element.tag_name().name() == "Logo"
            && element.tag_name().namespace() == root.tag_name().namespace()
            && let Some(value) = element.text()
        {
            let value = value.trim();
            if !value.starts_with("ms-resource:")
                && !entries.contains_key(&value.replace('\\', "/"))
            {
                return Err(WriteError::InvalidInput(format!(
                    "missing manifest reference {value}"
                )));
            }
        }
        for attribute in element.attributes() {
            if attribute.namespace().is_some()
                || !element.tag_name().namespace().is_some_and(|namespace| {
                    namespace.starts_with("http://schemas.microsoft.com/appx/manifest/")
                        || namespace == "http://schemas.microsoft.com/appx/2010/manifest"
                })
            {
                continue;
            }
            if matches!(
                attribute.name(),
                "Executable"
                    | "Logo"
                    | "Square44x44Logo"
                    | "Square150x150Logo"
                    | "Wide310x150Logo"
                    | "Image"
                    | "SmallLogo"
                    | "Square310x310Logo"
            ) {
                let value = attribute.value();
                if !value.starts_with("ms-resource:")
                    && !entries.contains_key(&value.replace('\\', "/"))
                {
                    return Err(WriteError::InvalidInput(format!(
                        "missing manifest reference {value}"
                    )));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_manifest_profile(manifest: &[u8], max_bytes: u64) -> Result<(), WriteError> {
    reject_sparse(&parse(manifest, max_bytes)?)
}

fn reject_sparse(document: &roxmltree::Document<'_>) -> Result<(), WriteError> {
    // Microsoft documents this property as a boolean under Package/Properties:
    // https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap10-allowexternalcontent
    for element in document.descendants().filter(|node| {
        node.has_tag_name((
            "http://schemas.microsoft.com/appx/manifest/uap/windows10/10",
            "AllowExternalContent",
        ))
    }) {
        let value: String = element
            .children()
            .filter(|node| node.is_text())
            .filter_map(|node| node.text())
            .collect();
        match value.trim() {
            "false" | "0" => {}
            "true" | "1" => {
                return Err(WriteError::Unsupported(
                    "sparse APPX external content".into(),
                ));
            }
            _ => {
                return Err(WriteError::InvalidInput(
                    "invalid AllowExternalContent boolean".into(),
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn resolve_content_types(
    xml: &[u8],
    entries: &[archive_core::Entry],
) -> Result<BTreeMap<String, String>, WriteError> {
    let mut overrides = BTreeMap::new();
    let mut defaults = BTreeMap::new();
    let document = parse(xml, xml.len() as u64)?;
    let root = document.root_element();
    if root.tag_name().name() != "Types"
        || root.tag_name().namespace()
            != Some("http://schemas.openxmlformats.org/package/2006/content-types")
    {
        return Err(WriteError::InvalidInput(
            "content types root or namespace".into(),
        ));
    }
    if root.attributes().len() != 0 {
        return Err(WriteError::Unsupported(
            "content type root attributes".into(),
        ));
    }
    for element in root.children().filter(|node| node.is_element()) {
        if element.tag_name().namespace() != root.tag_name().namespace() {
            return Err(WriteError::Unsupported(
                "content type extension namespace".into(),
            ));
        }
        let path_attribute = match element.tag_name().name() {
            "Override" => "PartName",
            "Default" => "Extension",
            _ => {
                return Err(WriteError::Unsupported(
                    "content type extension element".into(),
                ));
            }
        };
        if element.attributes().any(|attribute| {
            attribute.namespace().is_some()
                || !matches!(attribute.name(), "ContentType") && attribute.name() != path_attribute
        }) || element.children().any(|node| {
            node.is_element() || node.text().is_some_and(|text| !text.trim().is_empty())
        }) {
            return Err(WriteError::Unsupported(
                "content type extension content".into(),
            ));
        }
        let value = element
            .attribute("ContentType")
            .ok_or_else(|| WriteError::InvalidInput("missing content type".into()))?;
        match element.tag_name().name() {
            "Override" => {
                let path = element
                    .attribute("PartName")
                    .ok_or_else(|| WriteError::InvalidInput("missing PartName".into()))?;
                if overrides
                    .insert(path.to_owned(), value.to_owned())
                    .is_some()
                {
                    return Err(WriteError::InvalidInput(
                        "duplicate content type override".into(),
                    ));
                }
            }
            "Default" => {
                let ext = element
                    .attribute("Extension")
                    .ok_or_else(|| WriteError::InvalidInput("missing Extension".into()))?;
                if defaults
                    .insert(ext.to_ascii_lowercase(), value.to_owned())
                    .is_some()
                {
                    return Err(WriteError::InvalidInput(
                        "duplicate content type extension".into(),
                    ));
                }
            }
            _ => {
                return Err(WriteError::Unsupported(
                    "content type extension element".into(),
                ));
            }
        }
    }
    entries
        .iter()
        .filter(|e| {
            !matches!(
                e.name.as_str(),
                "AppxSignature.p7x" | "AppxBlockMap.xml" | "[Content_Types].xml"
            )
        })
        .map(|e| {
            let value = overrides
                .get(&format!("/{}", uri_path(&e.name)))
                .or_else(|| overrides.get(&format!("/{}", e.name)))
                .or_else(|| {
                    defaults.get(
                        &e.name
                            .rsplit('.')
                            .next()
                            .unwrap_or_default()
                            .to_ascii_lowercase(),
                    )
                })
                .ok_or_else(|| {
                    WriteError::Unsupported(format!("unresolved content type for {}", e.name))
                })?;
            if e.name == "AppxManifest.xml" && value != "application/vnd.ms-appx.manifest+xml" {
                return Err(WriteError::Unsupported("manifest content type".into()));
            }
            Ok((e.name.clone(), value.clone()))
        })
        .collect()
}
