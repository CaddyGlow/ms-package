use archive_core::Limits;
use std::io::Cursor;
fn limits() -> Limits {
    Limits {
        max_entries: 128,
        max_metadata_bytes: 1 << 20,
        max_entry_bytes: 1 << 20,
        max_total_bytes: 2 << 20,
        max_dictionary_bytes: 32 << 20,
        max_input_bytes: 1 << 20,
        max_active_workspace_bytes: 96 << 20,
        max_pending_output_bytes: 1 << 20,
        max_password_iterations: 1024,
        max_nesting_depth: 16,
        max_workers: 1,
    }
}
pub fn appx(data: &[u8]) {
    if data.len() > 1 << 20 {
        return;
    }
    if let Ok(mut package) = ms_package::AppxPackage::open(Cursor::new(data), limits(), 1 << 20) {
        let _ = package.validate(2 << 20);
    }
}
pub fn msi(data: &[u8]) {
    if data.len() > 1 << 20 {
        return;
    }
    if let Ok(mut package) =
        ms_package::InstallerPackage::open_bounded(Cursor::new(data), 128, 1 << 20)
    {
        let _ = package.files();
        for table in package.tables().into_iter().take(16) {
            let _ = package.table(&table);
        }
        for stream in package.streams().into_iter().take(16) {
            let _ = package.read_stream(&stream, 1 << 20);
        }
    }
}

pub fn run(target: &str, data: &[u8]) -> Result<(), &'static str> {
    match target {
        "appx" => appx(data),
        "msi" => msi(data),
        _ => return Err("unknown fuzz target"),
    }
    Ok(())
}

pub const TARGETS: &[&str] = &["appx", "msi"];
pub fn seeds(target: &str) -> Vec<Vec<u8>> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let valid = if target == "msi" {
        let package =
            msi::Package::create(msi::PackageType::Installer, Cursor::new(Vec::new())).unwrap();
        package.into_inner().unwrap().into_inner()
    } else {
        let manifest=br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="FuzzSeed" Publisher="CN=Fuzz" Version="1.0.0.0" ProcessorArchitecture="x64"/></Package>"#;
        let hash = STANDARD.encode(Sha256::digest(manifest));
        let blockmap = format!(
            r#"<BlockMap xmlns="http://schemas.microsoft.com/appx/2010/blockmap" HashMethod="http://www.w3.org/2001/04/xmlenc#sha256"><File Name="AppxManifest.xml" Size="{}"><Block Hash="{}"/></File></BlockMap>"#,
            manifest.len(),
            hash
        );
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name,bytes) in [("AppxManifest.xml",manifest.as_slice()),("AppxBlockMap.xml",blockmap.as_bytes()),("[Content_Types].xml",br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#.as_slice())] {
            zip.start_file(name,zip::write::SimpleFileOptions::default()).unwrap(); zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    };
    if target == "appx" {
        assert!(ms_package::AppxPackage::open(Cursor::new(&valid), limits(), 1 << 20).is_ok());
    } else {
        assert!(
            ms_package::InstallerPackage::open_bounded(Cursor::new(&valid), 128, 1 << 20).is_ok()
        );
    }
    vec![
        valid,
        include_bytes!("../../tests/fixtures/msi-null-column-type.msi").to_vec(),
        vec![],
        vec![0; 512],
    ]
}

#[cfg(test)]
mod smoke {
    #[test]
    fn corpus_and_truncations_exercise_owned_harnesses() {
        for target in super::TARGETS {
            for seed in super::seeds(target) {
                super::run(target, &seed).unwrap();
                for end in [0, seed.len() / 2, seed.len().saturating_sub(1)] {
                    super::run(target, &seed[..end]).unwrap();
                }
                for offset in (0..seed.len()).step_by((seed.len() / 16).max(1)) {
                    let mut mutation = seed.clone();
                    mutation[offset] ^= 0xff;
                    super::run(target, &mutation).unwrap();
                }
            }
        }
    }
}
