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
    let manifest = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Authoring.Test" Publisher="CN=Author" Version="1.0.0.0" ProcessorArchitecture="neutral" ResourceId="resources"/><Properties><ResourcePackage>true</ResourcePackage></Properties></Package>"#;
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
