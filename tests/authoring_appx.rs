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

#[test]
fn reserved_metadata_files_cannot_be_used_as_payload_directories() {
    let mut builder = AppxBuilder::new(MANIFEST, WriteOptions::default()).unwrap();
    for name in [
        "AppxManifest.xml/child",
        "AppxBlockMap.xml/child",
        "[Content_Types].xml/child",
        "AppxSignature.p7x/child",
        "appxblockmap.XML/child",
        "AppxMetadata/child",
    ] {
        assert!(
            builder.add_file(name, Cursor::new(b"payload")).is_err(),
            "{name}"
        );
    }
}

#[test]
fn replacement_charges_final_decoded_size_and_case_only_rename_preserves_payload() {
    let mut builder = AppxBuilder::new(MANIFEST, WriteOptions::default()).unwrap();
    builder
        .add_file("PAYLOAD.bin", Cursor::new(vec![1; 10_000]))
        .unwrap();
    let mut source = Cursor::new(Vec::new());
    builder.write(&mut source).unwrap();
    let mut options = WriteOptions::default();
    options.limits.max_total_bytes = 16_000;
    let mut editor = AppxEditor::open(source, options).unwrap();
    editor
        .replace_file("PAYLOAD.bin", Cursor::new(vec![2; 10_000]))
        .unwrap();
    editor.rename_file("PAYLOAD.bin", "payload.bin").unwrap();
    let mut output = Cursor::new(Vec::new());
    editor.write(&mut output).unwrap();
    let mut package = AppxPackage::open(output, Default::default(), 1 << 20).unwrap();
    let id = package
        .entries()
        .iter()
        .find(|entry| entry.name == "payload.bin")
        .unwrap()
        .id;
    assert_eq!(package.read_entry(id, 16_000).unwrap(), vec![2; 10_000]);
}

#[test]
fn manifest_storage_is_bounded_before_retention() {
    for budget in ["entries", "decoded", "scratch"] {
        let mut options = WriteOptions::default();
        match budget {
            "entries" => options.limits.max_entries = 2,
            "decoded" => options.limits.max_total_bytes = MANIFEST.len() as u64 - 1,
            "scratch" => options.limits.max_scratch_bytes = MANIFEST.len() as u64 - 1,
            _ => unreachable!(),
        }
        assert!(AppxBuilder::new(MANIFEST, options).is_err(), "{budget}");
    }
}

#[test]
fn sparse_external_content_is_rejected_using_its_namespace_and_boolean_value() {
    for (namespace, value, allowed) in [
        (
            "http://schemas.microsoft.com/appx/manifest/uap/windows10/10",
            "true",
            false,
        ),
        (
            "http://schemas.microsoft.com/appx/manifest/uap/windows10/10",
            " 1 ",
            false,
        ),
        (
            "http://schemas.microsoft.com/appx/manifest/uap/windows10/10",
            " <!--split--> true ",
            false,
        ),
        (
            "http://schemas.microsoft.com/appx/manifest/uap/windows10/10",
            "invalid",
            false,
        ),
        (
            "http://schemas.microsoft.com/appx/manifest/uap/windows10/10",
            "false",
            true,
        ),
        (
            "http://schemas.microsoft.com/appx/manifest/uap/windows10/10",
            "0",
            true,
        ),
        ("urn:custom-extension", "true", true),
    ] {
        let manifest = format!(
            "<Package xmlns=\"http://schemas.microsoft.com/appx/manifest/foundation/windows10\" xmlns:external=\"{namespace}\"><Identity Name=\"Example\" Publisher=\"CN=Example\" Version=\"1.0.0.0\"/><Properties><external:AllowExternalContent>{value}</external:AllowExternalContent></Properties></Package>"
        );
        let builder = AppxBuilder::new(manifest.into_bytes(), WriteOptions::default()).unwrap();
        let mut output = Cursor::new(Vec::new());
        assert_eq!(
            builder.write(&mut output).is_ok(),
            allowed,
            "{namespace} {value}"
        );
        if !allowed {
            assert!(output.into_inner().is_empty());
        }
    }
}

#[test]
fn source_payload_limit_is_checked_before_integrity_extraction() {
    use ms_package::authoring::WriteError;
    let mut builder = AppxBuilder::new(MANIFEST, WriteOptions::default()).unwrap();
    builder
        .add_file("large.bin", Cursor::new(vec![1; 1024]))
        .unwrap();
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output).unwrap();
    let offset = {
        let mut zip = zip::ZipArchive::new(Cursor::new(output.get_ref())).unwrap();
        zip.by_name("large.bin").unwrap().data_start() as usize
    };
    output.get_mut()[offset] ^= 1;
    let mut options = WriteOptions::default();
    options.limits.max_file_bytes = 512;
    assert!(matches!(
        AppxEditor::open(output, options),
        Err(WriteError::LimitExceeded("file bytes"))
    ));
}

#[test]
fn deflate_block_boundaries_and_metadata_layout_round_trip() {
    use ms_package::authoring::AppxCompression;
    let mut builder = AppxBuilder::new(MANIFEST, WriteOptions::default()).unwrap();
    builder.set_compression(AppxCompression::Deflate);
    for size in [0, 1, 65535, 65536, 65537, 131072] {
        builder
            .add_file(&format!("payload-{size}.bin"), Cursor::new(vec![42; size]))
            .unwrap();
    }
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output).unwrap();
    let mut package =
        AppxPackage::open(Cursor::new(output.get_ref()), Default::default(), 1 << 20).unwrap();
    package.validate(1 << 20).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(output.get_ref())).unwrap();
    for name in [
        "AppxManifest.xml",
        "AppxBlockMap.xml",
        "[Content_Types].xml",
    ] {
        assert_eq!(
            zip.by_name(name).unwrap().compression(),
            zip::CompressionMethod::Stored
        );
    }
    for size in [0, 1, 65535, 65536, 65537, 131072] {
        let name = format!("payload-{size}.bin");
        let index = zip.file_names().position(|entry| entry == name).unwrap();
        let file = zip.by_index_raw(index).unwrap();
        assert_eq!(file.compression(), zip::CompressionMethod::DEFLATE);
        assert_eq!(
            file.data_start() - file.header_start(),
            30 + name.len() as u64
        );
        let id = package
            .entries()
            .iter()
            .find(|entry| entry.name == name)
            .unwrap()
            .id;
        assert_eq!(package.read_entry(id, 1 << 20).unwrap(), vec![42; size]);
    }
    let mut editor =
        AppxEditor::open(Cursor::new(output.into_inner()), WriteOptions::default()).unwrap();
    editor.set_compression(AppxCompression::Deflate);
    editor
        .replace_file("payload-65537.bin", Cursor::new(vec![11; 65537]))
        .unwrap();
    let mut result = Cursor::new(Vec::new());
    editor.write(&mut result).unwrap();
    let mut package = AppxPackage::open(result, Default::default(), 1 << 20).unwrap();
    package.validate(1 << 20).unwrap();
    let id = package
        .entries()
        .iter()
        .find(|entry| entry.name == "payload-65537.bin")
        .unwrap()
        .id;
    assert_eq!(package.read_entry(id, 1 << 20).unwrap(), vec![11; 65537]);
}

#[test]
fn compressed_editor_rejects_wrong_physical_block_sizes() {
    use ms_package::authoring::AppxCompression;
    let mut builder = AppxBuilder::new(MANIFEST, WriteOptions::default()).unwrap();
    builder.set_compression(AppxCompression::Deflate);
    builder
        .add_file("payload.bin", Cursor::new(vec![7; 65537]))
        .unwrap();
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output).unwrap();
    let mut source = zip::ZipArchive::new(output).unwrap();
    let mut changed = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..source.len() {
        let mut file = source.by_index_raw(index).unwrap();
        if file.name() == "AppxBlockMap.xml" {
            use std::io::Read;
            let mut bytes = String::new();
            file.read_to_string(&mut bytes).unwrap();
            let hash = bytes.find("<Block Hash=").unwrap();
            // The manifest's stored blocks have no Size. Find the first
            // compressed block and perturb its physical length only.
            let block = bytes[hash..].find(" Size=\"").unwrap() + hash + 7;
            let end = bytes[block..].find('"').unwrap() + block;
            let size: u64 = bytes[block..end].parse().unwrap();
            bytes.replace_range(block..end, &(size + 1).to_string());
            changed
                .start_file("AppxBlockMap.xml", zip::write::SimpleFileOptions::default())
                .unwrap();
            changed.write_all(bytes.as_bytes()).unwrap();
        } else {
            changed.raw_copy_file(file).unwrap();
        }
    }
    assert!(AppxEditor::open(changed.finish().unwrap(), WriteOptions::default()).is_err());
}

#[test]
fn deflate_workspace_limit_fails_before_destination_changes() {
    use ms_package::authoring::AppxCompression;
    let mut options = WriteOptions::default();
    options.limits.max_scratch_bytes = 65536;
    let mut builder = AppxBuilder::new(MANIFEST, options).unwrap();
    builder.set_compression(AppxCompression::Deflate);
    builder
        .add_file("small.bin", Cursor::new(b"small"))
        .unwrap();
    let mut destination = Cursor::new(b"unchanged".to_vec());
    assert!(builder.write(&mut destination).is_err());
    assert_eq!(destination.into_inner(), b"unchanged");
}
