use crate::{Error, Result, safe_name};
use archive_core::{Archive, Limits};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Seek, SeekFrom},
};

/// Typed MSI table contents, preserving database column order.
#[derive(Clone, Debug)]
pub struct TableData {
    /// Column names.
    pub columns: Vec<String>,
    /// Typed database cells; nulls remain null.
    pub rows: Vec<Vec<msi::Value>>,
}

/// A declared Installer payload; paths do not simulate installer conditions.
#[derive(Clone, Debug)]
pub struct InstallerFile {
    /// File table primary key and cabinet member name.
    pub id: String,
    /// Declared target directory path.
    pub path: String,
    /// Declared source-tree name passed to an explicit loose-file resolver.
    pub source_path: String,
    /// Declared decoded size.
    pub size: u64,
    /// Installer file sequence.
    pub sequence: i32,
    /// Media cabinet name; leading '#' denotes embedded storage.
    pub cabinet: Option<String>,
}

/// Explicit resolver for external media; no host path search occurs.
pub trait MediaResolver {
    /// Supply named external cabinet or loose source-file bytes with a bound.
    fn resolve(&mut self, name: &str, max_bytes: u64) -> Result<Vec<u8>>;
}

/// A portable read-only Installer database and compound storage.
pub struct InstallerPackage<R: Read + Seek> {
    package: msi::Package<R>,
    max_rows: usize,
}

impl<R: Read + Seek> InstallerPackage<R> {
    /// Open a package without exposing database writers or installer execution.
    pub fn open(reader: R, max_rows: usize) -> Result<Self> {
        Self::open_bounded(reader, max_rows, 512 * 1024 * 1024)
    }
    /// Bound compound-storage input before the upstream parser allocates metadata.
    pub fn open_bounded(reader: R, max_rows: usize, max_storage: u64) -> Result<Self> {
        Self::open_with_metadata_limit(
            reader,
            max_rows,
            max_storage,
            max_storage.min(64 * 1024 * 1024),
        )
    }
    /// Bound storage and aggregate encoded MSI metadata before parsing tables.
    /// This is a stream-byte bound, not a process-wide heap bound.
    pub fn open_with_metadata_limit(
        mut reader: R,
        max_rows: usize,
        max_storage: u64,
        max_metadata: u64,
    ) -> Result<Self> {
        let size = reader.seek(SeekFrom::End(0))?;
        if size > max_storage {
            return Err(Error::Limit("compound storage bytes"));
        }
        reader.seek(SeekFrom::Start(0))?;
        crate::installer_metadata::preflight(&mut reader, max_metadata)?;
        reader.rewind()?;
        let package = msi::Package::open(reader)?;
        if package.package_type() != msi::PackageType::Installer {
            return Err(Error::Unsupported("transforms and patches".into()));
        }
        Ok(Self { package, max_rows })
    }
    /// Table names present in the database.
    pub fn tables(&self) -> Vec<String> {
        self.package.tables().map(|t| t.name().to_owned()).collect()
    }
    /// Logical raw stream names, separate from installed payload files.
    pub fn streams(&self) -> Vec<String> {
        self.package.streams().map(|s| s.to_owned()).collect()
    }
    /// Summary information parsed by the portable database backend.
    pub fn summary(&self) -> &msi::SummaryInfo {
        self.package.summary_info()
    }
    /// Read a raw logical stream with an actual-byte bound.
    pub fn read_stream(&mut self, name: &str, max: u64) -> Result<Vec<u8>> {
        let reader = self.package.read_stream(name)?;
        let mut data = Vec::new();
        reader.take(max.saturating_add(1)).read_to_end(&mut data)?;
        if data.len() as u64 > max {
            return Err(Error::Limit("stream bytes"));
        }
        Ok(data)
    }
    /// Read all typed cells of a table within the row budget.
    pub fn table(&mut self, name: &str) -> Result<TableData> {
        let rows = self.package.select_rows(msi::Select::table(name))?;
        let columns = rows.columns().iter().map(|c| c.name().to_owned()).collect();
        let mut values = Vec::new();
        for row in rows {
            if values.len() >= self.max_rows {
                return Err(Error::Limit("database rows"));
            }
            values.push((0..row.len()).map(|i| row[i].clone()).collect());
        }
        Ok(TableData {
            columns,
            rows: values,
        })
    }
    /// Resolve File/Component/Directory/Media references into declared paths.
    pub fn files(&mut self) -> Result<Vec<InstallerFile>> {
        let word_count = self.package.summary_info().word_count().unwrap_or(0);
        let dirs = self.table("Directory")?;
        let components = self.table("Component")?;
        let media = self.table("Media")?;
        let files = self.table("File")?;
        let mut directory_map = BTreeMap::new();
        for r in &dirs.rows {
            let id = text(&dirs, r, "Directory")?;
            let parent = cell(&dirs, r, "Directory_Parent")?
                .as_str()
                .map(str::to_owned);
            let name = text(&dirs, r, "DefaultDir")?;
            if directory_map
                .insert(id.to_owned(), (parent, name.to_owned()))
                .is_some()
            {
                return Err(Error::Malformed("duplicate directory".into()));
            }
        }
        let mut component_map = BTreeMap::new();
        for r in &components.rows {
            let id = text(&components, r, "Component")?;
            if component_map
                .insert(
                    id.to_owned(),
                    text(&components, r, "Directory_")?.to_owned(),
                )
                .is_some()
            {
                return Err(Error::Malformed("duplicate component".into()));
            }
        }
        let mut cabinets = Vec::new();
        for r in &media.rows {
            cabinets.push((
                integer(&media, r, "LastSequence")?,
                cell(&media, r, "Cabinet")?.as_str().map(str::to_owned),
            ));
        }
        cabinets.sort_by_key(|m| m.0);
        if cabinets.windows(2).any(|w| w[0].0 >= w[1].0) {
            return Err(Error::Malformed("media sequence ordering".into()));
        }
        let mut output = Vec::new();
        let mut ids = BTreeSet::new();
        let mut paths = BTreeSet::new();
        let mut sequences = BTreeSet::new();
        for r in &files.rows {
            let id = text(&files, r, "File")?.to_owned();
            let component = text(&files, r, "Component_")?;
            let directory = component_map
                .get(component)
                .ok_or_else(|| Error::Malformed("missing component".into()))?;
            let mut seen = BTreeSet::new();
            let mut segments = directory_path(directory, &directory_map, &mut seen)?;
            segments.push(long_name(text(&files, r, "FileName")?).to_owned());
            let path = safe_name(&segments.join("/"))?;
            let source_name = text(&files, r, "FileName")?;
            let mut source_segments = if word_count & 2 != 0 {
                Vec::new()
            } else {
                source_directory_path(
                    directory,
                    &directory_map,
                    &mut BTreeSet::new(),
                    word_count & 1 != 0,
                )?
            };
            source_segments.push(
                if word_count & 1 != 0 {
                    source_name.split('|').next().unwrap_or(source_name)
                } else {
                    long_name(source_name)
                }
                .to_owned(),
            );
            let source_path = safe_name(&source_segments.join("/"))?;
            let sequence = integer(&files, r, "Sequence")?;
            let size = u64::try_from(integer(&files, r, "FileSize")?)
                .map_err(|_| Error::Malformed("negative file size".into()))?;
            let media_cabinet = cabinets
                .iter()
                .find(|m| sequence <= m.0)
                .ok_or_else(|| Error::Malformed("missing media sequence".into()))?
                .1
                .clone();
            let attributes = cell(&files, r, "Attributes")?.as_int().unwrap_or(0);
            if attributes & 0x6000 == 0x6000 {
                return Err(Error::Malformed(
                    "contradictory file compression attributes".into(),
                ));
            }
            let compressed =
                attributes & 0x4000 != 0 || (attributes & 0x2000 == 0 && word_count & 2 != 0);
            let cabinet =
                if compressed {
                    Some(media_cabinet.ok_or_else(|| {
                        Error::Malformed("compressed file without cabinet".into())
                    })?)
                } else {
                    None
                };
            if !ids.insert(id.clone())
                || !paths.insert(path.to_lowercase())
                || !sequences.insert(sequence)
                || sequence <= 0
            {
                return Err(Error::Malformed("duplicate file/path/sequence".into()));
            }
            output.push(InstallerFile {
                id,
                path,
                source_path,
                size,
                sequence,
                cabinet,
            });
        }
        output.sort_by_key(|f| f.sequence);
        Ok(output)
    }
    /// Decode the declared cabinet member or an explicitly resolved loose file.
    pub fn read_file(
        &mut self,
        file: &InstallerFile,
        resolver: &mut impl MediaResolver,
        max: u64,
    ) -> Result<Vec<u8>> {
        if file.size > max {
            return Err(Error::Limit("payload bytes"));
        }
        let bytes = match &file.cabinet {
            Some(name) => {
                let data = if let Some(stream) = name.strip_prefix('#') {
                    self.read_stream(stream, max)?
                } else {
                    resolver.resolve(name, max)?
                };
                if data.len() as u64 > max {
                    return Err(Error::Limit("media bytes"));
                }
                let mut cab = Archive::open(Cursor::new(data), Limits::default())?;
                let member = cab
                    .entries()
                    .iter()
                    .find(|e| e.name == file.id)
                    .ok_or_else(|| Error::Integrity(format!("cabinet member {}", file.id)))?;
                cab.read_entry(member.id, file.size)?
            }
            None => resolver.resolve(&file.source_path, file.size)?,
        };
        if bytes.len() as u64 != file.size {
            return Err(Error::Integrity(format!("size of {}", file.id)));
        }
        Ok(bytes)
    }
}

fn cell<'a>(table: &TableData, row: &'a [msi::Value], column: &str) -> Result<&'a msi::Value> {
    let index = table
        .columns
        .iter()
        .position(|c| c == column)
        .ok_or_else(|| Error::Malformed(format!("missing column {column}")))?;
    row.get(index)
        .ok_or_else(|| Error::Malformed("short database row".into()))
}
fn text<'a>(table: &TableData, row: &'a [msi::Value], column: &str) -> Result<&'a str> {
    cell(table, row, column)?
        .as_str()
        .ok_or_else(|| Error::Malformed(format!("invalid string {column}")))
}
fn integer(table: &TableData, row: &[msi::Value], column: &str) -> Result<i32> {
    cell(table, row, column)?
        .as_int()
        .ok_or_else(|| Error::Malformed(format!("invalid integer {column}")))
}
fn long_name(name: &str) -> &str {
    name.split_once('|').map_or(name, |(_, long)| long)
}
fn directory_path(
    id: &str,
    dirs: &BTreeMap<String, (Option<String>, String)>,
    seen: &mut BTreeSet<String>,
) -> Result<Vec<String>> {
    if seen.len() >= 128 {
        return Err(Error::Limit("directory depth"));
    }
    if !seen.insert(id.to_owned()) {
        return Err(Error::Malformed("directory cycle".into()));
    }
    let (parent, name) = dirs
        .get(id)
        .ok_or_else(|| Error::Malformed("missing directory".into()))?;
    let mut parts = if let Some(parent) = parent {
        directory_path(parent, dirs, seen)?
    } else {
        Vec::new()
    };
    let target = long_name(
        name.split_once(':')
            .map_or(name.as_str(), |(target, _)| target),
    );
    if target != "." && target != "SourceDir" {
        parts.push(target.to_owned());
    }
    Ok(parts)
}

fn source_directory_path(
    id: &str,
    dirs: &BTreeMap<String, (Option<String>, String)>,
    seen: &mut BTreeSet<String>,
    short: bool,
) -> Result<Vec<String>> {
    if seen.len() >= 128 {
        return Err(Error::Limit("source directory depth"));
    }
    if !seen.insert(id.to_owned()) {
        return Err(Error::Malformed("source directory cycle".into()));
    }
    let (parent, name) = dirs
        .get(id)
        .ok_or_else(|| Error::Malformed("missing source directory".into()))?;
    let mut parts = if let Some(parent) = parent {
        source_directory_path(parent, dirs, seen, short)?
    } else {
        Vec::new()
    };
    let source = name
        .split_once(':')
        .map_or(name.as_str(), |(_, source)| source);
    let source = if short {
        source.split('|').next().unwrap_or(source)
    } else {
        long_name(source)
    };
    if source != "." && source != "SourceDir" {
        parts.push(source.to_owned());
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    struct Resolver(Option<Vec<u8>>);
    impl MediaResolver for Resolver {
        fn resolve(&mut self, name: &str, _max: u64) -> Result<Vec<u8>> {
            self.0
                .clone()
                .ok_or_else(|| Error::MissingMedia(name.into()))
        }
    }
    #[test]
    fn table_relationships_map_multiple_cabinets_and_source_names() {
        use msi::{Column, Insert, Value};
        let mut package =
            msi::Package::create(msi::PackageType::Installer, Cursor::new(Vec::new())).unwrap();
        package
            .create_table(
                "Directory",
                vec![
                    Column::build("Directory").primary_key().string(72),
                    Column::build("Directory_Parent").nullable().string(72),
                    Column::build("DefaultDir").string(255),
                ],
            )
            .unwrap();
        package
            .insert_rows(
                Insert::into("Directory")
                    .row(vec![
                        Value::Str("ROOT".into()),
                        Value::Null,
                        Value::Str("SourceDir".into()),
                    ])
                    .row(vec![
                        Value::Str("APP".into()),
                        Value::Str("ROOT".into()),
                        Value::Str("DEST~1|Destination:SRC~1|Source".into()),
                    ]),
            )
            .unwrap();
        package
            .create_table(
                "Component",
                vec![
                    Column::build("Component").primary_key().string(72),
                    Column::build("Directory_").string(72),
                ],
            )
            .unwrap();
        package
            .insert_rows(Insert::into("Component").row(vec![
                Value::Str("component".into()),
                Value::Str("APP".into()),
            ]))
            .unwrap();
        package
            .create_table(
                "Media",
                vec![
                    Column::build("DiskId").primary_key().int16(),
                    Column::build("LastSequence").int32(),
                    Column::build("Cabinet").nullable().string(255),
                ],
            )
            .unwrap();
        package
            .insert_rows(
                Insert::into("Media")
                    .row(vec![
                        Value::Int(1),
                        Value::Int(1),
                        Value::Str("#first.cab".into()),
                    ])
                    .row(vec![
                        Value::Int(2),
                        Value::Int(3),
                        Value::Str("second.cab".into()),
                    ]),
            )
            .unwrap();
        package
            .create_table(
                "File",
                vec![
                    Column::build("File").primary_key().string(72),
                    Column::build("Component_").string(72),
                    Column::build("FileName").string(255),
                    Column::build("FileSize").int32(),
                    Column::build("Sequence").int32(),
                    Column::build("Attributes").nullable().int16(),
                ],
            )
            .unwrap();
        for (id, sequence, attributes) in
            [("one", 1, 0x4000), ("two", 2, 0x4000), ("loose", 3, 0x2000)]
        {
            package
                .insert_rows(Insert::into("File").row(vec![
                    Value::Str(id.into()),
                    Value::Str("component".into()),
                    Value::Str(format!("{id}|{id}.txt")),
                    Value::Int(5),
                    Value::Int(sequence),
                    Value::Int(attributes),
                ]))
                .unwrap();
        }
        let mut reader = InstallerPackage::open(package.into_inner().unwrap(), 100).unwrap();
        let files = reader.files().unwrap();
        assert_eq!(files[0].cabinet.as_deref(), Some("#first.cab"));
        assert_eq!(files[1].cabinet.as_deref(), Some("second.cab"));
        assert_eq!(files[2].cabinet, None);
        assert_eq!(files[2].path, "Destination/loose.txt");
        assert_eq!(files[2].source_path, "Source/loose.txt");
        assert!(matches!(
            InstallerPackage::open(reader.package.into_inner().unwrap(), 1)
                .unwrap()
                .files(),
            Err(Error::Limit(_))
        ));
    }
    #[test]
    fn embedded_external_and_missing_cabinet_media() {
        let mut cab = cabinet::CabinetBuilder::new(cabinet::WriteCompression::None);
        cab.add_file("payload", b"hello").unwrap();
        let mut output = Cursor::new(Vec::new());
        cab.write(&mut output).unwrap();
        let cabinet = output.into_inner();
        let mut package =
            msi::Package::create(msi::PackageType::Installer, Cursor::new(Vec::new())).unwrap();
        package
            .write_stream("data.cab")
            .unwrap()
            .write_all(&cabinet)
            .unwrap();
        let mut reader = InstallerPackage::open(package.into_inner().unwrap(), 100).unwrap();
        let mut file = InstallerFile {
            id: "payload".into(),
            path: "app/hello.txt".into(),
            source_path: "app/hello.txt".into(),
            size: 5,
            sequence: 1,
            cabinet: Some("#data.cab".into()),
        };
        assert_eq!(
            reader.read_file(&file, &mut Resolver(None), 4096).unwrap(),
            b"hello"
        );
        file.cabinet = Some("external.cab".into());
        assert_eq!(
            reader
                .read_file(&file, &mut Resolver(Some(cabinet)), 4096)
                .unwrap(),
            b"hello"
        );
        assert!(matches!(
            reader.read_file(&file, &mut Resolver(None), 4096),
            Err(Error::MissingMedia(_))
        ));
        file.cabinet = None;
        assert_eq!(
            reader
                .read_file(&file, &mut Resolver(Some(b"hello".to_vec())), 4096)
                .unwrap(),
            b"hello"
        );
        assert!(matches!(
            reader.read_file(&file, &mut Resolver(Some(b"too long".to_vec())), 4096),
            Err(Error::Integrity(_))
        ));
    }
    #[test]
    fn rejects_storage_larger_than_explicit_budget() {
        assert!(matches!(
            InstallerPackage::open_bounded(Cursor::new(vec![0; 4096]), 100, 1024),
            Err(Error::Limit(_))
        ));
    }
    #[test]
    fn raw_stream_reads_enforce_actual_byte_limit() {
        let mut package =
            msi::Package::create(msi::PackageType::Installer, Cursor::new(Vec::new())).unwrap();
        package
            .write_stream("cabinet")
            .unwrap()
            .write_all(b"payload")
            .unwrap();
        let mut reader = InstallerPackage::open(package.into_inner().unwrap(), 100).unwrap();
        assert_eq!(reader.streams(), ["cabinet"]);
        assert!(matches!(
            reader.read_stream("cabinet", 6),
            Err(Error::Limit(_))
        ));
        assert_eq!(reader.read_stream("cabinet", 7).unwrap(), b"payload");
    }
    #[test]
    fn directory_cycles_and_short_long_names_are_checked() {
        let dirs = BTreeMap::from([
            ("root".into(), (None, "SourceDir".into())),
            (
                "app".into(),
                (Some("root".into()), "APP~1|Application".into()),
            ),
        ]);
        assert_eq!(
            directory_path("app", &dirs, &mut BTreeSet::new()).unwrap(),
            ["Application"]
        );
        let cycle = BTreeMap::from([
            ("a".into(), (Some("b".into()), ".".into())),
            ("b".into(), (Some("a".into()), ".".into())),
        ]);
        assert!(directory_path("a", &cycle, &mut BTreeSet::new()).is_err());
    }
    #[test]
    fn malformed_compound_storage_is_rejected() {
        assert!(InstallerPackage::open(Cursor::new(b"not compound storage"), 100).is_err());
    }
}
