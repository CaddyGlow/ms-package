//! Experimental file-only MSI for independent installation qualification.

#[cfg(feature = "write")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use ms_package::authoring::{
        InstallationContext, InstallerArchitecture, InstallerBuilder, InstallerCabinetSpec,
        InstallerIdentity, InstallerMediaLayout, InstallerMediaSink, WriteOptions,
    };
    use std::io::Cursor;

    let path = std::env::args_os()
        .nth(1)
        .ok_or("supply a distinct output .msi path")?;
    let architecture = match std::env::args().nth(2).as_deref().unwrap_or("x64") {
        "x64" => InstallerArchitecture::X64,
        "x86" => InstallerArchitecture::X86,
        _ => return Err("architecture must be x86 or x64".into()),
    };
    let context = match std::env::args().nth(3).as_deref().unwrap_or("per-user") {
        "per-user" => InstallationContext::PerUser,
        "per-machine" => InstallationContext::PerMachine,
        _ => return Err("context must be per-user or per-machine".into()),
    };
    let identity = InstallerIdentity {
        product_code: "{A09C9465-494B-4C24-BEFC-BCF975E23526}".into(),
        package_code: "{CE6F227A-D92D-45F9-BCAA-F432D2A2BFC6}".into(),
        upgrade_code: "{29079377-0EF5-4891-B138-B27804FAB594}".into(),
        name: "ms-package authoring qualification".into(),
        manufacturer: "ms-package".into(),
        version: "1.0.0".into(),
        directory_name: "ms-package-authoring-test".into(),
        architecture,
        context,
    };
    let mut builder = InstallerBuilder::new(identity, WriteOptions::default())?;
    builder.add_file(
        "Payload",
        "payload.txt",
        "{BC2C8871-6A34-44B4-A757-19A564CEAD66}",
        Cursor::new(b"ms-package file-only qualification payload\r\n"),
    )?;
    let media = match std::env::args().nth(4).as_deref().unwrap_or("embedded") {
        "embedded" => InstallerMediaLayout::Embedded,
        "external" => InstallerMediaLayout::ExternalCabinet {
            name: "payload.cab".into(),
        },
        "loose" => InstallerMediaLayout::Loose,
        "split" => {
            builder.add_file(
                "Second",
                "second.txt",
                "{19D5683B-80A8-4A7A-A5FB-C34BC77032F1}",
                Cursor::new(b"second qualification payload\r\n"),
            )?;
            InstallerMediaLayout::Cabinets {
                cabinets: vec![
                    InstallerCabinetSpec {
                        name: "first.cab".into(),
                        file_count: 1,
                        embedded: false,
                    },
                    InstallerCabinetSpec {
                        name: "second.cab".into(),
                        file_count: 1,
                        embedded: false,
                    },
                ],
            }
        }
        _ => return Err("media must be embedded, external, loose, or split".into()),
    };
    struct FileSink(std::path::PathBuf);
    impl InstallerMediaSink for FileSink {
        type Writer = std::fs::File;
        fn create(&mut self, name: &str) -> std::io::Result<Self::Writer> {
            let path = self.0.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
        }
        fn finish(&mut self, _: &str, writer: Self::Writer) -> std::io::Result<()> {
            writer.sync_all()
        }
    }
    let path = std::path::PathBuf::from(path);
    let mut sink = FileSink(
        path.parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf(),
    );
    builder.write_with_media(media, &mut sink, std::fs::File::create(path)?)?;
    Ok(())
}

#[cfg(not(feature = "write"))]
fn main() {
    eprintln!("enable the write feature to run this example");
}
