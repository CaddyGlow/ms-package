//! Bounded, unsigned bundle rebuilding with independently verified nested packages.
use super::appx::{
    emit_zip, read_bounded, reader_limits, resolve_content_types, validate_manifest_profile,
    validate_path,
};
use super::xml::{MetadataWriter, parse};
use super::{AppxEditor, WriteError, WriteOptions, WriteReport};
use crate::{AppxBundle, AppxPackage};
use archive_core::{Archive, EntryKind};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Seek, Write},
};

const MANIFEST: &str = "AppxMetadata/AppxBundleManifest.xml";

struct NestedPackage {
    bytes: Vec<u8>,
    identity: BTreeMap<String, String>,
    package_type: &'static str,
    qualifiers: Vec<BTreeMap<String, String>>,
    decoded_bytes: u64,
}

/// Creates an unsigned stored bundle from explicitly supplied, unsigned packages.
/// Nested package bytes and their decoded contents both count toward resource limits.
///
/// ```
/// use ms_package::authoring::{AppxBuilder, AppxBundleBuilder, WriteOptions};
/// use std::io::Cursor;
/// let options = WriteOptions::default();
/// let manifest = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Example.Bundle" Publisher="CN=Example" Version="1.0.0.0" ProcessorArchitecture="x64"/></Package>"#;
/// let package = AppxBuilder::new(manifest.as_slice(), options)?;
/// let mut nested = Cursor::new(Vec::new());
/// package.write(&mut nested)?;
/// let mut bundle = AppxBundleBuilder::new("Example.Bundle", "CN=Example", "1.0.0.0", options)?;
/// bundle.add_package("example-x64.msix", Cursor::new(nested.into_inner()))?;
/// let mut output = Cursor::new(Vec::new());
/// bundle.write(&mut output)?;
/// # Ok::<(), ms_package::authoring::WriteError>(())
/// ```
pub struct AppxBundleBuilder {
    identity: BTreeMap<String, String>,
    packages: BTreeMap<String, NestedPackage>,
    options: WriteOptions,
}

impl AppxBundleBuilder {
    /// Choose the bundle identity and bounded-memory authoring options.
    pub fn new(
        name: impl Into<String>,
        publisher: impl Into<String>,
        version: impl Into<String>,
        options: WriteOptions,
    ) -> Result<Self, WriteError> {
        let identity = BTreeMap::from([
            ("Name".into(), name.into()),
            ("Publisher".into(), publisher.into()),
            ("Version".into(), version.into()),
        ]);
        validate_identity(&identity)?;
        Ok(Self {
            identity,
            packages: BTreeMap::new(),
            options,
        })
    }

    /// Add a package, rejecting duplicate paths and unsupported nested profiles.
    pub fn add_package(
        &mut self,
        name: impl Into<String>,
        source: impl Read,
    ) -> Result<(), WriteError> {
        let name = name.into();
        validate_path(&name)?;
        if name.contains('/')
            || name.encode_utf16().count() > 256
            || !(name.ends_with(".appx") || name.ends_with(".msix"))
        {
            return Err(WriteError::Unsupported(
                "nested packages must have root APPX/MSIX names".into(),
            ));
        }
        if self
            .packages
            .keys()
            .any(|n| n.to_lowercase() == name.to_lowercase())
        {
            return Err(WriteError::InvalidInput(
                "duplicate nested package name".into(),
            ));
        }
        if (self.packages.len() as u64)
            .checked_add(4)
            .is_none_or(|count| count > self.options.limits.max_entries)
        {
            return Err(WriteError::LimitExceeded("bundle entry count"));
        }
        let used = self.packages.values().try_fold(0u64, |n, v| {
            n.checked_add(v.bytes.len() as u64)
                .ok_or(WriteError::LimitExceeded("bundle scratch"))
        })?;
        let remaining = (self.options.limits.max_scratch_bytes / 3)
            .min(self.options.limits.max_total_bytes)
            .checked_sub(used)
            .ok_or(WriteError::LimitExceeded("bundle scratch"))?;
        let bytes = read_bounded(source, self.options.limits.max_file_bytes.min(remaining))?;
        let nested = self.validate_nested(bytes)?;
        self.packages.insert(name.clone(), nested);
        if let Err(error) = self.check_budget(&self.packages) {
            self.packages.remove(&name);
            return Err(error);
        }
        Ok(())
    }

    fn check_budget(&self, packages: &BTreeMap<String, NestedPackage>) -> Result<(), WriteError> {
        if packages.len() as u64 + 3 > self.options.limits.max_entries {
            return Err(WriteError::LimitExceeded("bundle entry count"));
        }
        let mut total = 0u64;
        let mut scratch = 0u64;
        let mut identities = BTreeSet::new();
        for package in packages.values() {
            scratch = scratch
                .checked_add(package.bytes.len() as u64)
                .ok_or(WriteError::LimitExceeded("bundle scratch"))?;
            let identity = &package.identity;
            total = total
                .checked_add(package.decoded_bytes)
                .and_then(|n| n.checked_add(package.bytes.len() as u64))
                .ok_or(WriteError::LimitExceeded("bundle decoded bytes"))?;
            if !identities.insert((
                identity
                    .get("ProcessorArchitecture")
                    .cloned()
                    .unwrap_or_else(|| "neutral".into()),
                identity.get("ResourceId").cloned().unwrap_or_default(),
            )) {
                return Err(WriteError::InvalidInput(
                    "duplicate nested package identity".into(),
                ));
            }
        }
        if total > self.options.limits.max_total_bytes
            || scratch > self.options.limits.max_scratch_bytes / 3
        {
            return Err(WriteError::LimitExceeded("bundle aggregate resources"));
        }
        Ok(())
    }

    fn validate_nested(&self, bytes: Vec<u8>) -> Result<NestedPackage, WriteError> {
        let mut package = AppxPackage::open(
            Cursor::new(&bytes),
            reader_limits(&self.options.limits),
            self.options.limits.max_metadata_bytes,
        )?;
        let mut names = BTreeSet::new();
        let entries = package.entries().to_vec();
        for entry in &entries {
            validate_path(&entry.name)?;
            let limit = if matches!(
                entry.name.as_str(),
                "AppxManifest.xml" | "AppxBlockMap.xml" | "[Content_Types].xml"
            ) {
                self.options.limits.max_metadata_bytes
            } else {
                self.options.limits.max_file_bytes
            };
            if entry.size > limit {
                return Err(WriteError::LimitExceeded("nested decoded file bytes"));
            }
            if !names.insert(entry.name.to_lowercase()) {
                return Err(WriteError::InvalidInput(
                    "colliding nested package paths".into(),
                ));
            }
            let metadata = package.entry_metadata(entry.id)?;
            if entry.encrypted
                || entry.kind != EntryKind::File
                || !matches!(
                    metadata.format,
                    Some(archive_core::EntryFormatMetadata::Zip {
                        compression_method: 0 | 8,
                        ..
                    })
                )
            {
                return Err(WriteError::Unsupported(
                    "nested package must use unsigned Stored or Deflate file entries".into(),
                ));
            }
            if entry.name.eq_ignore_ascii_case("AppxSignature.p7x")
                || entry
                    .name
                    .eq_ignore_ascii_case("AppxMetadata/CodeIntegrity.cat")
            {
                return Err(WriteError::Unsupported(
                    "signed nested package; rebuild it explicitly first".into(),
                ));
            }
            if entry.name.to_ascii_lowercase().starts_with("appxmetadata/") {
                return Err(WriteError::Unsupported(
                    "unsupported nested APPX metadata".into(),
                ));
            }
        }
        for name in &names {
            for (index, _) in name.match_indices('/') {
                if names.contains(&name[..index]) {
                    return Err(WriteError::InvalidInput(
                        "nested package file/directory collision".into(),
                    ));
                }
            }
        }
        let identity = identity(package.manifest())?;
        validate_manifest_profile(package.manifest(), self.options.limits.max_metadata_bytes)?;
        validate_identity(&identity)?;
        let package_type = package_type(package.manifest())?;
        let qualifiers = resource_qualifiers(package.manifest())?;
        if package_type == "resource" {
            validate_resource_package(package.manifest(), &identity, &qualifiers)?;
        }
        if package_type == "resource" && identity.get("ResourceId").is_none_or(String::is_empty) {
            return Err(WriteError::InvalidInput(
                "resource packages require an explicit ResourceId".into(),
            ));
        }
        if ["Name", "Publisher"]
            .iter()
            .any(|key| identity.get(*key) != self.identity.get(*key))
        {
            return Err(WriteError::InvalidInput(
                "nested package identity differs from bundle".into(),
            ));
        }
        let verified = package.validate(self.options.limits.max_total_bytes)?;
        Ok(NestedPackage {
            bytes,
            identity,
            package_type,
            qualifiers,
            decoded_bytes: verified.bytes_verified,
        })
    }

    fn manifest(&self) -> Result<Vec<u8>, WriteError> {
        let mut xml = MetadataWriter::new(self.options.limits.max_metadata_bytes);
        xml.start("Bundle")?;
        xml.attribute("xmlns", "http://schemas.microsoft.com/appx/2013/bundle")?;
        xml.attribute("SchemaVersion", "1.0")?;
        xml.start("Identity")?;
        for key in ["Name", "Publisher", "Version"] {
            xml.attribute(key, &self.identity[key])?;
        }
        xml.end()?;
        xml.start("Packages")?;
        let mut offset = 0u64;
        for (name, package) in &self.packages {
            let id = &package.identity;
            let bytes = &package.bytes;
            offset = offset
                .checked_add(30 + name.len() as u64)
                .ok_or(WriteError::LimitExceeded("bundle offsets"))?;
            let kind = package.package_type;
            let qualifiers = &package.qualifiers;
            let resource = id.get("ResourceId").filter(|id| !id.is_empty());
            xml.start("Package")?;
            xml.attribute("Type", kind)?;
            xml.attribute("Version", &id["Version"])?;
            xml.attribute(
                "Architecture",
                id.get("ProcessorArchitecture")
                    .map_or("neutral", String::as_str),
            )?;
            xml.attribute("FileName", name)?;
            xml.attribute("Offset", &offset.to_string())?;
            xml.attribute("Size", &bytes.len().to_string())?;
            if let Some(id) = resource {
                xml.attribute("ResourceId", id)?;
            }
            if !qualifiers.is_empty() {
                xml.start("Resources")?;
                for attributes in qualifiers {
                    xml.start("Resource")?;
                    for (key, value) in attributes {
                        xml.attribute(key, value)?;
                    }
                    xml.end()?;
                }
                xml.end()?;
            }
            xml.end()?;
            offset = offset
                .checked_add(bytes.len() as u64)
                .ok_or(WriteError::LimitExceeded("bundle offsets"))?;
        }
        xml.end()?;
        xml.end()?;
        xml.finish()
    }

    /// Finalize a new destination. Errors can leave partial destination bytes.
    pub fn write<W: Write + Seek>(self, mut output: W) -> Result<WriteReport, WriteError> {
        if self.packages.is_empty() {
            return Err(WriteError::InvalidInput("empty bundle".into()));
        }
        self.check_budget(&self.packages)?;
        let manifest = self.manifest()?;
        let mut entries: BTreeMap<_, _> = self
            .packages
            .into_iter()
            .map(|(name, package)| (name, package.bytes))
            .collect();
        entries.insert(MANIFEST.into(), manifest);
        let source_bytes = entries.values().try_fold(0u64, |n, v| {
            n.checked_add(v.len() as u64)
                .ok_or(WriteError::LimitExceeded("bundle scratch"))
        })?;
        let mut scratch = Cursor::new(Vec::new());
        let mut options = self.options;
        options.limits.max_scratch_bytes = options
            .limits
            .max_scratch_bytes
            .checked_sub(source_bytes)
            .ok_or(WriteError::LimitExceeded("bundle scratch"))?
            / 2;
        let report = emit_zip(entries, &options, &mut scratch)?;
        let mut bundle = AppxBundle::open(
            Cursor::new(scratch.get_ref()),
            reader_limits(&self.options.limits),
            self.options.limits.max_metadata_bytes,
        )?;
        bundle.validate(self.options.limits.max_total_bytes)?;
        let names: Vec<_> = bundle
            .packages()
            .iter()
            .map(|p| p.file_name.clone())
            .collect();
        for name in names {
            bundle
                .select(
                    &name,
                    reader_limits(&self.options.limits),
                    self.options.limits.max_file_bytes,
                    self.options.limits.max_metadata_bytes,
                )?
                .validate(self.options.limits.max_total_bytes)?;
        }
        output.write_all(scratch.get_ref())?;
        output.flush()?;
        Ok(report)
    }
}

/// Rebuilds supported bundles into a separate destination.
/// Extension-bearing manifests are rejected rather than discarded.
pub struct AppxBundleEditor {
    builder: AppxBundleBuilder,
}

impl AppxBundleEditor {
    /// Open and validate outer and nested integrity before accepting edits.
    /// The initial preservation profile accepts this writer's canonical manifests.
    /// Effective outer content types must also match the writer's profile.
    pub fn open<R: Read + Seek>(source: R, options: WriteOptions) -> Result<Self, WriteError> {
        let bytes = read_bounded(
            source,
            options
                .limits
                .max_output_bytes
                .min(options.limits.max_scratch_bytes / 4),
        )?;
        let mut bundle = AppxBundle::open(
            Cursor::new(&bytes),
            reader_limits(&options.limits),
            options.limits.max_metadata_bytes,
        )?;
        bundle.validate(options.limits.max_total_bytes)?;
        let id = identity(bundle.manifest())?;
        let mut builder = AppxBundleBuilder::new(
            id["Name"].clone(),
            id["Publisher"].clone(),
            id["Version"].clone(),
            options,
        )?;
        let mut archive =
            Archive::open(Cursor::new(&bytes), reader_limits(&builder.options.limits))
                .map_err(crate::Error::from)?;
        for entry in archive.entries() {
            if entry.encrypted || entry.kind != EntryKind::File || entry.compression != "Stored" {
                return Err(WriteError::Unsupported(
                    "bundle input requires stored file entries".into(),
                ));
            }
            if !bundle.packages().iter().any(|p| p.file_name == entry.name)
                && !matches!(
                    entry.name.as_str(),
                    MANIFEST | "AppxBlockMap.xml" | "[Content_Types].xml"
                )
            {
                return Err(WriteError::Unsupported(
                    "unsupported bundle metadata or signature".into(),
                ));
            }
        }
        let types_entry = archive
            .entries()
            .iter()
            .find(|entry| entry.name == "[Content_Types].xml")
            .ok_or_else(|| WriteError::InvalidInput("missing bundle content types".into()))?
            .id;
        let types = archive
            .read_entry(types_entry, builder.options.limits.max_metadata_bytes)
            .map_err(crate::Error::from)?;
        let mappings = resolve_content_types(&types, archive.entries())?;
        if mappings.iter().any(|(name, content_type)| {
            content_type
                != if name == MANIFEST {
                    "application/vnd.ms-appx.bundlemanifest+xml"
                } else {
                    "application/octet-stream"
                }
        }) {
            return Err(WriteError::Unsupported(
                "bundle content-type preservation profile".into(),
            ));
        }
        for declaration in bundle.packages() {
            let entry = archive
                .entries()
                .iter()
                .find(|e| e.name == declaration.file_name)
                .ok_or_else(|| WriteError::InvalidInput("missing nested entry".into()))?;
            let nested = archive
                .read_entry(entry.id, builder.options.limits.max_file_bytes)
                .map_err(crate::Error::from)?;
            builder.add_package(&declaration.file_name, Cursor::new(nested))?;
        }
        if builder.manifest()? != bundle.manifest() {
            return Err(WriteError::Unsupported(
                "bundle manifest preservation profile".into(),
            ));
        }
        Ok(Self { builder })
    }
    /// Add a nested package.
    pub fn add_package(
        &mut self,
        name: impl Into<String>,
        source: impl Read,
    ) -> Result<(), WriteError> {
        self.builder.add_package(name, source)
    }
    /// Replace an existing nested package, preserving the original on failure.
    pub fn replace_package(&mut self, name: &str, source: impl Read) -> Result<(), WriteError> {
        let previous = self
            .builder
            .packages
            .remove(name)
            .ok_or_else(|| WriteError::InvalidInput("unknown nested package".into()))?;
        if let Err(error) = self.builder.add_package(name, source) {
            self.builder.packages.insert(name.into(), previous);
            return Err(error);
        }
        Ok(())
    }
    /// Remove an existing nested package.
    pub fn remove_package(&mut self, name: &str) -> Result<(), WriteError> {
        self.builder
            .packages
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| WriteError::InvalidInput("unknown nested package".into()))
    }
    /// Edit a nested package and rebuild its integrity metadata before replacement.
    /// Failed edits, finalization, or bundle conflicts preserve the previous package.
    pub fn edit_package(
        &mut self,
        name: &str,
        edit: impl FnOnce(&mut AppxEditor) -> Result<(), WriteError>,
    ) -> Result<(), WriteError> {
        let package = self
            .builder
            .packages
            .get(name)
            .ok_or_else(|| WriteError::InvalidInput("unknown nested package".into()))?;
        let retained = self
            .builder
            .packages
            .values()
            .try_fold(0u64, |total, package| {
                total
                    .checked_add(package.bytes.len() as u64)
                    .ok_or(WriteError::LimitExceeded("bundle scratch"))
            })?;
        let mut options = self.builder.options;
        // Reserve the retained bundle plus both staged output copies. Backend
        // opening allocations remain outside the logical scratch guarantee.
        options.limits.max_scratch_bytes = options
            .limits
            .max_scratch_bytes
            .checked_sub(retained)
            .ok_or(WriteError::LimitExceeded("bundle scratch"))?
            / 3;
        let mut editor = AppxEditor::open(Cursor::new(&package.bytes), options)?;
        edit(&mut editor)?;
        let mut output = Cursor::new(Vec::new());
        editor.write(&mut output)?;
        self.replace_package(name, Cursor::new(output.into_inner()))
    }
    /// Rebuild outer metadata after nested packages have been edited separately.
    pub fn write<W: Write + Seek>(self, output: W) -> Result<WriteReport, WriteError> {
        self.builder.write(output)
    }
}

fn identity(bytes: &[u8]) -> Result<BTreeMap<String, String>, WriteError> {
    let document = parse(bytes, bytes.len() as u64)?;
    let root = document.root_element();
    let identities: Vec<_> = root
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Identity")
        .collect();
    if identities.len() != 1 || identities[0].tag_name().namespace() != root.tag_name().namespace()
    {
        return Err(WriteError::InvalidInput(
            "missing or ambiguous package identity".into(),
        ));
    }
    let mut identity = BTreeMap::new();
    for attribute in identities[0].attributes() {
        if attribute.namespace().is_some()
            || !matches!(
                attribute.name(),
                "Name" | "Publisher" | "Version" | "ProcessorArchitecture" | "ResourceId"
            )
        {
            return Err(WriteError::Unsupported(
                "identity extension attributes".into(),
            ));
        }
        identity.insert(attribute.name().into(), attribute.value().into());
    }
    if ["Name", "Publisher", "Version"]
        .iter()
        .any(|key| !identity.contains_key(*key))
    {
        return Err(WriteError::InvalidInput(
            "missing bundle/package identity attribute".into(),
        ));
    }
    Ok(identity)
}

fn validate_identity(identity: &BTreeMap<String, String>) -> Result<(), WriteError> {
    let name = identity.get("Name").map_or("", String::as_str);
    let publisher = identity.get("Publisher").map_or("", String::as_str);
    let version = identity.get("Version").map_or("", String::as_str);
    if !(3..=50).contains(&name.len())
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        || publisher.is_empty()
        || publisher.encode_utf16().count() > 8192
        || publisher.chars().any(char::is_control)
    {
        return Err(WriteError::InvalidInput(
            "invalid bundle/package identity".into(),
        ));
    }
    if version.split('.').count() != 4
        || version.split('.').any(|part| {
            part.is_empty()
                || !part.bytes().all(|b| b.is_ascii_digit())
                || part.parse::<u16>().is_err()
        })
    {
        return Err(WriteError::InvalidInput(
            "invalid bundle/package version".into(),
        ));
    }
    if identity
        .get("ProcessorArchitecture")
        .is_some_and(|architecture| {
            !matches!(
                architecture.as_str(),
                "neutral" | "x86" | "x64" | "arm" | "arm64"
            )
        })
    {
        return Err(WriteError::Unsupported(
            "nested package architecture".into(),
        ));
    }
    if identity.get("ResourceId").is_some_and(|id| {
        id.len() > 30
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
    }) {
        return Err(WriteError::InvalidInput(
            "invalid nested resource identity".into(),
        ));
    }
    Ok(())
}

fn validate_resource_package(
    bytes: &[u8],
    identity: &BTreeMap<String, String>,
    qualifiers: &[BTreeMap<String, String>],
) -> Result<(), WriteError> {
    if identity.contains_key("ProcessorArchitecture") {
        return Err(WriteError::InvalidInput(
            "resource packages must omit ProcessorArchitecture".into(),
        ));
    }
    let document = parse(bytes, bytes.len() as u64)?;
    let root = document.root_element();
    if root.children().any(|node| {
        node.is_element()
            && matches!(
                node.tag_name().name(),
                "Capabilities" | "Applications" | "Extensions"
            )
    }) || root
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "Properties")
        .flat_map(|node| node.children())
        .any(|node| node.is_element() && node.tag_name().name() == "Framework")
    {
        return Err(WriteError::InvalidInput("resource packages cannot declare dependencies, capabilities, applications, extensions, or framework properties".into()));
    }
    for dependencies in root
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "Dependencies")
    {
        let namespace = root.tag_name().namespace();
        if namespace != Some("http://schemas.microsoft.com/appx/manifest/foundation/windows10")
            || dependencies.tag_name().namespace() != namespace
            || dependencies.attributes().len() != 0
            || !dependencies.children().any(|node| node.is_element())
        {
            return Err(WriteError::InvalidInput(
                "resource package compatibility dependencies must contain TargetDeviceFamily"
                    .into(),
            ));
        }
        for family in dependencies.children().filter(|node| node.is_element()) {
            if family.tag_name().namespace() != namespace
                || family.tag_name().name() != "TargetDeviceFamily"
                || family.children().any(|node| node.is_element())
                || family.attributes().any(|attribute| {
                    attribute.namespace().is_some()
                        || !matches!(attribute.name(), "Name" | "MinVersion" | "MaxVersionTested")
                })
                || family.attribute("Name").is_none_or(str::is_empty)
                || ["MinVersion", "MaxVersionTested"].iter().any(|name| {
                    family.attribute(*name).is_none_or(|version| {
                        version.split('.').count() != 4
                            || version.split('.').any(|part| {
                                part.is_empty()
                                    || !part.bytes().all(|byte| byte.is_ascii_digit())
                                    || part.parse::<u16>().is_err()
                            })
                    })
                })
            {
                return Err(WriteError::InvalidInput(
                    "unsupported resource package dependency".into(),
                ));
            }
        }
    }
    let mut kind = None;
    for qualifier in qualifiers {
        if qualifier.len() != 1 {
            return Err(WriteError::InvalidInput(
                "resource package qualifiers must describe one resource kind".into(),
            ));
        }
        let key = qualifier
            .keys()
            .next()
            .ok_or_else(|| WriteError::InvalidInput("empty resource qualifier".into()))?;
        if kind.is_some_and(|previous| previous != key) {
            return Err(WriteError::InvalidInput(
                "resource package qualifiers cannot mix resource kinds".into(),
            ));
        }
        kind = Some(key);
    }
    Ok(())
}

fn package_type(bytes: &[u8]) -> Result<&'static str, WriteError> {
    let document = parse(bytes, bytes.len() as u64)?;
    let root = document.root_element();
    let mut resource = None;
    for node in root
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "ResourcePackage")
    {
        if resource.is_some()
            || node.tag_name().namespace() != root.tag_name().namespace()
            || node.parent().is_none_or(|p| {
                p.tag_name().name() != "Properties"
                    || p.tag_name().namespace() != root.tag_name().namespace()
                    || p.parent() != Some(root)
            })
            || node.attributes().len() != 0
            || node.children().any(|n| n.is_element())
        {
            return Err(WriteError::InvalidInput(
                "ResourcePackage must be a unique Properties child".into(),
            ));
        }
        let text: String = node
            .children()
            .filter(|child| child.is_text())
            .filter_map(|child| child.text())
            .collect();
        resource = Some(match text.trim() {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => {
                return Err(WriteError::InvalidInput(
                    "invalid ResourcePackage property".into(),
                ));
            }
        });
    }
    Ok(if resource == Some(true) {
        "resource"
    } else {
        "application"
    })
}

fn resource_qualifiers(bytes: &[u8]) -> Result<Vec<BTreeMap<String, String>>, WriteError> {
    const UAP: &str = "http://schemas.microsoft.com/appx/manifest/uap/windows10";
    let document = parse(bytes, bytes.len() as u64)?;
    let root = document.root_element();
    let mut resources = Vec::new();
    let mut declarations = BTreeSet::new();
    let containers: Vec<_> = root
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "Resources")
        .collect();
    if containers.len() > 1 {
        return Err(WriteError::Unsupported(
            "multiple Resources declarations".into(),
        ));
    }
    for container in containers {
        if container.parent() != Some(root)
            || container.tag_name().namespace() != root.tag_name().namespace()
            || container.attributes().len() != 0
        {
            return Err(WriteError::Unsupported(
                "unsupported Resources declaration".into(),
            ));
        }
        for node in container.children() {
            if node.is_comment()
                || node.is_pi()
                || node.is_text() && node.text().is_some_and(|text| text.trim().is_empty())
            {
                continue;
            }
            if !node.is_element()
                || node.tag_name().name() != "Resource"
                || node.tag_name().namespace() != root.tag_name().namespace()
                || node.children().any(|n| {
                    n.is_element()
                        || n.is_text() && n.text().is_some_and(|text| !text.trim().is_empty())
                })
            {
                return Err(WriteError::Unsupported(
                    "unsupported resource declaration content".into(),
                ));
            }
            let mut attributes: BTreeMap<String, String> = BTreeMap::new();
            for attribute in node.attributes() {
                let key = attribute.name();
                let value = attribute.value();
                let allowed_namespace =
                    attribute.namespace().is_none() || attribute.namespace() == Some(UAP);
                let valid = match key {
                    "Language" => attribute.namespace().is_none() && valid_language(value),
                    "Scale" => matches!(
                        value,
                        "80" | "100"
                            | "120"
                            | "125"
                            | "140"
                            | "150"
                            | "160"
                            | "175"
                            | "180"
                            | "200"
                            | "225"
                            | "250"
                            | "300"
                            | "350"
                            | "400"
                            | "450"
                    ),
                    "DXFeatureLevel" => matches!(value, "dx9" | "dx10" | "dx11"),
                    _ => false,
                };
                if !allowed_namespace
                    || !valid
                    || attributes.insert(key.into(), value.into()).is_some()
                {
                    return Err(WriteError::Unsupported(
                        "unsupported or invalid resource qualifier".into(),
                    ));
                }
            }
            let semantic_key: Vec<_> = attributes
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        if key == "Language" {
                            value.to_ascii_lowercase()
                        } else {
                            value.clone()
                        },
                    )
                })
                .collect();
            if attributes.is_empty() || !declarations.insert(semantic_key) {
                return Err(WriteError::InvalidInput(
                    "empty or duplicate resource declaration".into(),
                ));
            }
            resources.push(attributes);
        }
    }
    if root.descendants().any(|n| {
        n.is_element()
            && n.tag_name().name() == "Resource"
            && n.parent()
                .is_none_or(|p| p.tag_name().name() != "Resources" || p.parent() != Some(root))
    }) {
        return Err(WriteError::Unsupported(
            "Resource outside Package/Resources".into(),
        ));
    }
    Ok(resources)
}

fn valid_language(value: &str) -> bool {
    // Conservative BCP-47 subset: language, optional script/region, then variants.
    // Extensions, private-use tags and generated placeholders require future support.
    if value.len() > 85 {
        return false;
    }
    let mut parts = value.split('-').peekable();
    let Some(language) = parts.next() else {
        return false;
    };
    if !(2..=8).contains(&language.len()) || !language.bytes().all(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    if parts
        .peek()
        .is_some_and(|part| part.len() == 4 && part.bytes().all(|b| b.is_ascii_alphabetic()))
    {
        parts.next();
    }
    if parts.peek().is_some_and(|part| {
        (part.len() == 2 && part.bytes().all(|b| b.is_ascii_alphabetic()))
            || (part.len() == 3 && part.bytes().all(|b| b.is_ascii_digit()))
    }) {
        parts.next();
    }
    let mut variants = BTreeSet::new();
    parts.all(|part| {
        ((5..=8).contains(&part.len()) || (part.len() == 4 && part.as_bytes()[0].is_ascii_digit()))
            && part.bytes().all(|b| b.is_ascii_alphanumeric())
            && variants.insert(part.to_ascii_lowercase())
    })
}
