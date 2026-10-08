#![cfg(feature = "write")]
use ms_package::AppxPackage;
use ms_package::authoring::{AppxBuilder, AppxEditor, WriteOptions};
use std::io::{Cursor, Write};

const MANIFEST: &[u8] = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Example.Authoring" Publisher="CN=Example" Version="1.0.0.0" ProcessorArchitecture="neutral"/></Package>"#;

#[test]
fn boundary_hashes_and_edit_operations_round_trip() {
    let mut builder = AppxBuilder::new(MANIFEST, WriteOptions::default()).unwrap();
    for size in [0, 65535, 65536, 65537] {
        builder
            .add_file(&format!("data/{size}.bin"), Cursor::new(vec![42; size]))
            .unwrap();
    }
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output).unwrap();
    let mut package =
        AppxPackage::open(Cursor::new(output.get_ref()), Default::default(), 1 << 20).unwrap();
    assert_eq!(
        package.validate(1 << 20).unwrap().bytes_verified,
        MANIFEST.len() as u64 + 196608
    );
    let mut editor =
        AppxEditor::open(Cursor::new(output.into_inner()), WriteOptions::default()).unwrap();
    editor
        .replace_file("data/65535.bin", Cursor::new(b"changed"))
        .unwrap();
    editor
        .rename_file("data/65536.bin", "data/renamed.bin")
        .unwrap();
    editor.remove_file("data/0.bin").unwrap();
    editor.add_file("new.txt", Cursor::new(b"new")).unwrap();
    let mut result = Cursor::new(Vec::new());
    editor.write(&mut result).unwrap();
    let mut package = AppxPackage::open(result, Default::default(), 1 << 20).unwrap();
    package.validate(1 << 20).unwrap();
    assert_eq!(package.manifest(), MANIFEST);
    let id = package
        .entries()
        .iter()
        .find(|e| e.name == "data/65535.bin")
        .unwrap()
        .id;
    assert_eq!(package.read_entry(id, 100).unwrap(), b"changed");
}

#[test]
fn deterministic_output_and_actual_source_limits() {
    let make = || {
        let mut builder = AppxBuilder::new(MANIFEST, WriteOptions::default()).unwrap();
        builder
            .add_file("unicodé.txt", Cursor::new(b"same"))
            .unwrap();
        let mut out = Cursor::new(Vec::new());
        builder.write(&mut out).unwrap();
        out.into_inner()
    };
    assert_eq!(make(), make());
    let mut options = WriteOptions::default();
    options.limits.max_file_bytes = 3;
    let mut builder = AppxBuilder::new(MANIFEST, options).unwrap();
    assert!(builder.add_file("x", Cursor::new(b"four")).is_err());
    for name in [
        "../x",
        "x\\y",
        "CON.txt",
        "x.",
        "C:/x",
        "AppxSignature.p7x",
        "COM¹",
        "COM².txt",
        "COM³",
        "LPT¹",
        "LPT².bin",
        "LPT³",
        "directory/com¹.txt",
        "AppxMetadata",
        "appxmetadata",
    ] {
        assert!(builder.add_file(name, Cursor::new(b"")).is_err());
    }
}

#[test]
fn destination_flush_errors_are_reported() {
    struct FailFlush(Cursor<Vec<u8>>);
    impl Write for FailFlush {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.write(b)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("flush"))
        }
    }
    impl std::io::Seek for FailFlush {
        fn seek(&mut self, p: std::io::SeekFrom) -> std::io::Result<u64> {
            self.0.seek(p)
        }
    }
    let builder = AppxBuilder::new(MANIFEST, WriteOptions::default()).unwrap();
    assert!(builder.write(FailFlush(Cursor::new(Vec::new()))).is_err());
}

#[test]
fn signed_sources_reject_or_strip_signature() {
    use ms_package::authoring::SignaturePolicy;
    let builder = AppxBuilder::new(MANIFEST, WriteOptions::default()).unwrap();
    let mut source = Cursor::new(Vec::new());
    builder.write(&mut source).unwrap();
    let mut zip = zip::ZipWriter::new_append(source).unwrap();
    zip.start_file(
        "AppxSignature.p7x",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(b"synthetic signature footprint").unwrap();
    let signed = zip.finish().unwrap().into_inner();
    assert!(AppxEditor::open(Cursor::new(&signed), WriteOptions::default()).is_err());
    let options = WriteOptions {
        signature_policy: SignaturePolicy::Remove,
        ..Default::default()
    };
    let editor = AppxEditor::open(Cursor::new(signed), options).unwrap();
    let mut output = Cursor::new(Vec::new());
    assert!(editor.write(&mut output).unwrap().signature_removed);
    let zip = zip::ZipArchive::new(output).unwrap();
    assert!(!zip.file_names().any(|name| name == "AppxSignature.p7x"));
}

#[test]
fn zip_headers_match_blockmap_and_part_names_are_uris() {
    let mut builder = AppxBuilder::new(MANIFEST, WriteOptions::default()).unwrap();
    builder
        .add_file("space #é%.txt", Cursor::new(b"value"))
        .unwrap();
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(output.get_ref())).unwrap();
    for index in 0..zip.len() {
        let file = zip.by_index(index).unwrap();
        assert_eq!(file.compression(), zip::CompressionMethod::Stored);
        let header = file.header_start() as usize;
        let data = output.get_ref();
        assert_eq!(&data[header..header + 4], b"PK\x03\x04");
        assert_eq!(
            u16::from_le_bytes([data[header + 28], data[header + 29]]),
            0
        );
        assert_eq!(
            u16::from_le_bytes([data[header + 6], data[header + 7]]) & 8,
            0
        );
        assert_eq!(
            file.data_start() - file.header_start(),
            30 + file.name().len() as u64
        );
    }
    use std::io::Read;
    let mut types = String::new();
    zip.by_name("[Content_Types].xml")
        .unwrap()
        .read_to_string(&mut types)
        .unwrap();
    assert!(types.contains("space%20%23%C3%A9%25.txt"));
    let mut blocks = String::new();
    zip.by_name("AppxBlockMap.xml")
        .unwrap()
        .read_to_string(&mut blocks)
        .unwrap();
    assert!(blocks.contains(&format!("LfhSize=\"{}\"", 30 + "space #é%.txt".len())));
}

#[test]
fn read_failures_and_exact_output_budget() {
    struct FailedRead;
    impl std::io::Read for FailedRead {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("source"))
        }
    }
    let mut builder = AppxBuilder::new(MANIFEST, WriteOptions::default()).unwrap();
    assert!(builder.add_file("broken", FailedRead).is_err());
    builder.add_file("short", Cursor::new(b"a")).unwrap();
    assert!(builder.add_file("SHORT", Cursor::new(b"b")).is_err());
    let mut output = Cursor::new(Vec::new());
    let report = builder.write(&mut output).unwrap();
    let mut options = WriteOptions::default();
    options.limits.max_output_bytes = report.output_bytes;
    let mut builder = AppxBuilder::new(MANIFEST, options).unwrap();
    builder.add_file("short", Cursor::new(b"a")).unwrap();
    builder.write(Cursor::new(Vec::new())).unwrap();
    options.limits.max_output_bytes -= 1;
    let mut builder = AppxBuilder::new(MANIFEST, options).unwrap();
    builder.add_file("short", Cursor::new(b"a")).unwrap();
    assert!(builder.write(Cursor::new(Vec::new())).is_err());
}

#[test]
fn manifest_references_block_payload_removal() {
    let manifest = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Example.Authoring" Publisher="CN=Example" Version="1.0.0.0"/><Applications><Application Executable="program.exe"/></Applications></Package>"#;
    let mut builder = AppxBuilder::new(manifest.as_slice(), WriteOptions::default()).unwrap();
    builder
        .add_file("program.exe", Cursor::new(b"payload"))
        .unwrap();
    let mut package = Cursor::new(Vec::new());
    builder.write(&mut package).unwrap();
    let mut editor = AppxEditor::open(package, WriteOptions::default()).unwrap();
    editor.remove_file("program.exe").unwrap();
    assert!(editor.write(Cursor::new(Vec::new())).is_err());
}

#[test]
fn custom_mime_and_manifest_extensions_survive_rebuilding() {
    let manifest = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" xmlns:custom="urn:example"><Identity Name="Example.Authoring" Publisher="CN=Example" Version="1.0.0.0"/><custom:Extension custom:Attribute="retain"/></Package>"#;
    let mut builder = AppxBuilder::new(manifest.as_slice(), WriteOptions::default()).unwrap();
    builder.add_file("x.bin", Cursor::new(b"payload")).unwrap();
    let mut source = Cursor::new(Vec::new());
    builder.write(&mut source).unwrap();
    let mut archive = zip::ZipArchive::new(source).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    use std::io::Read;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if file.name() == "[Content_Types].xml" {
            bytes = String::from_utf8(bytes)
                .unwrap()
                .replace("application/octet-stream", "application/x-example-custom")
                .into_bytes();
        }
        zip.start_file(file.name(), zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    let source = zip.finish().unwrap();
    let mut editor = AppxEditor::open(source, WriteOptions::default()).unwrap();
    editor.rename_file("x.bin", "renamed.bin").unwrap();
    let mut result = Cursor::new(Vec::new());
    editor.write(&mut result).unwrap();
    let package = AppxPackage::open(result, Default::default(), 1 << 20).unwrap();
    assert_eq!(package.manifest(), manifest);
    assert!(
        String::from_utf8_lossy(package.content_types())
            .contains("PartName=\"/renamed.bin\" ContentType=\"application/x-example-custom\"")
    );
}

#[test]
fn short_reads_and_metadata_scratch_boundaries() {
    struct OneByte(Cursor<Vec<u8>>);
    impl std::io::Read for OneByte {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(&mut out[..1])
        }
    }
    let mut builder = AppxBuilder::new(MANIFEST, WriteOptions::default()).unwrap();
    builder
        .add_file("x", OneByte(Cursor::new(vec![1; 65537])))
        .unwrap();
    let mut result = Cursor::new(Vec::new());
    let report = builder.write(&mut result).unwrap();
    let mut options = WriteOptions::default();
    options.limits.max_scratch_bytes = report.decoded_bytes + 2 * report.output_bytes;
    let mut builder = AppxBuilder::new(MANIFEST, options).unwrap();
    builder.add_file("x", Cursor::new(vec![1; 65537])).unwrap();
    builder.write(Cursor::new(Vec::new())).unwrap();
    options.limits.max_scratch_bytes -= 1;
    let mut builder = AppxBuilder::new(MANIFEST, options).unwrap();
    builder.add_file("x", Cursor::new(vec![1; 65537])).unwrap();
    assert!(builder.write(Cursor::new(Vec::new())).is_err());
    options.limits.max_metadata_bytes = MANIFEST.len() as u64 - 1;
    assert!(AppxBuilder::new(MANIFEST, options).is_err());
}

#[test]
fn malformed_metadata_and_source_integrity_fail_before_output() {
    let builder =
        AppxBuilder::new(b"<Package><broken>".as_slice(), WriteOptions::default()).unwrap();
    let mut output = Cursor::new(Vec::new());
    assert!(builder.write(&mut output).is_err());
    assert!(output.into_inner().is_empty());
    let mut builder = AppxBuilder::new(MANIFEST, WriteOptions::default()).unwrap();
    builder.add_file("x", Cursor::new(b"original")).unwrap();
    let mut source = Cursor::new(Vec::new());
    builder.write(&mut source).unwrap();
    let mut archive = zip::ZipArchive::new(source).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    use std::io::Read;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if file.name() == "x" {
            bytes[0] ^= 1;
        }
        zip.start_file(file.name(), zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    assert!(AppxEditor::open(zip.finish().unwrap(), WriteOptions::default()).is_err());
}

#[test]
fn content_type_extensions_fail_instead_of_being_discarded() {
    let builder = AppxBuilder::new(MANIFEST, WriteOptions::default()).unwrap();
    let mut source = Cursor::new(Vec::new());
    builder.write(&mut source).unwrap();
    let mut archive = zip::ZipArchive::new(source).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    use std::io::Read;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if file.name() == "[Content_Types].xml" {
            bytes = String::from_utf8(bytes)
                .unwrap()
                .replace("<Override ", "<Override PreserveMe=\"custom\" ")
                .into_bytes();
        }
        zip.start_file(file.name(), zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    assert!(AppxEditor::open(zip.finish().unwrap(), WriteOptions::default()).is_err());
}
