use ms_package::authoring::{AppxBuilder, AppxEditor, WriteOptions};
use std::io::Cursor;
use wasm_bindgen::prelude::*;

pub const MANIFEST: &[u8] = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Authoring.Test" Publisher="CN=Test" Version="1.0.0.0" ProcessorArchitecture="x64"/><Properties><DisplayName>Authoring Test</DisplayName><PublisherDisplayName>Test</PublisherDisplayName><Logo>logo.png</Logo></Properties><Resources><Resource Language="en-us"/></Resources><Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.26100.0"/></Dependencies></Package>"#;

/// Creates the same deterministic package on the host and in a browser Worker.
#[wasm_bindgen]
pub fn create_package(payload: &[u8], maximum: u64) -> Result<Vec<u8>, JsValue> {
    create_native(payload, maximum).map_err(|e| JsValue::from_str(&e.to_string()))
}

pub fn create_native(payload: &[u8], maximum: u64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut options = WriteOptions::default();
    options.limits.max_file_bytes = maximum;
    let mut builder = AppxBuilder::new(MANIFEST, options)?;
    builder.add_file("payload.txt", payload)?;
    builder.add_file("logo.png", &b"test logo"[..])?;
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output)?;
    Ok(output.into_inner())
}

/// Replaces one payload and rebuilds both integrity and content-type metadata.
#[wasm_bindgen]
pub fn edit_package(package: &[u8], payload: &[u8]) -> Result<Vec<u8>, JsValue> {
    let result = (|| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut editor = AppxEditor::open(Cursor::new(package), WriteOptions::default())?;
        editor.replace_file("payload.txt", payload)?;
        let mut output = Cursor::new(Vec::new());
        editor.write(&mut output)?;
        Ok(output.into_inner())
    })();
    result.map_err(|e| JsValue::from_str(&e.to_string()))
}

pub fn edit_native(package: &[u8], payload: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut editor = AppxEditor::open(Cursor::new(package), WriteOptions::default())?;
    editor.replace_file("payload.txt", payload)?;
    let mut output = Cursor::new(Vec::new());
    editor.write(&mut output)?;
    Ok(output.into_inner())
}

pub fn create_bundle_native(
    payload: &[u8],
    maximum: u64,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use ms_package::authoring::AppxBundleBuilder;
    let nested = create_native(payload, maximum)?;
    let mut bundle = AppxBundleBuilder::new(
        "Authoring.Test",
        "CN=Test",
        "1.0.0.0",
        WriteOptions::default(),
    )?;
    bundle.add_package("nested.msix", Cursor::new(nested))?;
    let mut output = Cursor::new(Vec::new());
    bundle.write(&mut output)?;
    Ok(output.into_inner())
}

#[wasm_bindgen]
pub fn create_bundle(payload: &[u8], maximum: u64) -> Result<Vec<u8>, JsValue> {
    create_bundle_native(payload, maximum).map_err(|error| JsValue::from_str(&error.to_string()))
}

pub fn edit_bundle_native(
    bytes: &[u8],
    payload: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use ms_package::authoring::AppxBundleEditor;
    let nested = create_native(payload, 1 << 20)?;
    let mut editor = AppxBundleEditor::open(Cursor::new(bytes), WriteOptions::default())?;
    editor.replace_package("nested.msix", Cursor::new(nested))?;
    let mut output = Cursor::new(Vec::new());
    editor.write(&mut output)?;
    Ok(output.into_inner())
}

#[wasm_bindgen]
pub fn edit_bundle(bytes: &[u8], payload: &[u8]) -> Result<Vec<u8>, JsValue> {
    edit_bundle_native(bytes, payload).map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen]
pub fn verify_bundle(bytes: &[u8], payload: &[u8]) -> Result<(), JsValue> {
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut bundle =
            ms_package::AppxBundle::open(Cursor::new(bytes), Default::default(), 1 << 20)?;
        bundle.validate(1 << 20)?;
        let mut nested = bundle.select("nested.msix", Default::default(), 1 << 20, 1 << 20)?;
        nested.validate(1 << 20)?;
        let id = nested
            .entries()
            .iter()
            .find(|entry| entry.name == "payload.txt")
            .ok_or("missing payload")?
            .id;
        if nested.read_entry(id, 1 << 20)? != payload {
            return Err("bundle payload mismatch".into());
        }
        Ok(())
    })();
    result.map_err(|error| JsValue::from_str(&error.to_string()))
}

pub fn create_database_native(
    payload: &[u8],
    maximum: u64,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use ms_package::authoring::InstallerDatabaseBuilder;
    let mut options = WriteOptions::default();
    options.limits.max_file_bytes = maximum;
    let mut database = InstallerDatabaseBuilder::new(options)?;
    database.create_table(
        "Custom",
        vec![msi::Column::build("Key").primary_key().string(64)],
    )?;
    database.insert_rows(
        "Custom",
        vec![vec![msi::Value::Str("WorkerEvidence".into())]],
    )?;
    database.write_stream("Payload", payload)?;
    let mut output = Vec::new();
    database.write(&mut output)?;
    Ok(output)
}

#[wasm_bindgen]
pub fn create_database(payload: &[u8], maximum: u64) -> Result<Vec<u8>, JsValue> {
    create_database_native(payload, maximum).map_err(|error| JsValue::from_str(&error.to_string()))
}

pub fn edit_database_native(
    bytes: &[u8],
    payload: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut editor =
        ms_package::authoring::InstallerEditor::open(Cursor::new(bytes), WriteOptions::default())?;
    editor.write_stream("Payload", payload)?;
    let mut output = Vec::new();
    editor.write(&mut output)?;
    Ok(output)
}

#[wasm_bindgen]
pub fn edit_database(bytes: &[u8], payload: &[u8]) -> Result<Vec<u8>, JsValue> {
    edit_database_native(bytes, payload).map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen]
pub fn verify_database(bytes: &[u8], payload: &[u8]) -> Result<(), JsValue> {
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut database = ms_package::InstallerPackage::open_with_metadata_limit(
            Cursor::new(bytes),
            100,
            1 << 20,
            1 << 20,
        )?;
        if database.read_stream("Payload", 1 << 20)? != payload {
            return Err("database stream mismatch".into());
        }
        let table = database.table("Custom")?;
        if table.rows != vec![vec![msi::Value::Str("WorkerEvidence".into())]] {
            return Err("database table mismatch".into());
        }
        Ok(())
    })();
    result.map_err(|error| JsValue::from_str(&error.to_string()))
}
