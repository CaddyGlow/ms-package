//! Unsigned MSI database authoring. Database writes do not establish installability.
//!
//! Editors copy their source and preserve its compound-file content. Identity values
//! remain unchanged unless explicitly edited through rows or summary information.
//! Limits bound container bytes and decoded rows, but cannot bound temporary
//! allocations inside the MSI/compound-file parsers. Declared single-column foreign
//! keys are checked before emission; implicit/conditional references, servicing
//! rules, and Windows lifecycle validation remain the caller's responsibility.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};

use super::{WriteError, WriteOptions, WriteReport};

/// Database-only validation and change report; it makes no installation claim.
#[derive(Debug, Clone)]
pub struct InstallerWriteReport {
    /// Container size and resource accounting.
    pub output: WriteReport,
    /// Tables explicitly changed by the caller.
    pub changed_tables: Vec<String>,
    /// Streams explicitly changed by the caller.
    pub changed_streams: Vec<String>,
    /// Whether summary properties were explicitly edited.
    pub summary_changed: bool,
}

/// Portable checks performed before emission; neither scope guarantees installation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallerValidationScope {
    /// Backend schemas/types/primary keys, declared foreign keys and resource bounds.
    DatabaseOnly,
    /// The canonical file-only table/action, identity and declared-media profile.
    CanonicalFileOnly,
}

/// One explicit change to a known package/product/component identity or key path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallerIdentityChange {
    /// Property name, PackageCode, or `Component.<identifier>.<field>`.
    pub name: String,
    /// Original value; absent for newly created identities.
    pub old_value: Option<String>,
    /// Emitted value; absent for removed identities.
    pub new_value: Option<String>,
}

/// Completed database report with identity differences and an explicit validation scope.
#[derive(Clone, Debug)]
pub struct InstallerDetailedWriteReport {
    /// Existing change and byte accounting report.
    pub database: InstallerWriteReport,
    /// Actual differences for known product, package and component identity fields.
    pub identity_changes: Vec<InstallerIdentityChange>,
    /// Portable validation performed; native lifecycle/trust are separate evidence.
    pub validation_scope: InstallerValidationScope,
    /// External media finalized before the database output.
    pub external_media: super::installer_media::InstallerMediaReport,
}

pub(super) fn identity_changes(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Vec<InstallerIdentityChange> {
    before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|name| {
            let old_value = before.get(name).cloned();
            let new_value = after.get(name).cloned();
            (old_value != new_value).then(|| InstallerIdentityChange {
                name: name.clone(),
                old_value,
                new_value,
            })
        })
        .collect()
}

pub(super) fn identity_values<R: Read + Seek>(
    package: &mut msi::Package<R>,
) -> Result<BTreeMap<String, String>, WriteError> {
    let mut values = BTreeMap::new();
    if let Some(package_code) = package.summary_info().uuid() {
        values.insert(
            "PackageCode".into(),
            package_code.braced().to_string().to_ascii_uppercase(),
        );
    }
    if package
        .get_table("Property")
        .is_some_and(|table| table.has_column("Property") && table.has_column("Value"))
    {
        for row in package.select_rows(msi::Select::table("Property"))? {
            if let (Some(name), Some(value)) = (row["Property"].as_str(), row["Value"].as_str())
                && matches!(name, "ProductCode" | "UpgradeCode" | "ProductVersion")
            {
                values.insert(name.into(), value.into());
            }
        }
    }
    if package
        .get_table("Component")
        .is_some_and(|table| table.has_column("Component"))
    {
        let fields: Vec<_> = ["ComponentId", "KeyPath"]
            .into_iter()
            .filter(|field| {
                package
                    .get_table("Component")
                    .is_some_and(|table| table.has_column(field))
            })
            .collect();
        for row in package.select_rows(msi::Select::table("Component"))? {
            if let Some(name) = row["Component"].as_str() {
                for field in &fields {
                    if let Some(value) = row[*field].as_str() {
                        values.insert(format!("Component.{name}.{field}"), value.into());
                    }
                }
            }
        }
    }
    Ok(values)
}

pub(super) struct BoundedCursor {
    pub(super) inner: Cursor<Vec<u8>>,
    pub(super) limit: u64,
}

impl Read for BoundedCursor {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.inner.read(bytes)
    }
}

impl Seek for BoundedCursor {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

impl Write for BoundedCursor {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let end = self.inner.position().checked_add(bytes.len() as u64);
        if end.is_none_or(|end| end > self.limit) {
            return Err(io::Error::other("MSI output/scratch byte limit exceeded"));
        }
        self.inner.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Creates a database using typed MSI schemas and rows.
///
/// Operations modify private bounded scratch storage. `write` finalizes the MSI
/// backend before touching the caller's destination. A destination I/O failure
/// can leave partial data. Package, product, upgrade, and component identifiers
/// are never generated or regenerated automatically.
///
/// ```
/// # #[cfg(feature = "write")] {
/// use ms_package::authoring::{InstallerDatabaseBuilder, WriteOptions};
/// let mut builder = InstallerDatabaseBuilder::new(WriteOptions::default())?;
/// builder.create_table("Custom", vec![msi::Column::build("Key").primary_key().string(64)])?;
/// builder.insert_rows("Custom", vec![vec![msi::Value::Str("Example".into())]])?;
/// let mut bytes = Vec::new();
/// builder.write(&mut bytes)?;
/// # }
/// # Ok::<(), ms_package::authoring::WriteError>(())
/// ```
pub struct InstallerDatabaseBuilder {
    package: msi::Package<BoundedCursor>,
    options: WriteOptions,
    tables: BTreeSet<String>,
    streams: BTreeSet<String>,
    summary_changed: bool,
    failed: bool,
    initial_identities: BTreeMap<String, String>,
}

impl InstallerDatabaseBuilder {
    /// Creates an unsigned Installer database, not an installable product.
    pub fn new(options: WriteOptions) -> Result<Self, WriteError> {
        let inner = BoundedCursor {
            inner: Cursor::new(Vec::new()),
            limit: options
                .limits
                .max_output_bytes
                .min(options.limits.max_scratch_bytes),
        };
        let package = msi::Package::create(msi::PackageType::Installer, inner)?;
        Self::from_package(package, options)
    }

    fn from_package(
        mut package: msi::Package<BoundedCursor>,
        options: WriteOptions,
    ) -> Result<Self, WriteError> {
        let initial_identities = identity_values(&mut package)?;
        Ok(Self {
            package,
            options,
            tables: BTreeSet::new(),
            streams: BTreeSet::new(),
            summary_changed: false,
            failed: false,
            initial_identities,
        })
    }

    fn check_name(name: &str) -> Result<(), WriteError> {
        if name.to_ascii_lowercase().contains("digitalsignature")
            || matches!(
                name.to_ascii_lowercase().as_str(),
                "msidigitalcertificate" | "msipackagecertificate"
            )
        {
            return Err(WriteError::Unsupported(
                "MSI signature metadata authoring".into(),
            ));
        }
        Ok(())
    }

    fn mutate(
        &mut self,
        operation: impl FnOnce(&mut msi::Package<BoundedCursor>) -> io::Result<()>,
    ) -> Result<(), WriteError> {
        if self.failed {
            return Err(WriteError::InvalidInput(
                "discard database after a failed operation".into(),
            ));
        }
        let result = operation(&mut self.package)
            .map_err(WriteError::from)
            .and_then(|()| self.validate().map(|_| ()));
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// Creates a table with its column and validation metadata.
    pub fn create_table(
        &mut self,
        name: &str,
        columns: Vec<msi::Column>,
    ) -> Result<(), WriteError> {
        Self::check_name(name)?;
        if columns.len() > 32 || name.len() as u64 > self.options.limits.max_metadata_bytes {
            return Err(WriteError::LimitExceeded("MSI schema"));
        }
        let codepage = self.package.database_codepage();
        let mut metadata = name.len() as u64;
        for column in &columns {
            for value in std::iter::once(column.name()).chain(
                column
                    .enum_values()
                    .into_iter()
                    .flatten()
                    .map(String::as_str),
            ) {
                metadata = metadata
                    .checked_add(value.len() as u64)
                    .ok_or(WriteError::LimitExceeded("MSI schema metadata"))?;
                if metadata > self.options.limits.max_metadata_bytes {
                    return Err(WriteError::LimitExceeded("MSI schema metadata"));
                }
                if codepage.decode(&codepage.encode(value)) != value {
                    return Err(WriteError::InvalidInput(
                        "schema string is not representable in its code page".into(),
                    ));
                }
            }
        }
        self.mutate(|package| package.create_table(name, columns))?;
        self.tables.insert(name.into());
        Ok(())
    }

    /// Drops a table. References from other tables are the caller's responsibility.
    pub fn drop_table(&mut self, name: &str) -> Result<(), WriteError> {
        Self::check_name(name)?;
        self.mutate(|package| package.drop_table(name))?;
        self.tables.insert(name.into());
        Ok(())
    }

    /// Inserts typed rows; the backend checks schema types and primary keys.
    pub fn insert_rows(
        &mut self,
        name: &str,
        rows: Vec<Vec<msi::Value>>,
    ) -> Result<(), WriteError> {
        Self::check_name(name)?;
        if rows.len() as u64 > self.options.limits.max_rows {
            return Err(WriteError::LimitExceeded("MSI rows"));
        }
        let bytes = rows
            .iter()
            .flatten()
            .filter_map(msi::Value::as_str)
            .try_fold(0u64, |total, value| total.checked_add(value.len() as u64));
        if bytes.is_none_or(|bytes| bytes > self.options.limits.max_metadata_bytes) {
            return Err(WriteError::LimitExceeded("MSI row metadata"));
        }
        self.mutate(|package| package.insert_rows(msi::Insert::into(name).rows(rows)))?;
        self.tables.insert(name.into());
        Ok(())
    }

    /// Updates rows matching an explicit condition, or every row when absent.
    pub fn update_rows(
        &mut self,
        name: &str,
        values: Vec<(String, msi::Value)>,
        condition: Option<msi::Expr>,
    ) -> Result<(), WriteError> {
        Self::check_name(name)?;
        let metadata = values.iter().try_fold(0u64, |total, (column, value)| {
            total.checked_add(column.len() as u64).and_then(|total| {
                total.checked_add(value.as_str().map_or(0, |value| value.len() as u64))
            })
        });
        if metadata.is_none_or(|total| total > self.options.limits.max_metadata_bytes) {
            return Err(WriteError::LimitExceeded("MSI update metadata"));
        }
        let mut query = msi::Update::table(name);
        for (column, value) in values {
            query = query.set(column, value);
        }
        if let Some(condition) = condition {
            query = query.with(condition);
        }
        self.mutate(|package| package.update_rows(query))?;
        self.tables.insert(name.into());
        Ok(())
    }

    /// Deletes matching rows, or every row when no condition is supplied.
    pub fn delete_rows(
        &mut self,
        name: &str,
        condition: Option<msi::Expr>,
    ) -> Result<(), WriteError> {
        Self::check_name(name)?;
        let mut query = msi::Delete::from(name);
        if let Some(condition) = condition {
            query = query.with(condition);
        }
        self.mutate(|package| package.delete_rows(query))?;
        self.tables.insert(name.into());
        Ok(())
    }

    /// Creates or replaces an embedded stream, truncating any previous content.
    pub fn write_stream(&mut self, name: &str, mut source: impl Read) -> Result<(), WriteError> {
        Self::check_name(name)?;
        let limit = self
            .options
            .limits
            .max_file_bytes
            .min(self.options.limits.max_total_bytes);
        self.mutate(|package| {
            let mut stream = package.write_stream(name)?;
            let count = io::copy(
                &mut source.by_ref().take(limit.saturating_add(1)),
                &mut stream,
            )?;
            if count > limit {
                return Err(io::Error::other("MSI stream byte limit exceeded"));
            }
            stream.flush()
        })?;
        self.streams.insert(name.into());
        Ok(())
    }

    /// Removes an embedded stream.
    pub fn remove_stream(&mut self, name: &str) -> Result<(), WriteError> {
        Self::check_name(name)?;
        self.mutate(|package| package.remove_stream(name))?;
        self.streams.insert(name.into());
        Ok(())
    }

    /// Edits explicit summary properties, preserving all untouched properties.
    /// Set a new PackageCode here when required by your distribution policy.
    pub fn edit_summary(
        &mut self,
        edit: impl FnOnce(&mut msi::SummaryInfo),
    ) -> Result<(), WriteError> {
        self.mutate(|package| {
            edit(package.summary_info_mut());
            Ok(())
        })?;
        self.summary_changed = true;
        Ok(())
    }

    /// Sets the database string code page explicitly.
    pub fn set_database_codepage(&mut self, codepage: msi::CodePage) -> Result<(), WriteError> {
        self.mutate(|package| {
            package.set_database_codepage(codepage);
            Ok(())
        })
    }

    fn validate(&mut self) -> Result<(u64, u64), WriteError> {
        let limits = &self.options.limits;
        let names: Vec<_> = self
            .package
            .tables()
            .map(|table| table.name().to_owned())
            .collect();
        let stream_names: Vec<_> = self.package.streams().collect();
        if (names.len() as u64).saturating_add(stream_names.len() as u64) > limits.max_entries {
            return Err(WriteError::LimitExceeded("MSI entries"));
        }
        let mut rows = 0u64;
        let mut metadata = 0u64;
        let codepage = self.package.database_codepage();
        let summary = self.package.summary_info();
        let summary_codepage = summary.codepage();
        let mut summary_strings: Vec<&str> = [
            summary.arch(),
            summary.author(),
            summary.comments(),
            summary.creating_application(),
            summary.subject(),
            summary.title(),
            summary.last_saved_by(),
        ]
        .into_iter()
        .flatten()
        .collect();
        let keywords = summary.keywords();
        summary_strings.extend(keywords.iter().map(String::as_str));
        for value in summary_strings {
            metadata = metadata
                .checked_add(value.len() as u64)
                .ok_or(WriteError::LimitExceeded("MSI summary metadata"))?;
            if summary_codepage.decode(&summary_codepage.encode(value)) != value {
                return Err(WriteError::InvalidInput(
                    "summary string is not representable in its code page".into(),
                ));
            }
        }
        let entries = (names.len() as u64).saturating_add(stream_names.len() as u64);
        for name in names {
            Self::check_name(&name)?;
            metadata = metadata
                .checked_add(name.len() as u64)
                .ok_or(WriteError::LimitExceeded("MSI metadata"))?;
            for row in self.package.select_rows(msi::Select::table(&name))? {
                rows = rows
                    .checked_add(1)
                    .ok_or(WriteError::LimitExceeded("MSI rows"))?;
                if rows > limits.max_rows {
                    return Err(WriteError::LimitExceeded("MSI rows"));
                }
                for index in 0..row.len() {
                    if let Some(value) = row[index].as_str() {
                        if codepage.decode(&codepage.encode(value)) != value {
                            return Err(WriteError::InvalidInput(
                                "row string is not representable in the database code page".into(),
                            ));
                        }
                        metadata = metadata
                            .checked_add(value.len() as u64)
                            .ok_or(WriteError::LimitExceeded("MSI metadata"))?;
                    }
                }
                if metadata > limits.max_metadata_bytes {
                    return Err(WriteError::LimitExceeded("MSI metadata"));
                }
            }
        }
        if metadata > limits.max_metadata_bytes {
            return Err(WriteError::LimitExceeded("MSI metadata"));
        }
        let mut total = 0u64;
        for name in stream_names {
            Self::check_name(&name)?;
            let count = io::copy(
                &mut self
                    .package
                    .read_stream(&name)?
                    .take(limits.max_file_bytes.saturating_add(1)),
                &mut io::sink(),
            )?;
            if count > limits.max_file_bytes {
                return Err(WriteError::LimitExceeded("MSI stream bytes"));
            }
            total = total
                .checked_add(count)
                .ok_or(WriteError::LimitExceeded("MSI aggregate stream bytes"))?;
            if total > limits.max_total_bytes {
                return Err(WriteError::LimitExceeded("MSI aggregate stream bytes"));
            }
        }
        Ok((entries, total))
    }

    /// Explicitly finalizes and emits the database, then flushes the destination.
    pub fn write(self, destination: impl Write) -> Result<InstallerWriteReport, WriteError> {
        self.write_detailed(destination)
            .map(|report| report.database)
    }

    /// Emits the database with actual identity differences and database-only scope.
    pub fn write_detailed(
        mut self,
        mut destination: impl Write,
    ) -> Result<InstallerDetailedWriteReport, WriteError> {
        if self.failed {
            return Err(WriteError::InvalidInput(
                "discard database after a failed operation".into(),
            ));
        }
        let (entries, decoded_bytes) = self.validate()?;
        self.validate_foreign_keys()?;
        let final_identities = identity_values(&mut self.package)?;
        let identity_changes = identity_changes(&self.initial_identities, &final_identities);
        self.package.flush()?;
        let bytes = self.package.into_inner()?.inner.into_inner();
        let output_bytes = bytes.len() as u64;
        crate::installer_metadata::preflight(
            &mut Cursor::new(&bytes),
            self.options.limits.max_metadata_bytes,
        )?;
        // Opening independently exercises backend serialization before publication.
        let reopened = msi::Package::open(Cursor::new(&bytes))?;
        if reopened.package_type() != msi::PackageType::Installer {
            return Err(WriteError::Unsupported(
                "serialized database profile".into(),
            ));
        }
        drop(reopened);
        destination.write_all(&bytes)?;
        destination.flush()?;
        Ok(InstallerDetailedWriteReport {
            identity_changes,
            validation_scope: InstallerValidationScope::DatabaseOnly,
            external_media: Default::default(),
            database: InstallerWriteReport {
                output: WriteReport {
                    entries,
                    decoded_bytes,
                    output_bytes,
                    signature_removed: false,
                },
                changed_tables: self.tables.into_iter().collect(),
                changed_streams: self.streams.into_iter().collect(),
                summary_changed: self.summary_changed,
            },
        })
    }

    fn validate_foreign_keys(&mut self) -> Result<(), WriteError> {
        if !self.package.has_table("_Validation") {
            return Ok(());
        }
        let mut declarations = Vec::new();
        for row in self
            .package
            .select_rows(msi::Select::table("_Validation"))?
        {
            match (row["KeyTable"].as_str(), row["KeyColumn"].as_int()) {
                (None, None) => {},
                (Some(target), Some(index)) if !target.is_empty() && index > 0 => {
                    let table = row["Table"].as_str().ok_or_else(|| WriteError::InvalidInput("foreign-key table declaration is missing".into()))?;
                    let column = row["Column"].as_str().ok_or_else(|| WriteError::InvalidInput("foreign-key column declaration is missing".into()))?;
                    declarations.push((table.to_owned(), column.to_owned(), target.to_owned(), index));
                },
                _ => return Err(WriteError::InvalidInput("foreign-key declaration requires both a target table and a positive column index".into())),
            }
        }
        for (table, column, target, target_index) in declarations {
            if !self.package.has_table(&table) {
                continue;
            }
            let source_column = self
                .package
                .get_table(&table)
                .and_then(|table| {
                    table
                        .columns()
                        .iter()
                        .position(|candidate| candidate.name() == column)
                })
                .ok_or_else(|| {
                    WriteError::InvalidInput("foreign-key source column is missing".into())
                })?;
            let source_values: BTreeSet<_> = self
                .package
                .select_rows(msi::Select::table(&table))?
                .map(|row| row[source_column].clone())
                .filter(|value| !value.is_null())
                .collect();
            if source_values.is_empty() {
                continue;
            }
            let target_column = usize::try_from(target_index)
                .ok()
                .and_then(|index| index.checked_sub(1))
                .filter(|index| {
                    self.package
                        .get_table(&target)
                        .is_some_and(|table| *index < table.columns().len())
                })
                .ok_or_else(|| {
                    WriteError::InvalidInput(format!(
                        "foreign-key target {target} column {target_index} is missing"
                    ))
                })?;
            let target_values: BTreeSet<_> = self
                .package
                .select_rows(msi::Select::table(&target))?
                .map(|row| row[target_column].clone())
                .collect();
            if !source_values.is_subset(&target_values) {
                return Err(WriteError::InvalidInput(format!(
                    "foreign key {table}.{column} references absent {target} values"
                )));
            }
        }
        Ok(())
    }
}

/// Opens an unsigned MSI as a private copy for raw database edits.
pub struct InstallerEditor;

impl InstallerEditor {
    /// Copies and preflights a source. Signed MSI and signature tables are rejected
    /// even under `SignaturePolicy::Remove`: safe MSI signature removal is unaudited.
    pub fn open(
        mut source: impl Read,
        options: WriteOptions,
    ) -> Result<InstallerDatabaseBuilder, WriteError> {
        let limit = options
            .limits
            .max_scratch_bytes
            .min(options.limits.max_output_bytes);
        let mut bytes = Vec::new();
        source
            .by_ref()
            .take(limit.saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(WriteError::LimitExceeded("MSI source bytes"));
        }
        crate::installer_metadata::preflight(
            &mut Cursor::new(&bytes),
            options.limits.max_metadata_bytes,
        )?;
        {
            let compound = cfb::CompoundFile::open(Cursor::new(&bytes))?;
            for entry in compound.walk() {
                Self::check_signature(entry.name())?;
            }
        }
        let package = msi::Package::open(BoundedCursor {
            inner: Cursor::new(bytes),
            limit,
        })?;
        if package.package_type() != msi::PackageType::Installer {
            return Err(WriteError::Unsupported("MSI transforms and patches".into()));
        }
        let mut builder = InstallerDatabaseBuilder::from_package(package, options)?;
        builder.validate()?;
        Ok(builder)
    }

    fn check_signature(name: &str) -> Result<(), WriteError> {
        InstallerDatabaseBuilder::check_name(name)
    }
}
