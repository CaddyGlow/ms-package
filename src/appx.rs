use crate::{Error, Result, safe_name};
use archive_core::{Archive, Entry, EntryId, Limits};
use base64::{Engine, engine::general_purpose::STANDARD};
use quick_xml::{NsReader, name::ResolveResult};
use quick_xml::{Reader, events::Event};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Seek, Write},
};

/// Hashes declared for a decoded package file, in 64-KiB blocks.
#[derive(Clone, Debug)]
pub struct BlockMapFile {
    /// Normalized archive name.
    pub name: String,
    /// Declared decoded file size.
    pub size: u64,
    /// SHA-256 block digests.
    pub hashes: Vec<[u8; 32]>,
}

/// Successful package integrity verification, without a signature trust claim.
#[derive(Clone, Copy, Debug)]
pub struct PackageIntegrity {
    /// Number of payload files verified.
    pub files_verified: usize,
    /// Number of decoded bytes verified.
    pub bytes_verified: u64,
    /// Signature verification is a separate, currently unsupported operation.
    pub signature_verified: bool,
}

/// Explicit nested-package identity declared by a bundle manifest.
#[derive(Clone, Debug)]
pub struct BundlePackage {
    /// Nested ZIP member name.
    pub file_name: String,
    /// Declared processor architecture.
    pub architecture: String,
    /// Resource identity, when present.
    pub resource_id: Option<String>,
    /// Declared package type (application or resource).
    pub package_type: String,
    /// Declared decoded nested-package size.
    pub size: u64,
    /// Declared nested package version.
    pub version: String,
}

/// Read-only bundle inspection with explicit package selection.
pub struct AppxBundle<R: Read + Seek> {
    archive: Archive<R>,
    manifest: Vec<u8>,
    packages: Vec<BundlePackage>,
    identity: BTreeMap<String, String>,
    max_metadata: u64,
}

impl<R: Read + Seek> AppxBundle<R> {
    /// Open a bundle and parse its package identities without automatic selection.
    pub fn open(reader: R, limits: Limits, max_metadata: u64) -> Result<Self> {
        let mut archive = Archive::open(reader, limits)?;
        if archive.format() != archive_core::Format::Zip {
            return Err(Error::Unsupported("bundle requires ZIP".into()));
        }
        let mut archive_names = BTreeSet::new();
        for entry in archive.entries() {
            let name = safe_name(entry.name.trim_end_matches('/'))?;
            if !archive_names.insert(name) || entry.encrypted {
                return Err(Error::Malformed(
                    "duplicate or encrypted bundle entry".into(),
                ));
            }
        }
        let id = archive
            .entries()
            .iter()
            .find(|e| e.name == "AppxMetadata/AppxBundleManifest.xml")
            .ok_or_else(|| Error::Malformed("missing bundle manifest".into()))?
            .id;
        let manifest = archive.read_entry(id, max_metadata)?;
        validate_xml(&manifest, "Bundle")?;
        validate_manifest(&manifest)?;
        let identity = manifest_identity(&manifest)?;
        validate_namespaces(
            &manifest,
            "Bundle",
            &["http://schemas.microsoft.com/appx/2013/bundle"],
            true,
        )?;
        let mut xml = Reader::from_reader(manifest.as_slice());
        let mut packages = Vec::new();
        let mut names = BTreeSet::new();
        let mut parents = Vec::new();
        loop {
            let event = xml
                .read_event()
                .map_err(|e| Error::Malformed(e.to_string()))?;
            let empty = matches!(&event, Event::Empty(_));
            match event {
                Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"Package" => {
                    if parents.len() != 2 || parents.last().is_none_or(|name| name != "Packages") {
                        return Err(Error::Malformed(
                            "bundle Package must be a Packages child".into(),
                        ));
                    }
                    let a = attributes(&e)?;
                    let file_name = safe_name(required(&a, "FileName")?)?;
                    if !names.insert(file_name.clone()) {
                        return Err(Error::Malformed("duplicate bundle package".into()));
                    }
                    let size = required(&a, "Size")?
                        .parse()
                        .map_err(|_| Error::Malformed("bundle package size".into()))?;
                    let entry = archive
                        .entries()
                        .iter()
                        .find(|e| e.name == file_name)
                        .ok_or_else(|| Error::Malformed("missing nested package".into()))?;
                    if entry.size != size
                        || entry.encrypted
                        || entry.kind != archive_core::EntryKind::File
                    {
                        return Err(Error::Malformed("nested package size or encryption".into()));
                    }
                    let architecture = a.get("Architecture").map_or("neutral", String::as_str);
                    if !matches!(architecture, "x86" | "x64" | "arm" | "arm64" | "neutral") {
                        return Err(Error::Malformed("bundle package architecture".into()));
                    }
                    let package_type = a.get("Type").map_or("resource", String::as_str);
                    if !matches!(package_type, "application" | "resource") {
                        return Err(Error::Malformed("bundle package type".into()));
                    }
                    let version = required(&a, "Version")?.to_owned();
                    validate_version(&version)?;
                    packages.push(BundlePackage {
                        file_name,
                        size,
                        architecture: architecture.to_owned(),
                        package_type: package_type.to_owned(),
                        resource_id: a.get("ResourceId").cloned(),
                        version,
                    });
                    if !empty {
                        parents.push("Package".into());
                    }
                }
                Event::Start(e) => {
                    parents.push(String::from_utf8_lossy(e.local_name().as_ref()).into_owned())
                }
                Event::End(_) => {
                    parents.pop();
                }
                Event::Eof => break,
                _ => {}
            }
        }
        if packages.is_empty() {
            return Err(Error::Malformed("empty bundle".into()));
        }
        Ok(Self {
            archive,
            manifest,
            packages,
            identity,
            max_metadata,
        })
    }
    /// Original bundle manifest bytes.
    pub fn manifest(&self) -> &[u8] {
        &self.manifest
    }
    /// Declared nested package identities.
    pub fn packages(&self) -> &[BundlePackage] {
        &self.packages
    }
    /// Verify the outer block map. Nested package integrity requires selecting and
    /// validating each desired package separately; this is not signature trust.
    pub fn validate(&mut self, max_total: u64) -> Result<PackageIntegrity> {
        let content_types = self
            .archive
            .entries()
            .iter()
            .find(|e| e.name == "[Content_Types].xml")
            .ok_or_else(|| Error::Malformed("missing bundle content types".into()))?
            .id;
        let content_types = self.archive.read_entry(content_types, self.max_metadata)?;
        validate_xml(&content_types, "Types")?;
        validate_namespaces(
            &content_types,
            "Types",
            &["http://schemas.openxmlformats.org/package/2006/content-types"],
            true,
        )?;
        validate_content_types(&content_types)?;
        let id = self
            .archive
            .entries()
            .iter()
            .find(|e| e.name == "AppxBlockMap.xml")
            .ok_or_else(|| Error::Malformed("missing bundle block map".into()))?
            .id;
        let data = self.archive.read_entry(id, self.max_metadata)?;
        let blocks = parse_blocks(&data)?;
        let mut total = 0u64;
        let mut covered = BTreeSet::new();
        for block in &blocks {
            total = total
                .checked_add(block.size)
                .ok_or(Error::Limit("bundle decoded bytes"))?;
            if total > max_total {
                return Err(Error::Limit("bundle decoded bytes"));
            }
            let entry = self
                .archive
                .entries()
                .iter()
                .find(|e| e.name == block.name)
                .ok_or_else(|| {
                    Error::Integrity(format!("missing bundle block map file {}", block.name))
                })?;
            if entry.size != block.size {
                return Err(Error::Integrity("bundle block map size".into()));
            }
            let mut verifier = BlockVerifier::new(block);
            self.archive.extract(entry.id, &mut verifier)?;
            verifier.finish()?;
            covered.insert(block.name.as_str());
        }
        for entry in self.archive.entries() {
            if entry.kind == archive_core::EntryKind::Directory
                || covered.contains(entry.name.as_str())
                || self.packages.iter().any(|p| p.file_name == entry.name)
                || matches!(
                    entry.name.as_str(),
                    "AppxBlockMap.xml" | "AppxSignature.p7x" | "[Content_Types].xml"
                )
            {
                continue;
            }
            return Err(Error::Integrity(format!(
                "unmapped bundle metadata {}",
                entry.name
            )));
        }
        Ok(PackageIntegrity {
            files_verified: blocks.len(),
            bytes_verified: total,
            signature_verified: false,
        })
    }
    /// Open exactly one caller-selected nested package with bounded copying.
    pub fn select(
        &mut self,
        file_name: &str,
        limits: Limits,
        max_package: u64,
        max_metadata: u64,
    ) -> Result<AppxPackage<std::io::Cursor<Vec<u8>>>> {
        let declaration = self
            .packages
            .iter()
            .find(|p| p.file_name == file_name)
            .ok_or_else(|| Error::Malformed("undeclared bundle selection".into()))?;
        let entry = self
            .archive
            .entries()
            .iter()
            .find(|e| e.name == file_name)
            .ok_or_else(|| Error::Malformed("missing selected package".into()))?;
        let bytes = self.archive.read_entry(entry.id, max_package)?;
        let package = AppxPackage::open(std::io::Cursor::new(bytes), limits, max_metadata)?;
        let identity = manifest_identity(package.manifest())?;
        if ["Name", "Publisher"]
            .iter()
            .any(|key| identity.get(*key) != self.identity.get(*key))
            || identity.get("Version") != Some(&declaration.version)
            || identity
                .get("ProcessorArchitecture")
                .map_or("neutral", String::as_str)
                != declaration.architecture
            || identity.get("ResourceId").map_or("", String::as_str)
                != declaration.resource_id.as_deref().unwrap_or("")
        {
            return Err(Error::Integrity(
                "nested package identity differs from bundle manifest".into(),
            ));
        }
        Ok(package)
    }
}

/// A single read-only APPX/MSIX package over a ZIP container.
pub struct AppxPackage<R: Read + Seek> {
    archive: Archive<R>,
    manifest: Vec<u8>,
    content_types: Vec<u8>,
    blocks: Vec<BlockMapFile>,
}

impl<R: Read + Seek> AppxPackage<R> {
    /// Parse a single package, preserving original manifest and namespace bytes.
    pub fn open(reader: R, limits: Limits, max_metadata: u64) -> Result<Self> {
        let mut archive = Archive::open(reader, limits)?;
        if archive.format() != archive_core::Format::Zip {
            return Err(Error::Unsupported("APPX/MSIX requires ZIP".into()));
        }
        let mut names = BTreeMap::new();
        for e in archive.entries() {
            if e.encrypted {
                return Err(Error::Unsupported("encrypted APPX/MSIX".into()));
            }
            if names.insert(e.name.clone(), e.id).is_some() {
                return Err(Error::Malformed("duplicate ZIP entry".into()));
            }
        }
        if names.contains_key("AppxMetadata/AppxBundleManifest.xml") {
            return Err(Error::Unsupported(
                "bundle requires explicit nested package selection".into(),
            ));
        }
        let mut metadata = |name: &str| -> Result<Vec<u8>> {
            let id = names
                .get(name)
                .ok_or_else(|| Error::Malformed(format!("missing {name}")))?;
            Ok(archive.read_entry(*id, max_metadata)?)
        };
        let manifest = metadata("AppxManifest.xml")?;
        validate_xml(&manifest, "Package")?;
        validate_namespaces(
            &manifest,
            "Package",
            &[
                "http://schemas.microsoft.com/appx/manifest/foundation/windows10",
                "http://schemas.microsoft.com/appx/2010/manifest",
            ],
            false,
        )?;
        validate_manifest(&manifest)?;
        let content_types = metadata("[Content_Types].xml")?;
        validate_xml(&content_types, "Types")?;
        validate_namespaces(
            &content_types,
            "Types",
            &["http://schemas.openxmlformats.org/package/2006/content-types"],
            true,
        )?;
        validate_content_types(&content_types)?;
        let blocks = parse_blocks(&metadata("AppxBlockMap.xml")?)?;
        Ok(Self {
            archive,
            manifest,
            content_types,
            blocks,
        })
    }
    /// All ZIP entries, including package metadata.
    pub fn entries(&self) -> &[Entry] {
        self.archive.entries()
    }

    /// Stored ZIP timestamps and permissions for an exact package entry.
    pub fn entry_metadata(&mut self, id: EntryId) -> Result<archive_core::EntryMetadata> {
        Ok(self.archive.entry_metadata(id)?)
    }
    /// Original manifest bytes.
    pub fn manifest(&self) -> &[u8] {
        &self.manifest
    }
    /// Original content-types metadata.
    pub fn content_types(&self) -> &[u8] {
        &self.content_types
    }
    /// Parsed block-map records.
    pub fn block_map(&self) -> &[BlockMapFile] {
        &self.blocks
    }
    /// Read a payload using an explicit decoded-byte bound.
    pub fn read_entry(&mut self, id: EntryId, max: u64) -> Result<Vec<u8>> {
        Ok(self.archive.read_entry(id, max)?)
    }
    /// Validate every declared decoded block and require payload coverage.
    pub fn validate(&mut self, max_total: u64) -> Result<PackageIntegrity> {
        let mut total = 0u64;
        let mut covered = BTreeSet::new();
        for file in &self.blocks {
            total = total
                .checked_add(file.size)
                .ok_or(Error::Limit("decoded bytes"))?;
            if total > max_total {
                return Err(Error::Limit("decoded bytes"));
            }
            let e = self
                .archive
                .entries()
                .iter()
                .find(|e| e.name == file.name)
                .ok_or_else(|| Error::Integrity(format!("missing {}", file.name)))?;
            let mut verifier = BlockVerifier::new(file);
            let result = self.archive.extract(e.id, &mut verifier);
            if verifier.failed {
                return Err(Error::Integrity(format!("block hash of {}", file.name)));
            }
            result?;
            verifier.finish()?;
            covered.insert(file.name.as_str());
        }
        for e in self.archive.entries() {
            if e.name.ends_with('/') {
                continue;
            }
            if !covered.contains(e.name.as_str())
                && !matches!(
                    e.name.as_str(),
                    "AppxBlockMap.xml" | "AppxSignature.p7x" | "[Content_Types].xml"
                )
            {
                return Err(Error::Integrity(format!("unmapped payload {}", e.name)));
            }
        }
        Ok(PackageIntegrity {
            files_verified: self.blocks.len(),
            bytes_verified: total,
            signature_verified: false,
        })
    }
}

struct BlockVerifier<'a> {
    file: &'a BlockMapFile,
    hash: Sha256,
    block_bytes: usize,
    block_index: usize,
    total: u64,
    failed: bool,
}
impl<'a> BlockVerifier<'a> {
    fn new(file: &'a BlockMapFile) -> Self {
        Self {
            file,
            hash: Sha256::new(),
            block_bytes: 0,
            block_index: 0,
            total: 0,
            failed: false,
        }
    }
    fn verify_block(&mut self) -> std::io::Result<()> {
        let actual: [u8; 32] = self.hash.finalize_reset().into();
        if self.file.hashes.get(self.block_index) != Some(&actual) {
            self.failed = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "package block hash mismatch",
            ));
        }
        self.block_index += 1;
        self.block_bytes = 0;
        Ok(())
    }
    fn finish(&mut self) -> Result<()> {
        if self.block_bytes != 0 {
            self.verify_block()
                .map_err(|_| Error::Integrity(format!("block hash of {}", self.file.name)))?;
        }
        if self.total != self.file.size || self.block_index != self.file.hashes.len() {
            return Err(Error::Integrity(format!(
                "size or block count of {}",
                self.file.name
            )));
        }
        Ok(())
    }
}
impl Write for BlockVerifier<'_> {
    fn write(&mut self, mut bytes: &[u8]) -> std::io::Result<usize> {
        let count = bytes.len();
        self.total = self
            .total
            .checked_add(count as u64)
            .ok_or_else(|| std::io::Error::other("package byte count overflow"))?;
        if self.total > self.file.size {
            self.failed = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "package file size mismatch",
            ));
        }
        while !bytes.is_empty() {
            let take = bytes.len().min(65536 - self.block_bytes);
            self.hash.update(&bytes[..take]);
            self.block_bytes += take;
            bytes = &bytes[take..];
            if self.block_bytes == 65536 {
                self.verify_block()?;
            }
        }
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn attributes(e: &quick_xml::events::BytesStart<'_>) -> Result<BTreeMap<String, String>> {
    let mut attrs = BTreeMap::new();
    for a in e.attributes() {
        let a = a.map_err(|e| Error::Malformed(e.to_string()))?;
        let key = String::from_utf8(a.key.as_ref().to_vec())
            .map_err(|e| Error::Malformed(e.to_string()))?;
        let value = a
            .unescape_value()
            .map_err(|e| Error::Malformed(e.to_string()))?
            .into_owned();
        if attrs.insert(key, value).is_some() {
            return Err(Error::Malformed("duplicate XML attribute".into()));
        }
    }
    Ok(attrs)
}

fn required<'a>(a: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str> {
    a.get(name)
        .map(String::as_str)
        .ok_or_else(|| Error::Malformed(format!("missing {name}")))
}

fn validate_xml(data: &[u8], root: &str) -> Result<()> {
    let mut reader = Reader::from_reader(data);
    let mut seen = false;
    let mut depth = 0usize;
    loop {
        match reader
            .read_event()
            .map_err(|e| Error::Malformed(e.to_string()))?
        {
            Event::Start(e) => {
                if depth == 0 {
                    if seen || e.local_name().as_ref() != root.as_bytes() {
                        return Err(Error::Malformed(format!("expected {root} root")));
                    }
                    seen = true;
                }
                depth += 1;
                if depth > 128 {
                    return Err(Error::Limit("XML depth"));
                }
                attributes(&e)?;
            }
            Event::Empty(e) => {
                if depth == 0 {
                    if seen || e.local_name().as_ref() != root.as_bytes() {
                        return Err(Error::Malformed(format!("expected {root} root")));
                    }
                    seen = true;
                }
                attributes(&e)?;
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| Error::Malformed("XML depth".into()))?;
            }
            Event::DocType(_) => return Err(Error::Unsupported("XML document types".into())),
            Event::Eof => break,
            _ => {}
        }
    }
    if !seen || depth != 0 {
        return Err(Error::Malformed("incomplete XML".into()));
    }
    Ok(())
}

fn validate_manifest(data: &[u8]) -> Result<()> {
    let mut reader = Reader::from_reader(data);
    let mut identities = 0;
    let mut depth = 0usize;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| Error::Malformed(e.to_string()))?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                if e.local_name().as_ref() == b"Identity" {
                    if depth != 1 {
                        return Err(Error::Malformed(
                            "Identity must be a direct Package child".into(),
                        ));
                    }
                    identities += 1;
                    let a = attributes(&e)?;
                    for key in ["Name", "Publisher", "Version"] {
                        if required(&a, key)?.is_empty() {
                            return Err(Error::Malformed(format!("empty identity {key}")));
                        }
                    }
                    validate_version(required(&a, "Version")?)?;
                }
                if !empty {
                    depth += 1;
                }
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| Error::Malformed("manifest depth".into()))?
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if identities != 1 {
        return Err(Error::Malformed("single package Identity required".into()));
    }
    Ok(())
}

fn validate_version(version: &str) -> Result<()> {
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 4 || parts.iter().any(|v| v.parse::<u16>().is_err()) {
        return Err(Error::Malformed("package version".into()));
    }
    Ok(())
}

fn manifest_identity(data: &[u8]) -> Result<BTreeMap<String, String>> {
    let mut reader = Reader::from_reader(data);
    loop {
        match reader
            .read_event()
            .map_err(|e| Error::Malformed(e.to_string()))?
        {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"Identity" => {
                return attributes(&e);
            }
            Event::Eof => return Err(Error::Malformed("missing package identity".into())),
            _ => {}
        }
    }
}

fn validate_namespaces(
    data: &[u8],
    root: &str,
    allowed: &[&str],
    all_elements: bool,
) -> Result<()> {
    let mut reader = NsReader::from_reader(data);
    let mut seen = false;
    loop {
        let (namespace, event) = reader
            .read_resolved_event()
            .map_err(|e| Error::Malformed(e.to_string()))?;
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let enforce = all_elements || !seen || e.local_name().as_ref() == b"Identity";
                if enforce {
                    match namespace {
                        ResolveResult::Bound(ns)
                            if allowed.iter().any(|a| ns.as_ref() == a.as_bytes()) => {}
                        _ => {
                            return Err(Error::Malformed(format!(
                                "unsupported or unbound {root} namespace"
                            )));
                        }
                    }
                }
                seen = true;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

fn validate_content_types(data: &[u8]) -> Result<()> {
    let mut reader = Reader::from_reader(data);
    let mut names = BTreeSet::new();
    loop {
        match reader
            .read_event()
            .map_err(|e| Error::Malformed(e.to_string()))?
        {
            Event::Start(e) | Event::Empty(e) => {
                let a = attributes(&e)?;
                let key = match e.local_name().as_ref() {
                    b"Types" => continue,
                    b"Default" => format!("extension:{}", required(&a, "Extension")?),
                    b"Override" => format!("part:{}", required(&a, "PartName")?),
                    _ => return Err(Error::Malformed("content-types element".into())),
                };
                if required(&a, "ContentType")?.is_empty() || !names.insert(key) {
                    return Err(Error::Malformed("duplicate or empty content type".into()));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if names.is_empty() {
        return Err(Error::Malformed("empty content types".into()));
    }
    Ok(())
}

fn parse_blocks(data: &[u8]) -> Result<Vec<BlockMapFile>> {
    validate_xml(data, "BlockMap")?;
    validate_namespaces(
        data,
        "BlockMap",
        &["http://schemas.microsoft.com/appx/2010/blockmap"],
        true,
    )?;
    let mut reader = Reader::from_reader(data);
    let mut files = Vec::new();
    let mut current: Option<BlockMapFile> = None;
    let mut names = BTreeSet::new();
    loop {
        let event = reader
            .read_event()
            .map_err(|e| Error::Malformed(e.to_string()))?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let a = attributes(&e)?;
                match e.local_name().as_ref() {
                    b"BlockMap" => {
                        if required(&a, "HashMethod")? != "http://www.w3.org/2001/04/xmlenc#sha256"
                        {
                            return Err(Error::Unsupported("block-map hash method".into()));
                        }
                    }
                    b"File" => {
                        if current.is_some() {
                            return Err(Error::Malformed("nested block-map File".into()));
                        }
                        let name = safe_name(required(&a, "Name")?)?;
                        if !names.insert(name.clone()) {
                            return Err(Error::Malformed("duplicate block-map file".into()));
                        }
                        let size = required(&a, "Size")?
                            .parse()
                            .map_err(|_| Error::Malformed("block-map size".into()))?;
                        current = Some(BlockMapFile {
                            name,
                            size,
                            hashes: Vec::new(),
                        });
                        if empty {
                            files.push(
                                current
                                    .take()
                                    .ok_or_else(|| Error::Malformed("file state".into()))?,
                            );
                        }
                    }
                    b"Block" => {
                        let file = current
                            .as_mut()
                            .ok_or_else(|| Error::Malformed("Block outside File".into()))?;
                        let hash = STANDARD
                            .decode(required(&a, "Hash")?)
                            .map_err(|e| Error::Malformed(e.to_string()))?;
                        file.hashes.push(
                            hash.try_into()
                                .map_err(|_| Error::Malformed("SHA-256 digest length".into()))?,
                        );
                    }
                    _ => return Err(Error::Unsupported("block-map element".into())),
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"File" => files.push(
                current
                    .take()
                    .ok_or_else(|| Error::Malformed("File end".into()))?,
            ),
            Event::Eof => break,
            _ => {}
        }
    }
    for file in &files {
        if file.size.div_ceil(65536) != file.hashes.len() as u64 {
            return Err(Error::Malformed(format!("block count of {}", file.name)));
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_duplicate_block_map_files() {
        let xml = br#"<BlockMap xmlns="http://schemas.microsoft.com/appx/2010/blockmap" HashMethod="http://www.w3.org/2001/04/xmlenc#sha256"><File Name="x" Size="0"/><File Name="x" Size="0"/></BlockMap>"#;
        assert!(parse_blocks(xml).is_err());
    }
    #[test]
    fn rejects_traversing_block_map_names() {
        let xml = br#"<BlockMap xmlns="http://schemas.microsoft.com/appx/2010/blockmap" HashMethod="http://www.w3.org/2001/04/xmlenc#sha256"><File Name="..\x" Size="0"/></BlockMap>"#;
        assert!(parse_blocks(xml).is_err());
    }
    #[test]
    fn rejects_malformed_manifest_and_document_type() {
        assert!(validate_xml(b"<Package><Identity></Package>", "Package").is_err());
        assert!(validate_xml(b"<!DOCTYPE Package><Package/>", "Package").is_err());
        assert!(validate_manifest(b"<Package/>").is_err());
        assert!(validate_manifest(br#"<Package><Dependencies><Identity Name="X" Publisher="CN=X" Version="1.0.0.0"/></Dependencies></Package>"#).is_err());
        assert!(validate_namespaces(br#"<Package xmlns="invalid"><Identity Name="X" Publisher="CN=X" Version="1.0.0.0"/></Package>"#, "Package", &["http://schemas.microsoft.com/appx/manifest/foundation/windows10"],false).is_err());
    }
    #[test]
    fn streaming_block_hashes_verify_arbitrary_chunks_without_buffering_files() {
        let data = vec![42; 2 * 65536 + 13];
        let file = BlockMapFile {
            name: "big.bin".into(),
            size: data.len() as u64,
            hashes: data
                .chunks(65536)
                .map(|b| Sha256::digest(b).into())
                .collect(),
        };
        let mut verifier = BlockVerifier::new(&file);
        for chunk in data.chunks(997) {
            verifier.write_all(chunk).unwrap();
        }
        verifier.finish().unwrap();
        let mut bad = data;
        bad[65536 + 10] ^= 1;
        let mut verifier = BlockVerifier::new(&file);
        assert!(verifier.write_all(&bad).is_err());
    }
    #[test]
    fn validates_decoded_payload_and_detects_corruption() {
        use std::io::{Cursor, Write};
        use zip::{ZipWriter, write::SimpleFileOptions};
        let manifest = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Test" Publisher="CN=Test" Version="1.0.0.0" ProcessorArchitecture="x64"/></Package>"#;
        let payload = b"hello";
        let hash = |data: &[u8]| STANDARD.encode(Sha256::digest(data));
        let blockmap = format!(
            "<BlockMap xmlns=\"http://schemas.microsoft.com/appx/2010/blockmap\" HashMethod=\"http://www.w3.org/2001/04/xmlenc#sha256\"><File Name=\"AppxManifest.xml\" Size=\"{}\"><Block Hash=\"{}\"/></File><File Name=\"hello.txt\" Size=\"5\"><Block Hash=\"{}\"/></File></BlockMap>",
            manifest.len(),
            hash(manifest),
            hash(payload)
        );
        for corrupt in [false, true] {
            let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
            for (name, data) in [
                ("AppxManifest.xml", manifest.as_slice()),
                (
                    "[Content_Types].xml",
                    b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>"
                        .as_slice(),
                ),
                ("AppxBlockMap.xml", blockmap.as_bytes()),
                ("hello.txt", if corrupt { b"wrong" } else { payload }),
            ] {
                zip.start_file(name, SimpleFileOptions::default()).unwrap();
                zip.write_all(data).unwrap();
            }
            let bytes = zip.finish().unwrap().into_inner();
            let bundle_manifest = format!(
                "<Bundle xmlns=\"http://schemas.microsoft.com/appx/2013/bundle\"><Identity Name=\"Test\" Publisher=\"CN=Test\" Version=\"1.0.0.0\"/><Packages><Package FileName=\"test.msix\" Size=\"{}\" Version=\"1.0.0.0\" Architecture=\"x64\" Type=\"application\"/></Packages></Bundle>",
                bytes.len()
            );
            let mut bundle_zip = ZipWriter::new(Cursor::new(Vec::new()));
            bundle_zip
                .start_file(
                    "AppxMetadata/AppxBundleManifest.xml",
                    SimpleFileOptions::default(),
                )
                .unwrap();
            bundle_zip.write_all(bundle_manifest.as_bytes()).unwrap();
            bundle_zip
                .start_file("test.msix", SimpleFileOptions::default())
                .unwrap();
            bundle_zip.write_all(&bytes).unwrap();
            let mut bundle = AppxBundle::open(
                Cursor::new(bundle_zip.finish().unwrap().into_inner()),
                Limits::default(),
                1024 * 1024,
            )
            .unwrap();
            assert_eq!(bundle.packages()[0].architecture, "x64");
            assert!(
                bundle
                    .select("missing.msix", Limits::default(), 1024 * 1024, 1024 * 1024)
                    .is_err()
            );
            assert!(
                bundle
                    .select("test.msix", Limits::default(), 1, 1024 * 1024)
                    .is_err()
            );
            let mut selected = bundle
                .select("test.msix", Limits::default(), 1024 * 1024, 1024 * 1024)
                .unwrap();
            assert_eq!(selected.validate(1024 * 1024).is_err(), corrupt);
            let mut package =
                AppxPackage::open(Cursor::new(bytes), Limits::default(), 1024 * 1024).unwrap();
            assert_eq!(package.manifest(), manifest);
            assert_eq!(package.validate(1024 * 1024).is_err(), corrupt);
        }
    }
}
