//! Synthetic package fixtures shared by native and browser parity checks.
use base64::{Engine, engine::general_purpose::STANDARD};
use msi::{Column, Insert, Value};
use sha2::{Digest, Sha256};
use std::{
    io::{Cursor, Write},
    path::PathBuf,
};
use zip::{ZipWriter, write::SimpleFileOptions};

fn appx(corrupt: bool) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    appx_arch(corrupt, "x64")
}
fn appx_arch(corrupt: bool, architecture: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    appx_profile(corrupt, Some(architecture), None)
}
fn appx_profile(
    corrupt: bool,
    architecture: Option<&str>,
    resource: Option<&str>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let architecture = architecture
        .map(|v| format!(" ProcessorArchitecture=\"{v}\""))
        .unwrap_or_default();
    let resource = resource
        .map(|v| format!(" ResourceId=\"{v}\""))
        .unwrap_or_default();
    let manifest = format!(
        "<Package xmlns=\"http://schemas.microsoft.com/appx/manifest/foundation/windows10\"><Identity Name=\"Browser.Test\" Publisher=\"CN=Test\" Version=\"1.0.0.0\"{architecture}{resource}/></Package>"
    );
    let manifest = manifest.as_bytes();
    let payload = b"portable package payload\n";
    let hash = |b: &[u8]| STANDARD.encode(Sha256::digest(b));
    let blockmap = format!(
        "<BlockMap xmlns=\"http://schemas.microsoft.com/appx/2010/blockmap\" HashMethod=\"http://www.w3.org/2001/04/xmlenc#sha256\"><File Name=\"AppxManifest.xml\" Size=\"{}\"><Block Hash=\"{}\"/></File><File Name=\"payload.txt\" Size=\"{}\"><Block Hash=\"{}\"/></File></BlockMap>",
        manifest.len(),
        hash(manifest),
        payload.len(),
        hash(payload)
    );
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [("AppxManifest.xml", manifest), ("[Content_Types].xml", b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"txt\" ContentType=\"text/plain\"/></Types>".as_slice()), ("AppxBlockMap.xml", blockmap.as_bytes()), ("payload.txt", if corrupt { b"damaged package payload!\n" } else { payload })] {
        zip.start_file(name, SimpleFileOptions::default())?;
        zip.write_all(bytes)?;
    }
    Ok(zip.finish()?.into_inner())
}

fn bundle(corrupt: bool) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let x64 = appx_arch(corrupt, "x64")?;
    let arm64 = appx_arch(false, "arm64")?;
    let manifest = format!(
        "<Bundle xmlns=\"http://schemas.microsoft.com/appx/2013/bundle\"><Identity Name=\"Browser.Test\" Publisher=\"CN=Test\" Version=\"1.0.0.0\"/><Packages><Package FileName=\"app-x64.msix\" Size=\"{}\" Version=\"1.0.0.0\" Architecture=\"x64\" Type=\"application\"/><Package FileName=\"app-arm64.msix\" Size=\"{}\" Version=\"1.0.0.0\" Architecture=\"arm64\" Type=\"application\"/></Packages></Bundle>",
        x64.len(),
        arm64.len()
    );
    let hash = STANDARD.encode(Sha256::digest(manifest.as_bytes()));
    let blockmap = format!(
        "<BlockMap xmlns=\"http://schemas.microsoft.com/appx/2010/blockmap\" HashMethod=\"http://www.w3.org/2001/04/xmlenc#sha256\"><File Name=\"AppxMetadata\\AppxBundleManifest.xml\" Size=\"{}\"><Block Hash=\"{hash}\"/></File></BlockMap>",
        manifest.len()
    );
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [("AppxMetadata/AppxBundleManifest.xml", manifest.as_bytes()), ("AppxBlockMap.xml", blockmap.as_bytes()), ("[Content_Types].xml", b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"msix\" ContentType=\"application/vnd.ms-appx\"/></Types>"), ("app-x64.msix", x64.as_slice()), ("app-arm64.msix", arm64.as_slice())] {
        zip.start_file(name, SimpleFileOptions::default())?;
        zip.write_all(bytes)?;
    }
    Ok(zip.finish()?.into_inner())
}

fn installer(embedded: bool) -> Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    let payload = b"portable package payload\n";
    let mut cab = cabinet::CabinetBuilder::new(cabinet::WriteCompression::MsZip);
    cab.add_file("payload", payload)?;
    let mut cabinet = Cursor::new(Vec::new());
    cab.write(&mut cabinet)?;
    let mut package = msi::Package::create(msi::PackageType::Installer, Cursor::new(Vec::new()))?;
    package.summary_info_mut().set_word_count(2);
    let cabinet = cabinet.into_inner();
    if embedded {
        package.write_stream("data.cab")?.write_all(&cabinet)?;
    }
    package.create_table(
        "Directory",
        vec![
            Column::build("Directory").primary_key().string(72),
            Column::build("Directory_Parent").nullable().string(72),
            Column::build("DefaultDir").string(255),
        ],
    )?;
    package.insert_rows(Insert::into("Directory").row(vec![
        Value::Str("TARGETDIR".into()),
        Value::Null,
        Value::Str("SourceDir".into()),
    ]))?;
    package.create_table(
        "Component",
        vec![
            Column::build("Component").primary_key().string(72),
            Column::build("ComponentId").nullable().string(38),
            Column::build("Directory_").string(72),
            Column::build("Attributes").int16(),
            Column::build("Condition").nullable().string(255),
            Column::build("KeyPath").nullable().string(72),
        ],
    )?;
    package.insert_rows(Insert::into("Component").row(vec![
        Value::Str("component".into()),
        Value::Null,
        Value::Str("TARGETDIR".into()),
        Value::Int(0),
        Value::Null,
        Value::Str("payload".into()),
    ]))?;
    package.create_table(
        "Media",
        vec![
            Column::build("DiskId").primary_key().int16(),
            Column::build("LastSequence").int32(),
            Column::build("DiskPrompt").nullable().string(64),
            Column::build("Cabinet").nullable().string(255),
            Column::build("VolumeLabel").nullable().string(32),
            Column::build("Source").nullable().string(72),
        ],
    )?;
    package.insert_rows(Insert::into("Media").row(vec![
        Value::Int(1),
        Value::Int(1),
        Value::Null,
        Value::Str(if embedded { "#data.cab" } else { "data.cab" }.into()),
        Value::Null,
        Value::Null,
    ]))?;
    package.create_table(
        "File",
        vec![
            Column::build("File").primary_key().string(72),
            Column::build("Component_").string(72),
            Column::build("FileName").string(255),
            Column::build("FileSize").int32(),
            Column::build("Version").nullable().string(72),
            Column::build("Language").nullable().string(20),
            Column::build("Attributes").nullable().int16(),
            Column::build("Sequence").int32(),
        ],
    )?;
    package.insert_rows(Insert::into("File").row(vec![
        Value::Str("payload".into()),
        Value::Str("component".into()),
        Value::Str("payload.txt".into()),
        Value::Int(payload.len() as i32),
        Value::Null,
        Value::Null,
        Value::Int(0x4000),
        Value::Int(1),
    ]))?;
    Ok((package.into_inner()?.into_inner(), cabinet))
}

fn installer_missing_second() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let (bytes, _) = installer(true)?;
    let mut package = msi::Package::open(Cursor::new(bytes))?;
    package.insert_rows(Insert::into("Media").row(vec![
        Value::Int(2),
        Value::Int(2),
        Value::Null,
        Value::Str("missing.cab".into()),
        Value::Null,
        Value::Null,
    ]))?;
    package.insert_rows(Insert::into("File").row(vec![
        Value::Str("second".into()),
        Value::Str("component".into()),
        Value::Str("second.txt".into()),
        Value::Int(25),
        Value::Null,
        Value::Null,
        Value::Int(0x4000),
        Value::Int(2),
    ]))?;
    Ok(package.into_inner()?.into_inner())
}

struct NoMedia;
impl ms_package::MediaResolver for NoMedia {
    fn resolve(&mut self, name: &str, _: u64) -> ms_package::Result<Vec<u8>> {
        Err(ms_package::Error::MissingMedia(name.into()))
    }
}

struct CabinetMedia(Vec<u8>);
impl ms_package::MediaResolver for CabinetMedia {
    fn resolve(&mut self, name: &str, max: u64) -> ms_package::Result<Vec<u8>> {
        if name != "data.cab" {
            return Err(ms_package::Error::MissingMedia(name.into()));
        }
        if self.0.len() as u64 > max {
            return Err(ms_package::Error::Limit("external cabinet bytes"));
        }
        Ok(self.0.clone())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("explicit output directory required")?,
    );
    std::fs::create_dir_all(&output)?;
    let valid = appx(false)?;
    let corrupt = appx(true)?;
    let (msi, _) = installer(true)?;
    let (external_msi, external_cab) = installer(false)?;
    let mut app = ms_package::AppxPackage::open(
        Cursor::new(valid.clone()),
        archive_core::Limits::default(),
        1024 * 1024,
    )?;
    let id = app
        .entries()
        .iter()
        .find(|e| e.name == "payload.txt")
        .ok_or("missing APPX payload")?
        .id;
    let app_bytes = app.read_entry(id, 1024 * 1024)?;
    let integrity = app.validate(1024 * 1024)?;
    let mut database = ms_package::InstallerPackage::open(Cursor::new(msi.clone()), 1000)?;
    let files = database.files()?;
    let file = files.first().ok_or("missing MSI payload")?;
    let installer_bytes = database.read_file(file, &mut NoMedia, 1024 * 1024)?;
    let valid_bundle = bundle(false)?;
    let mut bundled = ms_package::AppxBundle::open(
        Cursor::new(&valid_bundle),
        archive_core::Limits::default(),
        1024 * 1024,
    )?;
    let outer = bundled.validate(1024 * 1024)?;
    let mut selected = bundled.select(
        "app-x64.msix",
        archive_core::Limits::default(),
        1024 * 1024,
        1024 * 1024,
    )?;
    selected.validate(1024 * 1024)?;
    let mut external = ms_package::InstallerPackage::open(Cursor::new(external_msi.clone()), 1000)?;
    let external_files = external.files()?;
    let external_file = &external_files[0];
    if !matches!(
        external.read_file(external_file, &mut NoMedia, 1 << 20),
        Err(ms_package::Error::MissingMedia(_))
    ) {
        return Err("missing external media was not reported".into());
    }
    let external_bytes = external.read_file(
        external_file,
        &mut CabinetMedia(external_cab.clone()),
        1 << 20,
    )?;
    let expected = serde_json::json!({"appx":{"payload_name":"payload.txt","payload_bytes":app_bytes,"files_verified":integrity.files_verified,"bytes_verified":integrity.bytes_verified,"signature_verified":false},"bundle":{"package_names":["app-x64.msix","app-arm64.msix"],"selected":"app-x64.msix","payload_name":"payload.txt","payload_bytes":app_bytes,"outer_files_verified":outer.files_verified,"outer_bytes_verified":outer.bytes_verified,"signature_verified":false},"msi":{"payload_id":file.id,"payload_path":file.path,"payload_bytes":installer_bytes,"tables":database.tables(),"streams":database.streams()},"external_msi":{"payload_id":external_file.id,"payload_path":external_file.path,"payload_bytes":external_bytes,"cabinet":"data.cab","missing_media":"failure"},"corrupt_appx_integrity":"failure","corrupt_bundle_selected_integrity":"failure"});
    std::fs::write(output.join("valid.msix"), valid)?;
    std::fs::write(output.join("corrupt.msix"), corrupt)?;
    std::fs::write(output.join("embedded.msi"), msi)?;
    std::fs::write(output.join("external.msi"), external_msi)?;
    std::fs::write(output.join("data.cab"), external_cab)?;
    std::fs::write(
        output.join("missing-second.msi"),
        installer_missing_second()?,
    )?;
    std::fs::write(output.join("valid.msixbundle"), valid_bundle)?;
    std::fs::write(output.join("corrupt.msixbundle"), bundle(true)?)?;
    std::fs::write(
        output.join("expected.json"),
        serde_json::to_vec_pretty(&expected)?,
    )?;
    println!("{}", output.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_bundle_uses_standard_neutral_architecture_and_type_defaults() {
        let nested = appx_profile(false, None, Some("scale-100")).unwrap();
        let manifest = format!(
            "<Bundle xmlns=\"http://schemas.microsoft.com/appx/2013/bundle\"><Identity Name=\"Browser.Test\" Publisher=\"CN=Test\" Version=\"1.0.0.0\"/><Packages><Package FileName=\"resources.msix\" Version=\"1.0.0.0\" Size=\"{}\" ResourceId=\"scale-100\"/></Packages></Bundle>",
            nested.len()
        );
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in [
            ("resources.msix", nested.as_slice()),
            ("AppxMetadata/AppxBundleManifest.xml", manifest.as_bytes()),
        ] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        let mut bundle = ms_package::AppxBundle::open(
            Cursor::new(zip.finish().unwrap().into_inner()),
            archive_core::Limits::default(),
            1 << 20,
        )
        .unwrap();
        assert_eq!(bundle.packages()[0].architecture, "neutral");
        assert_eq!(bundle.packages()[0].package_type, "resource");
        assert_eq!(
            bundle.packages()[0].resource_id.as_deref(),
            Some("scale-100")
        );
        bundle
            .select(
                "resources.msix",
                archive_core::Limits::default(),
                1 << 20,
                1 << 20,
            )
            .unwrap()
            .validate(1 << 20)
            .unwrap();
    }
    #[test]
    fn ordinary_external_installer_media_matches_embedded_payload() {
        let (embedded, _) = installer(true).unwrap();
        let (external, cabinet) = installer(false).unwrap();
        let mut embedded = ms_package::InstallerPackage::open(Cursor::new(embedded), 1000).unwrap();
        let mut external = ms_package::InstallerPackage::open(Cursor::new(external), 1000).unwrap();
        let embedded_file = embedded.files().unwrap().remove(0);
        let external_file = external.files().unwrap().remove(0);
        assert!(
            matches!(external.read_file(&external_file, &mut NoMedia, 1 << 20), Err(ms_package::Error::MissingMedia(name)) if name == "data.cab")
        );
        let actual = external
            .read_file(&external_file, &mut CabinetMedia(cabinet), 1 << 20)
            .unwrap();
        assert_eq!(
            actual,
            embedded
                .read_file(&embedded_file, &mut NoMedia, 1 << 20)
                .unwrap()
        );
        let mut mixed = ms_package::InstallerPackage::open(
            Cursor::new(installer_missing_second().unwrap()),
            1000,
        )
        .unwrap();
        let files = mixed.files().unwrap();
        assert_eq!(
            mixed.read_file(&files[0], &mut NoMedia, 1 << 20).unwrap(),
            actual
        );
        assert!(
            matches!(mixed.read_file(&files[1], &mut NoMedia, 1 << 20), Err(ms_package::Error::MissingMedia(name)) if name == "missing.cab")
        );
    }
    fn rewrite_manifest(bytes: Vec<u8>, replace: impl FnOnce(String) -> String) -> Vec<u8> {
        let mut input = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut output = ZipWriter::new(Cursor::new(Vec::new()));
        let mut replace = Some(replace);
        for i in 0..input.len() {
            let mut entry = input.by_index(i).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
            if entry.name() == "AppxMetadata/AppxBundleManifest.xml" {
                bytes = replace.take().unwrap()(String::from_utf8(bytes).unwrap()).into_bytes();
            }
            output
                .start_file(entry.name(), SimpleFileOptions::default())
                .unwrap();
            output.write_all(&bytes).unwrap();
        }
        output.finish().unwrap().into_inner()
    }
    #[test]
    fn bundle_fixture_binding_integrity_and_explicit_selection_limits() {
        use archive_core::Limits;
        let valid = bundle(false).unwrap();
        let mut reader =
            ms_package::AppxBundle::open(Cursor::new(valid.clone()), Limits::default(), 1 << 20)
                .unwrap();
        assert_eq!(reader.validate(1 << 20).unwrap().files_verified, 1);
        assert!(reader.validate(1).is_err());
        assert!(
            reader
                .select("app-x64.msix", Limits::default(), 1, 1 << 20)
                .is_err()
        );
        assert!(
            reader
                .select(
                    "app-x64.msix",
                    Limits {
                        max_entries: 1,
                        ..Default::default()
                    },
                    1 << 20,
                    1 << 20
                )
                .is_err()
        );
        assert!(
            reader
                .select("app-x64.msix", Limits::default(), 1 << 20, 1)
                .is_err()
        );
        assert!(
            ms_package::AppxBundle::open(Cursor::new(valid.clone()), Limits::default(), 1).is_err()
        );
        for (from, to) in [
            ("CN=Test", "CN=Other"),
            ("Name=\"Browser.Test\"", "Name=\"Other.Name\""),
            ("Architecture=\"x64\"", "Architecture=\"arm64\""),
            (
                "Version=\"1.0.0.0\" Architecture",
                "Version=\"2.0.0.0\" Architecture",
            ),
            (
                "Architecture=\"x64\"",
                "Architecture=\"x64\" ResourceId=\"different-resource\"",
            ),
        ] {
            let bytes = rewrite_manifest(valid.clone(), |manifest| manifest.replace(from, to));
            let mut reader =
                ms_package::AppxBundle::open(Cursor::new(bytes), Limits::default(), 1 << 20)
                    .unwrap();
            assert!(
                reader
                    .select("app-x64.msix", Limits::default(), 1 << 20, 1 << 20)
                    .is_err()
            );
            assert!(reader.validate(1 << 20).is_err());
        }
        let mut reader = ms_package::AppxBundle::open(
            Cursor::new(bundle(true).unwrap()),
            Limits::default(),
            1 << 20,
        )
        .unwrap();
        reader.validate(1 << 20).unwrap();
        assert!(
            reader
                .select("app-x64.msix", Limits::default(), 1 << 20, 1 << 20)
                .unwrap()
                .validate(1 << 20)
                .is_err()
        );
        reader
            .select("app-arm64.msix", Limits::default(), 1 << 20, 1 << 20)
            .unwrap()
            .validate(1 << 20)
            .unwrap();
    }
}
