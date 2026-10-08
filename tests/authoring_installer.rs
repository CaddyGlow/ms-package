#![cfg(feature = "write")]

use std::io::{self, Cursor, Read, Write};

use ms_package::authoring::{InstallerDatabaseBuilder, InstallerEditor, WriteOptions};

fn database() -> Vec<u8> {
    let mut builder = InstallerDatabaseBuilder::new(WriteOptions::default()).unwrap();
    builder
        .create_table(
            "Custom",
            vec![
                msi::Column::build("Key").primary_key().string(64),
                msi::Column::build("Value").nullable().string(0),
            ],
        )
        .unwrap();
    builder
        .insert_rows(
            "Custom",
            vec![vec![
                msi::Value::Str("item".into()),
                msi::Value::Str("preserved".into()),
            ]],
        )
        .unwrap();
    builder
        .write_stream("Unknown", Cursor::new(b"long stream content"))
        .unwrap();
    builder
        .edit_summary(|summary| summary.set_author("Original author"))
        .unwrap();
    let mut bytes = Vec::new();
    builder.write(&mut bytes).unwrap();
    bytes
}

#[test]
fn copy_edit_preserves_custom_schema_summary_and_truncates_stream() {
    let source = database();
    let mut editor = InstallerEditor::open(Cursor::new(&source), WriteOptions::default()).unwrap();
    editor.write_stream("Unknown", Cursor::new(b"x")).unwrap();
    let mut output = Vec::new();
    let report = editor.write(&mut output).unwrap();
    assert_eq!(report.changed_streams, ["Unknown"]);
    let mut package = msi::Package::open(Cursor::new(output)).unwrap();
    assert_eq!(package.summary_info().author(), Some("Original author"));
    let rows: Vec<_> = package
        .select_rows(msi::Select::table("Custom"))
        .unwrap()
        .collect();
    assert_eq!(rows[0]["Value"].as_str(), Some("preserved"));
    let mut bytes = Vec::new();
    package
        .read_stream("Unknown")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"x");
}

#[test]
fn row_edit_operations_preserve_primary_key_constraints() {
    let mut editor =
        InstallerEditor::open(Cursor::new(database()), WriteOptions::default()).unwrap();
    editor
        .update_rows(
            "Custom",
            vec![("Value".into(), msi::Value::Str("updated".into()))],
            None,
        )
        .unwrap();
    editor
        .insert_rows(
            "Custom",
            vec![vec![msi::Value::Str("other".into()), msi::Value::Null]],
        )
        .unwrap();
    editor
        .delete_rows(
            "Custom",
            Some(msi::Expr::col("Key").eq(msi::Expr::string("other"))),
        )
        .unwrap();
    let mut output = Vec::new();
    editor.write(&mut output).unwrap();
    let mut package = msi::Package::open(Cursor::new(output)).unwrap();
    let rows: Vec<_> = package
        .select_rows(msi::Select::table("Custom"))
        .unwrap()
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["Value"].as_str(), Some("updated"));
}

#[test]
fn signed_sources_reject_both_signature_policies() {
    let mut compound = cfb::CompoundFile::open(Cursor::new(database())).unwrap();
    compound
        .create_stream("/\u{5}MsiDigitalSignatureEx")
        .unwrap()
        .write_all(b"signature")
        .unwrap();
    let bytes = compound.into_inner().into_inner();
    for policy in [
        ms_package::authoring::SignaturePolicy::Reject,
        ms_package::authoring::SignaturePolicy::Remove,
    ] {
        let options = WriteOptions {
            signature_policy: policy,
            ..WriteOptions::default()
        };
        assert!(InstallerEditor::open(Cursor::new(&bytes), options).is_err());
    }
}

#[test]
fn source_rows_and_streams_are_bounded() {
    let source = database();
    let mut options = WriteOptions::default();
    options.limits.max_scratch_bytes = source.len() as u64 - 1;
    assert!(InstallerEditor::open(Cursor::new(&source), options).is_err());
    options = WriteOptions::default();
    options.limits.max_file_bytes = 2;
    assert!(InstallerEditor::open(Cursor::new(&source), options).is_err());
    options = WriteOptions::default();
    options.limits.max_rows = 0;
    assert!(InstallerEditor::open(Cursor::new(&source), options).is_err());
}

#[test]
fn unrepresentable_strings_are_rejected_and_failed_edits_cannot_emit() {
    let mut editor =
        InstallerEditor::open(Cursor::new(database()), WriteOptions::default()).unwrap();
    editor
        .set_database_codepage(msi::CodePage::UsAscii)
        .unwrap();
    assert!(
        editor
            .update_rows(
                "Custom",
                vec![("Value".into(), msi::Value::Str("snowman ☃".into()))],
                None
            )
            .is_err()
    );
    let mut output = Vec::new();
    assert!(editor.write(&mut output).is_err());
    assert!(output.is_empty());
}

#[test]
fn destination_flush_failure_is_reported() {
    struct FlushFailure;
    impl Write for FlushFailure {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("flush failure"))
        }
    }
    let editor = InstallerEditor::open(Cursor::new(database()), WriteOptions::default()).unwrap();
    assert!(editor.write(FlushFailure).is_err());
}

fn file_installer() -> Vec<u8> {
    use ms_package::authoring::{
        InstallationContext, InstallerArchitecture, InstallerBuilder, InstallerIdentity,
    };
    let identity = InstallerIdentity {
        product_code: "{A09C9465-494B-4C24-BEFC-BCF975E23526}".into(),
        package_code: "{CE6F227A-D92D-45F9-BCAA-F432D2A2BFC6}".into(),
        upgrade_code: "{29079377-0EF5-4891-B138-B27804FAB594}".into(),
        name: "Test product".into(),
        manufacturer: "Test manufacturer".into(),
        version: "1.0.0".into(),
        directory_name: "TestProduct".into(),
        architecture: InstallerArchitecture::X64,
        context: InstallationContext::PerUser,
    };
    let mut builder = InstallerBuilder::new(identity, WriteOptions::default()).unwrap();
    builder
        .add_file(
            "Payload",
            "payload.txt",
            "{BC2C8871-6A34-44B4-A757-19A564CEAD66}",
            Cursor::new(b"original payload"),
        )
        .unwrap();
    let mut output = Vec::new();
    builder.write(&mut output).unwrap();
    output
}

#[test]
fn minimal_profile_coordinates_cabinet_and_explicit_identities() {
    let output = file_installer();
    let mut reader = ms_package::InstallerPackage::open(Cursor::new(output), 10_000).unwrap();
    assert_eq!(reader.files().unwrap().len(), 1);
    assert_eq!(reader.summary().arch(), Some("x64"));
    let files = reader.table("File").unwrap();
    assert_eq!(files.rows.len(), 1);
    assert_eq!(files.rows[0][3].as_int(), Some(16));
}

#[test]
fn canonical_payload_edits_rebuild_all_related_metadata() {
    use ms_package::authoring::InstallerPayloadEditor;
    let mut editor = InstallerPayloadEditor::open(
        Cursor::new(file_installer()),
        "{ABE5397E-765C-4CEF-9DB0-203029DF0240}",
        WriteOptions::default(),
    )
    .unwrap();
    editor
        .replace_file("Payload", Cursor::new(b"replacement"))
        .unwrap();
    editor
        .rename_file(
            "Payload",
            "renamed.txt",
            "{9677FAB1-4181-4DE8-B45C-FEC3923246DA}",
        )
        .unwrap();
    editor
        .add_file(
            "Second",
            "second.txt",
            "{19D5683B-80A8-4A7A-A5FB-C34BC77032F1}",
            Cursor::new(b"second"),
        )
        .unwrap();
    let mut output = Vec::new();
    editor.write(&mut output).unwrap();
    let mut reader = ms_package::InstallerPackage::open(Cursor::new(&output), 10_000).unwrap();
    assert_eq!(reader.files().unwrap().len(), 2);
    assert_eq!(
        reader
            .summary()
            .uuid()
            .unwrap()
            .braced()
            .to_string()
            .to_ascii_uppercase(),
        "{ABE5397E-765C-4CEF-9DB0-203029DF0240}"
    );
    drop(reader);
    let mut editor = InstallerPayloadEditor::open(
        Cursor::new(output),
        "{FF219795-EE82-47A0-83F5-8611DF99B6F7}",
        WriteOptions::default(),
    )
    .unwrap();
    editor.remove_file("Second").unwrap();
    let mut final_output = Vec::new();
    editor.write(&mut final_output).unwrap();
    assert_eq!(
        ms_package::InstallerPackage::open(Cursor::new(final_output), 10_000)
            .unwrap()
            .files()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn payload_editor_rejects_unknown_preservation_and_reused_package_codes() {
    use ms_package::authoring::InstallerPayloadEditor;
    let source = file_installer();
    assert!(
        InstallerPayloadEditor::open(
            Cursor::new(&source),
            "{CE6F227A-D92D-45F9-BCAA-F432D2A2BFC6}",
            WriteOptions::default()
        )
        .is_err()
    );
    let mut editor = InstallerEditor::open(Cursor::new(&source), WriteOptions::default()).unwrap();
    editor
        .write_stream("Custom", Cursor::new(b"must not drop"))
        .unwrap();
    let mut changed = Vec::new();
    editor.write(&mut changed).unwrap();
    assert!(
        InstallerPayloadEditor::open(
            Cursor::new(changed),
            "{ABE5397E-765C-4CEF-9DB0-203029DF0240}",
            WriteOptions::default()
        )
        .is_err()
    );
}

#[test]
fn declared_foreign_keys_are_checked_before_destination_is_touched() {
    let mut builder = InstallerDatabaseBuilder::new(WriteOptions::default()).unwrap();
    builder
        .create_table(
            "Parent",
            vec![msi::Column::build("Key").primary_key().string(64)],
        )
        .unwrap();
    builder
        .create_table(
            "Child",
            vec![
                msi::Column::build("Key").primary_key().string(64),
                msi::Column::build("Parent_")
                    .foreign_key("Parent", 1)
                    .string(64),
            ],
        )
        .unwrap();
    builder
        .insert_rows(
            "Child",
            vec![vec![
                msi::Value::Str("child".into()),
                msi::Value::Str("missing".into()),
            ]],
        )
        .unwrap();
    let mut bytes = Vec::new();
    assert!(builder.write(&mut bytes).is_err());
    assert!(bytes.is_empty());
}
