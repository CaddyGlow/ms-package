#![cfg(feature = "write")]
use ms_package::{
    AppxBundle,
    authoring::{AppxBuilder, AppxBundleBuilder, AppxBundleEditor, WriteOptions},
};
use std::io::Cursor;

fn package(architecture: &str, publisher: &str) -> Vec<u8> {
    let manifest = format!(
        "<Package xmlns=\"http://schemas.microsoft.com/appx/manifest/foundation/windows10\"><Identity Name=\"Authoring.Test\" Publisher=\"{}\" Version=\"1.0.0.0\" ProcessorArchitecture=\"{}\"/></Package>",
        publisher, architecture
    );
    let mut builder = AppxBuilder::new(manifest.into_bytes(), WriteOptions::default()).unwrap();
    builder
        .add_file("payload.txt", Cursor::new(b"payload"))
        .unwrap();
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output).unwrap();
    output.into_inner()
}

fn bundle() -> Vec<u8> {
    let mut builder = AppxBundleBuilder::new(
        "Authoring.Test",
        "CN=Author",
        "1.0.0.0",
        WriteOptions::default(),
    )
    .unwrap();
    builder
        .add_package("x64.msix", Cursor::new(package("x64", "CN=Author")))
        .unwrap();
    builder
        .add_package("x86.msix", Cursor::new(package("x86", "CN=Author")))
        .unwrap();
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output).unwrap();
    output.into_inner()
}

#[test]
fn bundles_bind_identities_and_verify_nested_contents() {
    let mut bundle = AppxBundle::open(
        Cursor::new(bundle()),
        archive_core::Limits::default(),
        1 << 20,
    )
    .unwrap();
    bundle.validate(1 << 20).unwrap();
    let mut nested = bundle
        .select(
            "x64.msix",
            archive_core::Limits::default(),
            1 << 20,
            1 << 20,
        )
        .unwrap();
    assert!(nested.validate(1 << 20).unwrap().bytes_verified > 0);
}

#[test]
fn bundle_editor_replaces_and_removes_packages() {
    let mut editor =
        AppxBundleEditor::open(Cursor::new(bundle()), WriteOptions::default()).unwrap();
    editor.remove_package("x86.msix").unwrap();
    editor
        .replace_package("x64.msix", Cursor::new(package("arm64", "CN=Author")))
        .unwrap();
    let mut output = Cursor::new(Vec::new());
    editor.write(&mut output).unwrap();
    let mut bundle = AppxBundle::open(
        Cursor::new(output.into_inner()),
        archive_core::Limits::default(),
        1 << 20,
    )
    .unwrap();
    assert_eq!(bundle.packages().len(), 1);
    assert_eq!(bundle.packages()[0].architecture, "arm64");
    bundle
        .select(
            "x64.msix",
            archive_core::Limits::default(),
            1 << 20,
            1 << 20,
        )
        .unwrap()
        .validate(1 << 20)
        .unwrap();
}

#[test]
fn bundle_rejects_duplicate_identity_and_publisher_mismatch() {
    let mut builder = AppxBundleBuilder::new(
        "Authoring.Test",
        "CN=Author",
        "1.0.0.0",
        WriteOptions::default(),
    )
    .unwrap();
    builder
        .add_package("one.msix", Cursor::new(package("x64", "CN=Author")))
        .unwrap();
    assert!(
        builder
            .add_package("two.msix", Cursor::new(package("x64", "CN=Author")))
            .is_err()
    );
    assert!(
        builder
            .add_package("wrong.msix", Cursor::new(package("x86", "CN=Other")))
            .is_err()
    );
}

#[test]
fn bundle_rejects_corrupted_nested_package_and_aggregate_limit() {
    let mut bytes = package("x64", "CN=Author");
    let offset = bytes.windows(7).position(|v| v == b"payload").unwrap();
    bytes[offset] ^= 1;
    let mut builder = AppxBundleBuilder::new(
        "Authoring.Test",
        "CN=Author",
        "1.0.0.0",
        WriteOptions::default(),
    )
    .unwrap();
    assert!(builder.add_package("bad.msix", Cursor::new(bytes)).is_err());
    let mut options = WriteOptions::default();
    options.limits.max_total_bytes = 10;
    let mut bounded =
        AppxBundleBuilder::new("Authoring.Test", "CN=Author", "1.0.0.0", options).unwrap();
    assert!(
        bounded
            .add_package("large.msix", Cursor::new(package("x64", "CN=Author")))
            .is_err()
    );
}

#[test]
fn recorded_bundle_offsets_match_zip_payload_locations() {
    let bytes = bundle();
    let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let inspected = AppxBundle::open(
        Cursor::new(&bytes),
        archive_core::Limits::default(),
        1 << 20,
    )
    .unwrap();
    let manifest = std::str::from_utf8(inspected.manifest()).unwrap();
    for package in inspected.packages() {
        let file = archive.by_name(&package.file_name).unwrap();
        assert!(manifest.contains(&format!(
            "FileName=\"{}\" Offset=\"{}\" Size=\"{}\"",
            package.file_name,
            file.data_start(),
            file.size()
        )));
    }
}

#[test]
fn resource_package_type_uses_property_and_preserves_resource_id() {
    let manifest = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Authoring.Test" Publisher="CN=Author" Version="1.0.0.0" ResourceId="resources"/><Properties><ResourcePackage>true</ResourcePackage></Properties></Package>"#;
    let builder = AppxBuilder::new(manifest.to_vec(), WriteOptions::default()).unwrap();
    let mut package = Cursor::new(Vec::new());
    builder.write(&mut package).unwrap();
    let mut builder = AppxBundleBuilder::new(
        "Authoring.Test",
        "CN=Author",
        "1.0.0.0",
        WriteOptions::default(),
    )
    .unwrap();
    builder
        .add_package("resource.msix", Cursor::new(package.into_inner()))
        .unwrap();
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output).unwrap();
    let bundle = AppxBundle::open(
        Cursor::new(output.into_inner()),
        archive_core::Limits::default(),
        1 << 20,
    )
    .unwrap();
    assert_eq!(bundle.packages()[0].package_type, "resource");
    assert_eq!(
        bundle.packages()[0].resource_id.as_deref(),
        Some("resources")
    );
}

#[test]
fn bundle_transfers_resource_qualifiers_and_rejects_unknown_attributes() {
    fn nested(qualifier: &str) -> Vec<u8> {
        let manifest = format!(
            r#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"><Identity Name="Authoring.Test" Publisher="CN=Author" Version="1.0.0.0" ProcessorArchitecture="neutral"/><Resources><Resource {qualifier}/></Resources></Package>"#
        );
        let package = AppxBuilder::new(manifest.into_bytes(), WriteOptions::default()).unwrap();
        let mut output = Cursor::new(Vec::new());
        package.write(&mut output).unwrap();
        output.into_inner()
    }
    let mut builder = AppxBundleBuilder::new(
        "Authoring.Test",
        "CN=Author",
        "1.0.0.0",
        WriteOptions::default(),
    )
    .unwrap();
    builder
        .add_package(
            "qualified.msix",
            Cursor::new(nested(
                r#"Language="en-us" uap:Scale="200" uap:DXFeatureLevel="dx11""#,
            )),
        )
        .unwrap();
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output).unwrap();
    let bundle = AppxBundle::open(
        Cursor::new(output.get_ref()),
        archive_core::Limits::default(),
        1 << 20,
    )
    .unwrap();
    assert!(std::str::from_utf8(bundle.manifest()).unwrap().contains(
        r#"<Resources><Resource DXFeatureLevel="dx11" Language="en-us" Scale="200"/></Resources>"#
    ));
    let editor =
        AppxBundleEditor::open(Cursor::new(output.into_inner()), WriteOptions::default()).unwrap();
    editor.write(Cursor::new(Vec::new())).unwrap();
    let mut builder = AppxBundleBuilder::new(
        "Authoring.Test",
        "CN=Author",
        "1.0.0.0",
        WriteOptions::default(),
    )
    .unwrap();
    assert!(
        builder
            .add_package(
                "unknown.msix",
                Cursor::new(nested(r#"Language="en-us" Unknown="value""#))
            )
            .is_err()
    );
}

#[test]
fn bundle_entry_limit_rejects_before_consuming_source() {
    struct MustNotRead;
    impl std::io::Read for MustNotRead {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            panic!("source consumed after entry limit was reached")
        }
    }
    let mut options = WriteOptions::default();
    options.limits.max_entries = 3;
    let mut builder =
        AppxBundleBuilder::new("Authoring.Test", "CN=Author", "1.0.0.0", options).unwrap();
    assert!(matches!(
        builder.add_package("blocked.msix", MustNotRead),
        Err(ms_package::authoring::WriteError::LimitExceeded(_))
    ));
}

#[test]
fn failed_replacement_and_nested_edit_preserve_previous_packages() {
    let original = bundle();
    let mut editor =
        AppxBundleEditor::open(Cursor::new(&original), WriteOptions::default()).unwrap();
    assert!(
        editor
            .replace_package("x64.msix", Cursor::new(package("x86", "CN=Author")))
            .is_err()
    );
    assert!(
        editor
            .replace_package("x64.msix", Cursor::new(package("x64", "CN=Other")))
            .is_err()
    );
    assert!(
        editor
            .edit_package("x64.msix", |nested| {
                nested.replace_file("payload.txt", &b"must not survive"[..])?;
                Err(ms_package::authoring::WriteError::InvalidInput(
                    "cancelled edit".into(),
                ))
            })
            .is_err()
    );
    let mut unchanged = Cursor::new(Vec::new());
    editor.write(&mut unchanged).unwrap();
    assert_eq!(unchanged.into_inner(), original);
}

#[test]
fn nested_payload_edit_updates_nested_and_outer_metadata() {
    let mut editor =
        AppxBundleEditor::open(Cursor::new(bundle()), WriteOptions::default()).unwrap();
    editor
        .edit_package("x64.msix", |nested| {
            nested.rename_file("payload.txt", "renamed.txt")?;
            nested.replace_file("renamed.txt", &b"nested edited payload"[..])
        })
        .unwrap();
    editor.remove_package("x86.msix").unwrap();
    assert!(editor.edit_package("x86.msix", |_| Ok(())).is_err());
    editor
        .add_package("new.msix", Cursor::new(package("arm64", "CN=Author")))
        .unwrap();
    let mut output = Cursor::new(Vec::new());
    editor.write(&mut output).unwrap();
    let mut bundle = AppxBundle::open(
        Cursor::new(output.into_inner()),
        archive_core::Limits::default(),
        1 << 20,
    )
    .unwrap();
    bundle.validate(1 << 20).unwrap();
    let mut nested = bundle
        .select(
            "x64.msix",
            archive_core::Limits::default(),
            1 << 20,
            1 << 20,
        )
        .unwrap();
    nested.validate(1 << 20).unwrap();
    let id = nested
        .entries()
        .iter()
        .find(|entry| entry.name == "renamed.txt")
        .unwrap()
        .id;
    assert_eq!(
        nested.read_entry(id, 100).unwrap(),
        b"nested edited payload"
    );
}

#[test]
fn bundle_identity_checks_reject_non_schema_values() {
    for (name, version) in [
        ("ab", "1.0.0.0"),
        ("invalid_name", "1.0.0.0"),
        ("Valid.Bundle", "+1.0.0.0"),
        ("Valid.Bundle", "65536.0.0.0"),
    ] {
        assert!(
            AppxBundleBuilder::new(name, "CN=Author", version, WriteOptions::default()).is_err()
        );
    }
}

#[test]
fn invalid_nested_identity_and_resource_property_fail_during_add() {
    let mut bundle = AppxBundleBuilder::new(
        "Authoring.Test",
        "CN=Author",
        "1.0.0.0",
        WriteOptions::default(),
    )
    .unwrap();
    assert!(
        bundle
            .add_package(
                "bad-architecture.msix",
                Cursor::new(package("bad", "CN=Author"))
            )
            .is_err()
    );
    for (resource_id, property) in [
        ("", "true"),
        ("invalid_id", "true"),
        ("resource", "true<!--separator-->false"),
    ] {
        let manifest = format!(
            r#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Authoring.Test" Publisher="CN=Author" Version="1.0.0.0" ProcessorArchitecture="neutral" ResourceId="{resource_id}"/><Properties><ResourcePackage>{property}</ResourcePackage></Properties></Package>"#
        );
        let package = AppxBuilder::new(manifest.into_bytes(), WriteOptions::default()).unwrap();
        let mut output = Cursor::new(Vec::new());
        package.write(&mut output).unwrap();
        assert!(
            bundle
                .add_package("bad-resource.msix", Cursor::new(output.into_inner()))
                .is_err()
        );
    }
}

#[test]
fn bundle_editor_rejects_custom_outer_content_types_without_discarding_them() {
    use std::io::{Read, Write};
    let bytes = bundle();
    let mut source = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut rebuilt = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..source.len() {
        let mut file = source.by_index(index).unwrap();
        let name = file.name().to_owned();
        let mut content = Vec::new();
        file.read_to_end(&mut content).unwrap();
        if name == "[Content_Types].xml" {
            content = String::from_utf8(content)
                .unwrap()
                .replace("application/octet-stream", "application/x-custom-package")
                .into_bytes();
        }
        rebuilt
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        rebuilt.write_all(&content).unwrap();
    }
    let bytes = rebuilt.finish().unwrap().into_inner();
    AppxBundle::open(
        Cursor::new(&bytes),
        archive_core::Limits::default(),
        1 << 20,
    )
    .unwrap()
    .validate(1 << 20)
    .unwrap();
    assert!(matches!(
        AppxBundleEditor::open(Cursor::new(bytes), WriteOptions::default()),
        Err(ms_package::authoring::WriteError::Unsupported(_))
    ));
}

#[test]
fn resource_semantic_rules_fail_before_bundle_emission() {
    fn resource(architecture: &str, forbidden: &str, qualifiers: &str) -> Vec<u8> {
        let manifest = format!(
            r#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"><Identity Name="Authoring.Test" Publisher="CN=Author" Version="1.0.0.0" ResourceId="resource" {architecture}/><Properties><ResourcePackage>1</ResourcePackage></Properties>{forbidden}<Resources>{qualifiers}</Resources></Package>"#
        );
        let package = AppxBuilder::new(manifest.into_bytes(), WriteOptions::default()).unwrap();
        let mut output = Cursor::new(Vec::new());
        package.write(&mut output).unwrap();
        output.into_inner()
    }
    for (architecture, forbidden, qualifiers) in [
        (
            r#"ProcessorArchitecture="neutral""#,
            "",
            r#"<Resource Language="en-us"/>"#,
        ),
        ("", "<Dependencies/>", r#"<Resource Language="en-us"/>"#),
        (
            "",
            r#"<Dependencies><PackageDependency Name="Other"/></Dependencies>"#,
            r#"<Resource Language="en-us"/>"#,
        ),
        (
            "",
            r#"<Dependencies><TargetDeviceFamily xmlns="foreign" Name="Windows.Desktop" MinVersion="10.0.0.0" MaxVersionTested="10.0.0.0"/></Dependencies>"#,
            r#"<Resource Language="en-us"/>"#,
        ),
        ("", "<Applications/>", r#"<Resource Language="en-us"/>"#),
        ("", "", r#"<Resource Language="en-us" uap:Scale="200"/>"#),
        (
            "",
            "",
            r#"<Resource Language="en-us"/><Resource uap:Scale="200"/>"#,
        ),
        (
            "",
            "",
            r#"<Resource Language="en-us"/><Resource Language="EN-US"/>"#,
        ),
    ] {
        let mut bundle = AppxBundleBuilder::new(
            "Authoring.Test",
            "CN=Author",
            "1.0.0.0",
            WriteOptions::default(),
        )
        .unwrap();
        assert!(
            bundle
                .add_package(
                    "resource.msix",
                    Cursor::new(resource(architecture, forbidden, qualifiers))
                )
                .is_err()
        );
    }
    let mut bundle = AppxBundleBuilder::new(
        "Authoring.Test",
        "CN=Author",
        "1.0.0.0",
        WriteOptions::default(),
    )
    .unwrap();
    bundle
        .add_package(
            "resource.msix",
            Cursor::new(resource(
                "",
                r#"<Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.26100.0"/></Dependencies>"#,
                r#"<Resource Language="en-us"/><Resource Language="fr-fr"/>"#,
            )),
        )
        .unwrap();
    bundle.write(Cursor::new(Vec::new())).unwrap();
}

#[test]
fn deflate_nested_packages_round_trip_and_enforce_decoded_file_limits() {
    use ms_package::authoring::AppxCompression;
    let manifest = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Authoring.Test" Publisher="CN=Author" Version="1.0.0.0" ProcessorArchitecture="x64"/></Package>"#;
    let mut package = AppxBuilder::new(manifest.as_slice(), WriteOptions::default()).unwrap();
    package.set_compression(AppxCompression::Deflate);
    package
        .add_file("payload.txt", &vec![b'x'; 65537][..])
        .unwrap();
    let mut compressed = Cursor::new(Vec::new());
    package.write(&mut compressed).unwrap();
    let mut limited = WriteOptions::default();
    limited.limits.max_file_bytes = 4096;
    assert!(compressed.get_ref().len() < 4096);
    let mut bounded =
        AppxBundleBuilder::new("Authoring.Test", "CN=Author", "1.0.0.0", limited).unwrap();
    assert!(matches!(
        bounded.add_package("compressed.msix", Cursor::new(compressed.get_ref())),
        Err(ms_package::authoring::WriteError::LimitExceeded(_))
    ));
    let mut bundle = AppxBundleBuilder::new(
        "Authoring.Test",
        "CN=Author",
        "1.0.0.0",
        WriteOptions::default(),
    )
    .unwrap();
    bundle
        .add_package("compressed.msix", Cursor::new(compressed.into_inner()))
        .unwrap();
    let mut output = Cursor::new(Vec::new());
    bundle.write(&mut output).unwrap();
    let mut editor =
        AppxBundleEditor::open(Cursor::new(output.into_inner()), WriteOptions::default()).unwrap();
    editor
        .edit_package("compressed.msix", |package| {
            package.set_compression(AppxCompression::Deflate);
            package.replace_file("payload.txt", &b"edited compressed payload"[..])
        })
        .unwrap();
    let mut output = Cursor::new(Vec::new());
    editor.write(&mut output).unwrap();
    let mut bundle = AppxBundle::open(
        Cursor::new(output.into_inner()),
        archive_core::Limits::default(),
        1 << 20,
    )
    .unwrap();
    bundle.validate(1 << 20).unwrap();
    let mut nested = bundle
        .select(
            "compressed.msix",
            archive_core::Limits::default(),
            1 << 20,
            1 << 20,
        )
        .unwrap();
    nested.validate(1 << 20).unwrap();
    let entry = nested
        .entries()
        .iter()
        .find(|entry| entry.name == "payload.txt")
        .unwrap()
        .id;
    assert_eq!(
        nested.read_entry(entry, 100).unwrap(),
        b"edited compressed payload"
    );
}
