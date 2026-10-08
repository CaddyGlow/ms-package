//! Experimental file-only MSI coordination. Windows lifecycle qualification is pending.

use std::io::{Cursor, Read, Write};

use super::{InstallerDatabaseBuilder, InstallerWriteReport, WriteError, WriteOptions};

/// Explicit target architecture for the narrow file-only profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallerArchitecture {
    /// 32-bit components and ProgramFilesFolder.
    X86,
    /// 64-bit components and ProgramFiles64Folder.
    X64,
}

/// Explicit installation context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallationContext {
    /// Files below the user's LocalAppDataFolder; no elevation required.
    PerUser,
    /// Files below Program Files; elevation may be required.
    PerMachine,
}

/// Caller-owned identity and directory policy. No GUID is generated automatically.
#[derive(Clone, Debug)]
pub struct InstallerIdentity {
    /// Canonical braced product GUID.
    pub product_code: String,
    /// Canonical braced package GUID; supply a new code for a changed distributable.
    pub package_code: String,
    /// Canonical braced upgrade family GUID. Upgrade behavior is not qualified.
    pub upgrade_code: String,
    /// Display name.
    pub name: String,
    /// Manufacturer.
    pub manufacturer: String,
    /// Three-field MSI version (major/minor at most 255, build at most 65535).
    pub version: String,
    /// Installation directory leaf below the selected standard folder.
    pub directory_name: String,
    /// Target architecture.
    pub architecture: InstallerArchitecture,
    /// Installation context.
    pub context: InstallationContext,
}

#[derive(Clone)]
struct File {
    id: String,
    name: String,
    component_guid: String,
    bytes: Vec<u8>,
}

/// Coordinates one embedded stored cabinet, one feature, and one file per component.
///
/// This experimental profile contains no authored custom actions or UI. It accepts
/// unversioned data files, a flat directory, and explicit component GUIDs/key paths.
/// PE files and arbitrary payload edits require qualified version/servicing support
/// and are rejected. Structural round trips do not establish Windows installability.
#[derive(Clone)]
pub struct InstallerBuilder {
    identity: InstallerIdentity,
    options: WriteOptions,
    files: Vec<File>,
    total: u64,
}

fn invalid(message: &str) -> WriteError {
    WriteError::InvalidInput(message.into())
}

fn guid(value: &str) -> Result<uuid::Uuid, WriteError> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| invalid("invalid MSI GUID"))?;
    if parsed.is_nil() || parsed.braced().to_string().to_ascii_uppercase() != value {
        return Err(invalid("MSI GUID must be non-nil, uppercase, and braced"));
    }
    Ok(parsed)
}

fn leaf(value: &str) -> Result<(), WriteError> {
    if value.is_empty()
        || value.len() > 128
        || !value.is_ascii()
        || value.ends_with(['.', ' '])
        || value
            .chars()
            .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
        || matches!(value, "." | "..")
    {
        return Err(invalid(
            "MSI file and directory names must be safe ASCII leaves",
        ));
    }
    let stem = value.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(invalid("reserved Windows leaf name"));
    }
    Ok(())
}

impl InstallerBuilder {
    /// Validates explicit identities and a flat standard-folder installation policy.
    pub fn new(identity: InstallerIdentity, options: WriteOptions) -> Result<Self, WriteError> {
        guid(&identity.product_code)?;
        guid(&identity.package_code)?;
        guid(&identity.upgrade_code)?;
        if identity.product_code == identity.package_code
            || identity.product_code == identity.upgrade_code
            || identity.package_code == identity.upgrade_code
        {
            return Err(invalid(
                "product, package, and upgrade GUIDs must be distinct",
            ));
        }
        leaf(&identity.directory_name)?;
        if identity.name.is_empty() || identity.manufacturer.is_empty() {
            return Err(invalid("product name and manufacturer are required"));
        }
        let version: Vec<_> = identity.version.split('.').collect();
        if version.len() != 3
            || version
                .iter()
                .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
            || version[0].parse::<u8>().is_err()
            || version[1].parse::<u8>().is_err()
            || version[2].parse::<u16>().is_err()
        {
            return Err(invalid(
                "MSI product version requires major.minor.build within 255.255.65535",
            ));
        }
        let metadata = [
            &identity.product_code,
            &identity.package_code,
            &identity.upgrade_code,
            &identity.name,
            &identity.manufacturer,
            &identity.version,
            &identity.directory_name,
        ]
        .iter()
        .try_fold(0u64, |total, value| total.checked_add(value.len() as u64));
        if metadata.is_none_or(|total| total > options.limits.max_metadata_bytes) {
            return Err(WriteError::LimitExceeded("MSI identity metadata"));
        }
        Ok(Self {
            identity,
            options,
            files: Vec::new(),
            total: 0,
        })
    }

    /// Adds an unversioned data file with its explicit, stable component GUID.
    /// The file identifier is also the cabinet member and component key path.
    pub fn add_file(
        &mut self,
        id: &str,
        name: &str,
        component_guid: &str,
        mut source: impl Read,
    ) -> Result<(), WriteError> {
        if id.is_empty()
            || id.len() > 60
            || !id.as_bytes()[0].is_ascii_alphabetic()
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(invalid(
                "file identifier must be an ASCII MSI identifier of at most 60 characters",
            ));
        }
        leaf(name)?;
        guid(component_guid)?;
        if self.files.iter().any(|file| {
            file.id.eq_ignore_ascii_case(id)
                || file.name.eq_ignore_ascii_case(name)
                || file.component_guid == component_guid
        }) {
            return Err(invalid(
                "duplicate file identifier, target name, or component GUID",
            ));
        }
        if self.files.len() as u64 >= self.options.limits.max_entries || self.files.len() >= 32767 {
            return Err(WriteError::LimitExceeded("MSI file count"));
        }
        let available = self
            .options
            .limits
            .max_total_bytes
            .min(self.options.limits.max_scratch_bytes)
            .checked_sub(self.total)
            .ok_or(WriteError::LimitExceeded("MSI payload bytes"))?;
        let limit = available
            .min(self.options.limits.max_file_bytes)
            .min(i32::MAX as u64);
        let mut bytes = Vec::new();
        source
            .by_ref()
            .take(limit.saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(WriteError::LimitExceeded("MSI payload bytes"));
        }
        if bytes.starts_with(b"MZ") {
            return Err(WriteError::Unsupported(
                "PE version/language inspection is not qualified by the unversioned data profile"
                    .into(),
            ));
        }
        self.total += bytes.len() as u64;
        self.files.push(File {
            id: id.into(),
            name: name.into(),
            component_guid: component_guid.into(),
            bytes,
        });
        Ok(())
    }

    /// Builds tables, sequence actions, and an embedded cabinet, then explicitly
    /// finalizes all backends. Requires independent Windows validation before use.
    pub fn write(self, destination: impl Write) -> Result<InstallerWriteReport, WriteError> {
        if self.files.is_empty() {
            return Err(invalid(
                "the file-only MSI profile requires at least one file",
            ));
        }
        let mut cabinet = cabinet::CabinetBuilder::new(cabinet::WriteCompression::None);
        for file in &self.files {
            cabinet.add_file(&file.id, &file.bytes)?;
        }
        let scratch_limit = self
            .options
            .limits
            .max_scratch_bytes
            .checked_sub(self.total)
            .ok_or(WriteError::LimitExceeded("MSI cabinet scratch"))?;
        let mut cabinet_output = super::installer::BoundedCursor {
            inner: Cursor::new(Vec::new()),
            limit: scratch_limit
                .min(self.options.limits.max_file_bytes)
                .min(self.options.limits.max_output_bytes),
        };
        cabinet.write(&mut cabinet_output)?;
        cabinet_output.flush()?;
        let cabinet_bytes = cabinet_output.inner.into_inner();
        let mut options = self.options;
        options.limits.max_scratch_bytes = options
            .limits
            .max_scratch_bytes
            .checked_sub(self.total)
            .and_then(|left| left.checked_sub(cabinet_bytes.len() as u64))
            .ok_or(WriteError::LimitExceeded("MSI database scratch"))?;
        let mut database = InstallerDatabaseBuilder::new(options)?;
        database.set_database_codepage(msi::CodePage::Windows1252)?;
        let package_code = guid(&self.identity.package_code)?;
        database.edit_summary(|summary| {
            summary.set_codepage(msi::CodePage::Windows1252);
            summary.set_uuid(package_code);
            summary.set_arch(match self.identity.architecture {
                InstallerArchitecture::X86 => "Intel",
                InstallerArchitecture::X64 => "x64",
            });
            summary.set_languages(&[msi::Language::from_code(1033)]);
            summary.set_author(&self.identity.manufacturer);
            summary.set_subject(&self.identity.name);
            summary.set_creating_application("ms-package experimental file-only authoring");
            summary.set_word_count(if self.identity.context == InstallationContext::PerUser {
                10
            } else {
                2
            });
            summary.set_page_count(400);
        })?;
        create_schema(&mut database)?;
        let mut properties = vec![
            vec![s("ProductCode"), s(&self.identity.product_code)],
            vec![s("UpgradeCode"), s(&self.identity.upgrade_code)],
            vec![s("ProductName"), s(&self.identity.name)],
            vec![s("Manufacturer"), s(&self.identity.manufacturer)],
            vec![s("ProductVersion"), s(&self.identity.version)],
            vec![s("ProductLanguage"), s("1033")],
            vec![s("INSTALLLEVEL"), s("1")],
        ];
        if self.identity.context == InstallationContext::PerMachine {
            properties.push(vec![s("ALLUSERS"), s("1")]);
        }
        database.insert_rows("Property", properties)?;
        let parent = match (self.identity.context, self.identity.architecture) {
            (InstallationContext::PerUser, _) => "LocalAppDataFolder",
            (InstallationContext::PerMachine, InstallerArchitecture::X86) => "ProgramFilesFolder",
            (InstallationContext::PerMachine, InstallerArchitecture::X64) => "ProgramFiles64Folder",
        };
        database.insert_rows(
            "Directory",
            vec![
                vec![s("TARGETDIR"), n(), s("SourceDir")],
                vec![s(parent), s("TARGETDIR"), s(".")],
                vec![s("INSTALLDIR"), s(parent), s(&self.identity.directory_name)],
            ],
        )?;
        database.insert_rows(
            "Feature",
            vec![vec![
                s("Main"),
                n(),
                s(&self.identity.name),
                n(),
                i(1),
                i(1),
                s("INSTALLDIR"),
                i(0),
            ]],
        )?;
        for (index, file) in self.files.iter().enumerate() {
            let component = format!("C_{}", file.id);
            database.insert_rows(
                "Component",
                vec![vec![
                    s(&component),
                    s(&file.component_guid),
                    s("INSTALLDIR"),
                    i(
                        if self.identity.architecture == InstallerArchitecture::X64 {
                            256
                        } else {
                            0
                        },
                    ),
                    n(),
                    s(&file.id),
                ]],
            )?;
            database.insert_rows("FeatureComponents", vec![vec![s("Main"), s(&component)]])?;
            database.insert_rows(
                "File",
                vec![vec![
                    s(&file.id),
                    s(&component),
                    s(&file.name),
                    i(file.bytes.len() as i32),
                    n(),
                    n(),
                    i(0),
                    i(index as i32 + 1),
                ]],
            )?;
        }
        database.insert_rows(
            "Media",
            vec![vec![
                i(1),
                i(self.files.len() as i32),
                n(),
                s("#payload.cab"),
                n(),
                n(),
            ]],
        )?;
        let actions = [
            ("ValidateProductID", 700),
            ("CostInitialize", 800),
            ("FileCost", 900),
            ("CostFinalize", 1000),
            ("InstallValidate", 1400),
            ("InstallInitialize", 1500),
            ("ProcessComponents", 1600),
            ("UnpublishFeatures", 1800),
            ("RemoveFiles", 3500),
            ("RemoveFolders", 3600),
            ("CreateFolders", 3700),
            ("InstallFiles", 4000),
            ("RegisterUser", 6000),
            ("RegisterProduct", 6100),
            ("PublishFeatures", 6300),
            ("PublishProduct", 6400),
            ("InstallFinalize", 6600),
        ];
        database.insert_rows(
            "InstallExecuteSequence",
            actions
                .into_iter()
                .map(|(action, sequence)| vec![s(action), n(), i(sequence)])
                .collect(),
        )?;
        database.write_stream("payload.cab", Cursor::new(cabinet_bytes))?;
        database.write(destination)
    }
}

fn s(value: &str) -> msi::Value {
    msi::Value::Str(value.into())
}
fn i(value: i32) -> msi::Value {
    msi::Value::Int(value)
}
fn n() -> msi::Value {
    msi::Value::Null
}
fn key(name: &str) -> msi::Column {
    msi::Column::build(name).primary_key().id_string(72)
}
fn text(name: &str, length: usize, nullable: bool) -> msi::Column {
    let column = msi::Column::build(name);
    if nullable {
        column.nullable().string(length)
    } else {
        column.string(length)
    }
}
fn number(name: &str, nullable: bool, wide: bool) -> msi::Column {
    let column = msi::Column::build(name);
    let column = if nullable { column.nullable() } else { column };
    if wide { column.int32() } else { column.int16() }
}
fn reference(name: &str, table: &str, nullable: bool) -> msi::Column {
    let column = msi::Column::build(name).foreign_key(table, 1);
    if nullable {
        column.nullable().id_string(72)
    } else {
        column.id_string(72)
    }
}

fn create_schema(database: &mut InstallerDatabaseBuilder) -> Result<(), WriteError> {
    database.create_table("Property", vec![key("Property"), text("Value", 0, false)])?;
    database.create_table(
        "Directory",
        vec![
            key("Directory"),
            reference("Directory_Parent", "Directory", true),
            text("DefaultDir", 255, false),
        ],
    )?;
    database.create_table(
        "Component",
        vec![
            key("Component"),
            text("ComponentId", 38, true),
            reference("Directory_", "Directory", false),
            number("Attributes", false, false),
            text("Condition", 255, true),
            text("KeyPath", 72, true),
        ],
    )?;
    database.create_table(
        "Feature",
        vec![
            msi::Column::build("Feature").primary_key().id_string(38),
            reference("Feature_Parent", "Feature", true),
            text("Title", 64, true),
            text("Description", 255, true),
            number("Display", true, false),
            number("Level", false, false),
            reference("Directory_", "Directory", true),
            number("Attributes", false, false),
        ],
    )?;
    database.create_table(
        "FeatureComponents",
        vec![
            msi::Column::build("Feature_")
                .primary_key()
                .foreign_key("Feature", 1)
                .id_string(38),
            msi::Column::build("Component_")
                .primary_key()
                .foreign_key("Component", 1)
                .id_string(72),
        ],
    )?;
    database.create_table(
        "File",
        vec![
            key("File"),
            reference("Component_", "Component", false),
            text("FileName", 255, false),
            number("FileSize", false, true),
            text("Version", 72, true),
            text("Language", 20, true),
            number("Attributes", true, false),
            number("Sequence", false, false),
        ],
    )?;
    database.create_table(
        "Media",
        vec![
            msi::Column::build("DiskId").primary_key().int16(),
            number("LastSequence", false, false),
            text("DiskPrompt", 64, true),
            text("Cabinet", 255, true),
            text("VolumeLabel", 32, true),
            text("Source", 72, true),
        ],
    )?;
    database.create_table(
        "InstallExecuteSequence",
        vec![
            key("Action"),
            text("Condition", 255, true),
            number("Sequence", true, false),
        ],
    )?;
    Ok(())
}

/// Rebuilds only canonical databases emitted by `InstallerBuilder`.
///
/// Admission compares every compound-file stream with a regenerated canonical
/// source, rejecting custom actions, unknown streams, additional relationships,
/// changed schemas, and summary metadata that cannot be preserved. Replacements
/// retain component identity and key paths. Renaming or removing a component can
/// change servicing behavior; these experimental operations have no upgrade claim.
pub struct InstallerPayloadEditor {
    builder: InstallerBuilder,
}

impl InstallerPayloadEditor {
    /// Opens the narrow canonical profile, requiring a distinct new PackageCode.
    pub fn open(
        mut source: impl Read,
        new_package_code: &str,
        options: WriteOptions,
    ) -> Result<Self, WriteError> {
        guid(new_package_code)?;
        let mut bytes = Vec::new();
        let source_limit = options.limits.max_scratch_bytes / 2;
        source
            .by_ref()
            .take(source_limit.saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > source_limit {
            return Err(WriteError::LimitExceeded(
                "MSI payload editor source scratch",
            ));
        }
        let mut validation_options = options;
        validation_options.limits.max_scratch_bytes -= bytes.len() as u64;
        drop(super::InstallerEditor::open(
            Cursor::new(&bytes),
            validation_options,
        )?);
        let mut package = msi::Package::open(Cursor::new(&bytes))?;
        let mut properties = std::collections::BTreeMap::new();
        for row in package.select_rows(msi::Select::table("Property"))? {
            let key = row["Property"]
                .as_str()
                .ok_or_else(|| invalid("canonical property key"))?
                .to_owned();
            let value = row["Value"]
                .as_str()
                .ok_or_else(|| invalid("canonical property value"))?
                .to_owned();
            properties.insert(key, value);
        }
        let property = |name: &str| {
            properties
                .get(name)
                .cloned()
                .ok_or_else(|| invalid("canonical product properties missing"))
        };
        let architecture = match package.summary_info().arch() {
            Some("Intel") => InstallerArchitecture::X86,
            Some("x64") => InstallerArchitecture::X64,
            _ => {
                return Err(WriteError::Unsupported(
                    "noncanonical MSI architecture".into(),
                ));
            }
        };
        let context = match properties.get("ALLUSERS").map(String::as_str) {
            Some("1") => InstallationContext::PerMachine,
            None => InstallationContext::PerUser,
            _ => return Err(WriteError::Unsupported("noncanonical MSI context".into())),
        };
        let old_package_code = package
            .summary_info()
            .uuid()
            .ok_or_else(|| invalid("missing PackageCode"))?
            .braced()
            .to_string()
            .to_ascii_uppercase();
        if old_package_code == new_package_code {
            return Err(invalid("payload edits require a distinct new PackageCode"));
        }
        let directory_name = package
            .select_rows(msi::Select::table("Directory"))?
            .find(|row| row["Directory"].as_str() == Some("INSTALLDIR"))
            .and_then(|row| row["DefaultDir"].as_str().map(str::to_owned))
            .ok_or_else(|| invalid("canonical INSTALLDIR missing"))?;
        let identity = InstallerIdentity {
            product_code: property("ProductCode")?,
            package_code: old_package_code,
            upgrade_code: property("UpgradeCode")?,
            name: property("ProductName")?,
            manufacturer: property("Manufacturer")?,
            version: property("ProductVersion")?,
            directory_name,
            architecture,
            context,
        };
        let mut builder = InstallerBuilder::new(identity, options)?;
        let mut components = std::collections::BTreeMap::new();
        for row in package.select_rows(msi::Select::table("Component"))? {
            components.insert(
                row["Component"]
                    .as_str()
                    .ok_or_else(|| invalid("canonical component identifier"))?
                    .to_owned(),
                row["ComponentId"]
                    .as_str()
                    .ok_or_else(|| invalid("canonical component GUID"))?
                    .to_owned(),
            );
        }
        let mut file_rows: Vec<_> = package.select_rows(msi::Select::table("File"))?.collect();
        file_rows.sort_by_key(|row| row["Sequence"].as_int());
        drop(package);
        let mut reader = crate::InstallerPackage::open_with_metadata_limit(
            Cursor::new(&bytes),
            usize::try_from(options.limits.max_rows)
                .map_err(|_| WriteError::LimitExceeded("MSI rows"))?,
            options.limits.max_output_bytes,
            options.limits.max_metadata_bytes,
        )?;
        let declared_files = reader.files()?;
        struct NoMedia;
        impl crate::MediaResolver for NoMedia {
            fn resolve(&mut self, name: &str, _: u64) -> crate::Result<Vec<u8>> {
                Err(crate::Error::MissingMedia(name.into()))
            }
        }
        for row in file_rows {
            let id = row["File"]
                .as_str()
                .ok_or_else(|| invalid("canonical file identifier"))?;
            let name = row["FileName"]
                .as_str()
                .ok_or_else(|| invalid("canonical file name"))?;
            let component = row["Component_"]
                .as_str()
                .ok_or_else(|| invalid("canonical file component"))?;
            let component_guid = components
                .get(component)
                .ok_or_else(|| invalid("canonical component missing"))?;
            let file = declared_files
                .iter()
                .find(|file| file.id == id)
                .ok_or_else(|| invalid("canonical file missing"))?;
            let payload_limit = options
                .limits
                .max_file_bytes
                .min(options.limits.max_total_bytes.saturating_sub(builder.total));
            let scratch_available = options
                .limits
                .max_scratch_bytes
                .checked_sub((bytes.len() as u64).saturating_mul(2))
                .and_then(|left| left.checked_sub(builder.total))
                .ok_or(WriteError::LimitExceeded("MSI editor scratch"))?;
            if file.size > payload_limit.min(scratch_available) {
                return Err(WriteError::LimitExceeded("MSI editor file bytes"));
            }
            let decoded = reader.read_file(
                file,
                &mut NoMedia,
                options.limits.max_file_bytes.min(scratch_available),
            )?;
            let live = (bytes.len() as u64)
                .checked_mul(2)
                .and_then(|total| total.checked_add(builder.total))
                .and_then(|total| total.checked_add(decoded.len() as u64))
                .ok_or(WriteError::LimitExceeded("MSI editor scratch"))?;
            if live > options.limits.max_scratch_bytes {
                return Err(WriteError::LimitExceeded("MSI editor scratch"));
            }
            builder.add_file(id, name, component_guid, Cursor::new(decoded))?;
        }
        drop(reader);
        let canonical_scratch = options
            .limits
            .max_scratch_bytes
            .checked_sub((bytes.len() as u64).saturating_mul(2))
            .and_then(|left| left.checked_sub(builder.total))
            .ok_or(WriteError::LimitExceeded(
                "MSI canonical comparison scratch",
            ))?;
        if builder.total > canonical_scratch {
            return Err(WriteError::LimitExceeded(
                "MSI canonical comparison scratch",
            ));
        }
        let mut canonical_builder = builder.clone();
        canonical_builder.options.limits.max_scratch_bytes = canonical_scratch;
        canonical_builder.options.limits.max_output_bytes = canonical_builder
            .options
            .limits
            .max_output_bytes
            .min(bytes.len() as u64);
        let mut canonical = Vec::new();
        canonical_builder.write(&mut canonical)?;
        compare_streams(&bytes, &canonical)?;
        builder.identity.package_code = new_package_code.into();
        Ok(Self { builder })
    }

    /// Adds a new component with its own explicit GUID.
    pub fn add_file(
        &mut self,
        id: &str,
        name: &str,
        component_guid: &str,
        source: impl Read,
    ) -> Result<(), WriteError> {
        self.builder.add_file(id, name, component_guid, source)
    }

    /// Replaces an existing unversioned file, retaining its component and key path.
    pub fn replace_file(&mut self, id: &str, mut source: impl Read) -> Result<(), WriteError> {
        let index = self
            .builder
            .files
            .iter()
            .position(|file| file.id == id)
            .ok_or_else(|| invalid("file to replace does not exist"))?;
        let old = &self.builder.files[index];
        let remaining = self.builder.total - old.bytes.len() as u64;
        let limit = self
            .builder
            .options
            .limits
            .max_total_bytes
            .min(self.builder.options.limits.max_scratch_bytes)
            .checked_sub(remaining)
            .ok_or(WriteError::LimitExceeded("MSI payload bytes"))?
            .min(self.builder.options.limits.max_file_bytes)
            .min(i32::MAX as u64);
        let mut bytes = Vec::new();
        source
            .by_ref()
            .take(limit.saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(WriteError::LimitExceeded("MSI payload bytes"));
        }
        if bytes.starts_with(b"MZ") {
            return Err(WriteError::Unsupported("versioned PE payload".into()));
        }
        if self
            .builder
            .total
            .checked_add(bytes.len() as u64)
            .is_none_or(|total| total > self.builder.options.limits.max_scratch_bytes)
        {
            return Err(WriteError::LimitExceeded("MSI replacement scratch"));
        }
        self.builder.total = remaining + bytes.len() as u64;
        self.builder.files[index].bytes = bytes;
        Ok(())
    }

    /// Removes a file and its single-file component and feature relationship.
    pub fn remove_file(&mut self, id: &str) -> Result<(), WriteError> {
        let index = self
            .builder
            .files
            .iter()
            .position(|file| file.id == id)
            .ok_or_else(|| invalid("file to remove does not exist"))?;
        let file = self.builder.files.remove(index);
        self.builder.total -= file.bytes.len() as u64;
        Ok(())
    }

    /// Renames a file as a newly identified component. A new component GUID is
    /// required because its target key-path name changes.
    pub fn rename_file(
        &mut self,
        id: &str,
        new_name: &str,
        new_component_guid: &str,
    ) -> Result<(), WriteError> {
        leaf(new_name)?;
        guid(new_component_guid)?;
        let index = self
            .builder
            .files
            .iter()
            .position(|file| file.id == id)
            .ok_or_else(|| invalid("file to rename does not exist"))?;
        if self.builder.files.iter().any(|file| {
            file.name.eq_ignore_ascii_case(new_name) || file.component_guid == new_component_guid
        }) {
            return Err(invalid("rename target or component GUID conflicts"));
        }
        self.builder.files[index].name = new_name.into();
        self.builder.files[index].component_guid = new_component_guid.into();
        Ok(())
    }

    /// Rebuilds tables, sequences, and cabinet under the explicit new PackageCode.
    pub fn write(self, destination: impl Write) -> Result<InstallerWriteReport, WriteError> {
        self.builder.write(destination)
    }
}

fn compare_streams(source: &[u8], canonical: &[u8]) -> Result<(), WriteError> {
    let mut left = cfb::CompoundFile::open(Cursor::new(source))?;
    let mut right = cfb::CompoundFile::open(Cursor::new(canonical))?;
    let entries = |compound: &cfb::CompoundFile<Cursor<&[u8]>>| {
        compound
            .walk()
            .map(|entry| (entry.path().to_path_buf(), entry.is_stream(), entry.len()))
            .collect::<std::collections::BTreeSet<_>>()
    };
    let left_entries = entries(&left);
    if left_entries != entries(&right) {
        return Err(WriteError::Unsupported(
            "MSI payload editor requires an unchanged canonical file-only schema and streams"
                .into(),
        ));
    }
    for (path, is_stream, _) in left_entries {
        if is_stream {
            let mut a = left.open_stream(&path)?;
            let mut b = right.open_stream(&path)?;
            let mut a_bytes = [0u8; 8192];
            let mut b_bytes = [0u8; 8192];
            loop {
                let count = a.read(&mut a_bytes)?;
                b.read_exact(&mut b_bytes[..count])?;
                if a_bytes[..count] != b_bytes[..count] {
                    return Err(WriteError::Unsupported(
                        "MSI payload editor cannot preserve noncanonical metadata or references"
                            .into(),
                    ));
                }
                if count == 0 {
                    break;
                }
            }
        }
    }
    Ok(())
}
