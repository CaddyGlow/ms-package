//! Bounded, unsigned bundle rebuilding with independently verified nested packages.
use super::appx::{emit_zip, read_bounded, reader_limits, validate_path};
use super::xml::{MetadataWriter, parse};
use super::{WriteError, WriteOptions, WriteReport};
use crate::{AppxBundle, AppxPackage};
use archive_core::{Archive, EntryKind};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Seek, Write},
};

const MANIFEST: &str = "AppxMetadata/AppxBundleManifest.xml";

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
    packages: BTreeMap<String, Vec<u8>>,
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
        for value in identity.values() {
            if value.is_empty() || value.chars().any(char::is_control) {
                return Err(WriteError::InvalidInput("invalid bundle identity".into()));
            }
        }
        let version = &identity["Version"];
        if version.split('.').count() != 4 || version.split('.').any(|v| v.parse::<u16>().is_err())
        {
            return Err(WriteError::InvalidInput("invalid bundle version".into()));
        }
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
        if name.contains('/') || !(name.ends_with(".appx") || name.ends_with(".msix")) {
            return Err(WriteError::Unsupported(
                "nested packages must have root APPX/MSIX names".into(),
            ));
        }
        if self.packages.keys().any(|n| n.eq_ignore_ascii_case(&name)) {
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
            n.checked_add(v.len() as u64)
                .ok_or(WriteError::LimitExceeded("bundle scratch"))
        })?;
        let remaining = (self.options.limits.max_scratch_bytes / 3)
            .min(self.options.limits.max_total_bytes)
            .checked_sub(used)
            .ok_or(WriteError::LimitExceeded("bundle scratch"))?;
        let bytes = read_bounded(source, self.options.limits.max_file_bytes.min(remaining))?;
        self.validate_nested(&bytes)?;
        self.packages.insert(name.clone(), bytes);
        if let Err(error) = self.check_budget(&self.packages) {
            self.packages.remove(&name);
            return Err(error);
        }
        Ok(())
    }

    fn check_budget(&self, packages: &BTreeMap<String, Vec<u8>>) -> Result<(), WriteError> {
        if packages.len() as u64 + 3 > self.options.limits.max_entries {
            return Err(WriteError::LimitExceeded("bundle entry count"));
        }
        let mut total = 0u64;
        let mut scratch = 0u64;
        let mut identities = BTreeSet::new();
        for bytes in packages.values() {
            scratch = scratch
                .checked_add(bytes.len() as u64)
                .ok_or(WriteError::LimitExceeded("bundle scratch"))?;
            let (identity, decoded) = self.validate_nested(bytes)?;
            total = total
                .checked_add(decoded)
                .and_then(|n| n.checked_add(bytes.len() as u64))
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

    fn validate_nested(&self, bytes: &[u8]) -> Result<(BTreeMap<String, String>, u64), WriteError> {
        let mut package = AppxPackage::open(
            Cursor::new(bytes),
            reader_limits(&self.options.limits),
            self.options.limits.max_metadata_bytes,
        )?;
        for entry in package.entries() {
            validate_path(&entry.name)?;
            if entry.encrypted || entry.kind != EntryKind::File || entry.compression != "Stored" {
                return Err(WriteError::Unsupported(
                    "nested package must use unsigned stored file entries".into(),
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
        let identity = identity(package.manifest())?;
        package_type(package.manifest())?;
        resource_qualifiers(package.manifest())?;
        if ["Name", "Publisher"]
            .iter()
            .any(|key| identity.get(*key) != self.identity.get(*key))
        {
            return Err(WriteError::InvalidInput(
                "nested package identity differs from bundle".into(),
            ));
        }
        let verified = package.validate(self.options.limits.max_total_bytes)?;
        Ok((identity, verified.bytes_verified))
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
        for (name, bytes) in &self.packages {
            let (id, _) = self.validate_nested(bytes)?;
            offset = offset
                .checked_add(30 + name.len() as u64)
                .ok_or(WriteError::LimitExceeded("bundle offsets"))?;
            let package = AppxPackage::open(
                Cursor::new(bytes),
                reader_limits(&self.options.limits),
                self.options.limits.max_metadata_bytes,
            )?;
            let kind = package_type(package.manifest())?;
            let qualifiers = resource_qualifiers(package.manifest())?;
            let resource = id.get("ResourceId").filter(|id| !id.is_empty());
            if kind == "resource" && resource.is_none() {
                return Err(WriteError::Unsupported(
                    "resource packages require an explicit ResourceId".into(),
                ));
            }
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
                        xml.attribute(&key, &value)?;
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
        let mut entries = self.packages;
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
        if attribute.namespace().is_some() {
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
        resource = Some(match node.text().map(str::trim) {
            Some("true") => true,
            Some("false") => false,
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
            let mut attributes = BTreeMap::new();
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
            if attributes.is_empty() || resources.contains(&attributes) {
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
