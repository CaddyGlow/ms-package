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

#[test]
fn payload_editor_rejects_identity_collisions_and_retired_components() {
    use ms_package::authoring::InstallerPayloadEditor;
    let source = file_installer();
    for code in [
        "{A09C9465-494B-4C24-BEFC-BCF975E23526}",
        "{29079377-0EF5-4891-B138-B27804FAB594}",
    ] {
        assert!(
            InstallerPayloadEditor::open(Cursor::new(&source), code, WriteOptions::default())
                .is_err()
        );
    }
    let mut editor = InstallerPayloadEditor::open(
        Cursor::new(source),
        "{ABE5397E-765C-4CEF-9DB0-203029DF0240}",
        WriteOptions::default(),
    )
    .unwrap();
    editor
        .rename_file(
            "Payload",
            "renamed.txt",
            "{9677FAB1-4181-4DE8-B45C-FEC3923246DA}",
        )
        .unwrap();
    assert!(
        editor
            .rename_file(
                "Payload",
                "payload.txt",
                "{BC2C8871-6A34-44B4-A757-19A564CEAD66}"
            )
            .is_err()
    );
    editor.remove_file("Payload").unwrap();
    assert!(
        editor
            .add_file(
                "Reused",
                "reused.txt",
                "{BC2C8871-6A34-44B4-A757-19A564CEAD66}",
                Cursor::new(b"data")
            )
            .is_err()
    );
}

#[test]
fn payload_editor_rejects_noncanonical_property_schema_without_panicking() {
    use ms_package::authoring::InstallerPayloadEditor;
    let mut database = InstallerDatabaseBuilder::new(WriteOptions::default()).unwrap();
    database
        .create_table(
            "Property",
            vec![
                msi::Column::build("Key").primary_key().string(72),
                msi::Column::build("Value").string(0),
            ],
        )
        .unwrap();
    database
        .insert_rows(
            "Property",
            vec![vec![
                msi::Value::Str("ProductCode".into()),
                msi::Value::Str("custom".into()),
            ]],
        )
        .unwrap();
    let mut source = Vec::new();
    database.write(&mut source).unwrap();
    assert!(
        InstallerPayloadEditor::open(
            Cursor::new(source),
            "{ABE5397E-765C-4CEF-9DB0-203029DF0240}",
            WriteOptions::default()
        )
        .is_err()
    );
}

#[test]
fn detailed_reports_distinguish_raw_validation_and_exact_identity_changes() {
    use ms_package::authoring::{InstallerPayloadEditor, InstallerValidationScope};
    let source = file_installer();
    let raw = InstallerEditor::open(Cursor::new(&source), WriteOptions::default()).unwrap();
    let report = raw.write_detailed(&mut Vec::new()).unwrap();
    assert_eq!(
        report.validation_scope,
        InstallerValidationScope::DatabaseOnly
    );
    assert!(report.identity_changes.is_empty());
    let mut editor = InstallerPayloadEditor::open(
        Cursor::new(source),
        "{ABE5397E-765C-4CEF-9DB0-203029DF0240}",
        WriteOptions::default(),
    )
    .unwrap();
    editor
        .rename_file(
            "Payload",
            "renamed.txt",
            "{9677FAB1-4181-4DE8-B45C-FEC3923246DA}",
        )
        .unwrap();
    let report = editor.write_detailed(&mut Vec::new()).unwrap();
    assert_eq!(
        report.validation_scope,
        InstallerValidationScope::CanonicalFileOnly
    );
    let names: Vec<_> = report
        .identity_changes
        .iter()
        .map(|change| change.name.as_str())
        .collect();
    assert_eq!(names, ["Component.C_Payload.ComponentId", "PackageCode"]);
    let change = report
        .identity_changes
        .iter()
        .find(|change| change.name == "PackageCode")
        .unwrap();
    assert_eq!(
        change.old_value.as_deref(),
        Some("{CE6F227A-D92D-45F9-BCAA-F432D2A2BFC6}")
    );
    assert_eq!(
        change.new_value.as_deref(),
        Some("{ABE5397E-765C-4CEF-9DB0-203029DF0240}")
    );
}

#[test]
fn incomplete_foreign_key_declarations_cannot_be_emitted() {
    let mut editor =
        InstallerEditor::open(Cursor::new(database()), WriteOptions::default()).unwrap();
    editor
        .update_rows(
            "_Validation",
            vec![("KeyTable".into(), msi::Value::Str("Custom".into()))],
            Some(msi::Expr::col("Table").eq(msi::Expr::string("Custom"))),
        )
        .unwrap();
    let mut output = Vec::new();
    assert!(editor.write(&mut output).is_err());
    assert!(output.is_empty());
}

#[test]
fn replacement_read_reserves_existing_payload_scratch() {
    use ms_package::authoring::InstallerPayloadEditor;
    use std::cell::Cell;
    use std::rc::Rc;
    struct Counted(Rc<Cell<u64>>);
    impl Read for Counted {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            output.fill(b'x');
            self.0.set(self.0.get() + output.len() as u64);
            Ok(output.len())
        }
    }
    let mut options = WriteOptions::default();
    options.limits.max_scratch_bytes = 100_000;
    options.limits.max_file_bytes = 100_000;
    options.limits.max_total_bytes = 100_000;
    let mut editor = InstallerPayloadEditor::open(
        Cursor::new(file_installer()),
        "{ABE5397E-765C-4CEF-9DB0-203029DF0240}",
        options,
    )
    .unwrap();
    let count = Rc::new(Cell::new(0));
    assert!(
        editor
            .replace_file("Payload", Counted(count.clone()))
            .is_err()
    );
    assert_eq!(count.get(), 100_000 - b"original payload".len() as u64 + 1);
    editor.write(&mut Vec::new()).unwrap();
}

#[test]
fn canonical_external_loose_and_partitioned_media_roundtrip() {
    use ms_package::authoring::{
        InstallerCabinetSpec, InstallerMediaLayout, InstallerMediaSink, InstallerPayloadEditor,
    };
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Media(BTreeMap<String, Vec<u8>>);
    impl InstallerMediaSink for Media {
        type Writer = Vec<u8>;
        fn create(&mut self, _: &str) -> io::Result<Self::Writer> {
            Ok(Vec::new())
        }
        fn finish(&mut self, name: &str, writer: Self::Writer) -> io::Result<()> {
            self.0.insert(name.into(), writer);
            Ok(())
        }
    }
    impl ms_package::MediaResolver for Media {
        fn resolve(&mut self, name: &str, max: u64) -> ms_package::Result<Vec<u8>> {
            let bytes = self
                .0
                .get(name)
                .ok_or_else(|| ms_package::Error::MissingMedia(name.into()))?;
            assert!(bytes.len() as u64 <= max);
            Ok(bytes.clone())
        }
    }
    let layouts = [
        InstallerMediaLayout::ExternalCabinet {
            name: "external.cab".into(),
        },
        InstallerMediaLayout::Loose,
        InstallerMediaLayout::Cabinets {
            cabinets: vec![
                InstallerCabinetSpec {
                    name: "embedded.cab".into(),
                    file_count: 1,
                    embedded: true,
                },
                InstallerCabinetSpec {
                    name: "external.cab".into(),
                    file_count: 1,
                    embedded: false,
                },
            ],
        },
    ];
    for layout in layouts {
        let mut editor = InstallerPayloadEditor::open(
            Cursor::new(file_installer()),
            "{ABE5397E-765C-4CEF-9DB0-203029DF0240}",
            WriteOptions::default(),
        )
        .unwrap();
        if matches!(layout, InstallerMediaLayout::Cabinets { .. }) {
            editor
                .add_file(
                    "Second",
                    "second.txt",
                    "{19D5683B-80A8-4A7A-A5FB-C34BC77032F1}",
                    Cursor::new(b"second"),
                )
                .unwrap();
        }
        let mut media = Media::default();
        let mut bytes = Vec::new();
        editor
            .write_with_media(layout.clone(), &mut media, &mut bytes)
            .unwrap();
        if matches!(layout, InstallerMediaLayout::ExternalCabinet { .. }) {
            let original = media.0["external.cab"].clone();
            media
                .0
                .get_mut("external.cab")
                .unwrap()
                .extend_from_slice(b"trailing");
            assert!(
                InstallerPayloadEditor::open_with_media(
                    Cursor::new(&bytes),
                    "{FF219795-EE82-47A0-83F5-8611DF99B6F7}",
                    WriteOptions::default(),
                    &mut media
                )
                .is_err()
            );
            media.0.insert("external.cab".into(), original);
            struct FailsAfterSeven(usize);
            impl Write for FailsAfterSeven {
                fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                    if self.0 == 7 {
                        return Err(io::Error::other("destination failed"));
                    }
                    let count = bytes.len().min(7 - self.0);
                    self.0 += count;
                    Ok(count)
                }
                fn flush(&mut self) -> io::Result<()> {
                    Ok(())
                }
            }
            let editor = InstallerPayloadEditor::open_with_media(
                Cursor::new(&bytes),
                "{FF219795-EE82-47A0-83F5-8611DF99B6F7}",
                WriteOptions::default(),
                &mut media,
            )
            .unwrap();
            let error = editor
                .write_with_media(layout.clone(), &mut media, FailsAfterSeven(0))
                .unwrap_err();
            match error {
                ms_package::authoring::WriteError::Media {
                    completed,
                    incomplete,
                    bytes_written,
                    ..
                } => {
                    assert_eq!(completed, ["external.cab"]);
                    assert_eq!(incomplete, "<MSI>");
                    assert_eq!(bytes_written, 7);
                }
                other => panic!("unexpected error: {other}"),
            }
        }
        let mut reopened = InstallerPayloadEditor::open_with_media(
            Cursor::new(&bytes),
            "{FF219795-EE82-47A0-83F5-8611DF99B6F7}",
            WriteOptions::default(),
            &mut media,
        )
        .unwrap();
        reopened
            .replace_file("Payload", Cursor::new(b"edited payload"))
            .unwrap();
        let mut output = Vec::new();
        reopened
            .write_with_media(layout, &mut media, &mut output)
            .unwrap();
        let mut reader = ms_package::InstallerPackage::open(Cursor::new(output), 1000).unwrap();
        let files = reader.files().unwrap();
        let payload = files.iter().find(|file| file.id == "Payload").unwrap();
        assert_eq!(
            reader.read_file(payload, &mut media, 1_000_000).unwrap(),
            b"edited payload"
        );
    }
}
