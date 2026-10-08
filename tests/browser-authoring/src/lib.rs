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

pub fn compressed_native(
    payload: &[u8],
    bundle: bool,
    edited: bool,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use ms_package::authoring::{AppxBundleBuilder, AppxBundleEditor, AppxCompression};
    let mut package = AppxBuilder::new(MANIFEST, WriteOptions::default())?;
    package.set_compression(AppxCompression::Deflate);
    package.add_file("payload.txt", payload)?;
    package.add_file("logo.png", &b"test logo"[..])?;
    let mut bytes = Cursor::new(Vec::new());
    package.write(&mut bytes)?;
    if !bundle {
        if edited {
            let mut editor =
                AppxEditor::open(Cursor::new(bytes.into_inner()), WriteOptions::default())?;
            editor.set_compression(AppxCompression::Deflate);
            editor.replace_file("payload.txt", payload)?;
            let mut output = Cursor::new(Vec::new());
            editor.write(&mut output)?;
            return Ok(output.into_inner());
        }
        return Ok(bytes.into_inner());
    }
    let mut builder = AppxBundleBuilder::new(
        "Authoring.Test",
        "CN=Test",
        "1.0.0.0",
        WriteOptions::default(),
    )?;
    builder.add_package("nested.msix", Cursor::new(bytes.into_inner()))?;
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output)?;
    if edited {
        let mut editor =
            AppxBundleEditor::open(Cursor::new(output.into_inner()), WriteOptions::default())?;
        editor.edit_package("nested.msix", |package| {
            package.set_compression(AppxCompression::Deflate);
            package.replace_file("payload.txt", payload)
        })?;
        output = Cursor::new(Vec::new());
        editor.write(&mut output)?;
    }
    Ok(output.into_inner())
}

#[wasm_bindgen]
pub fn compressed_package(payload: &[u8], bundle: bool, edited: bool) -> Result<Vec<u8>, JsValue> {
    compressed_native(payload, bundle, edited)
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

#[derive(Default)]
struct MemoryMedia(std::collections::BTreeMap<String, Vec<u8>>);
impl ms_package::authoring::InstallerMediaSink for MemoryMedia {
    type Writer = Vec<u8>;
    fn create(&mut self, _name: &str) -> std::io::Result<Self::Writer> {
        Ok(Vec::new())
    }
    fn finish(&mut self, name: &str, writer: Self::Writer) -> std::io::Result<()> {
        self.0.insert(name.into(), writer);
        Ok(())
    }
}
impl ms_package::MediaResolver for MemoryMedia {
    fn resolve(&mut self, name: &str, maximum: u64) -> ms_package::Result<Vec<u8>> {
        let bytes = self
            .0
            .get(name)
            .ok_or_else(|| ms_package::Error::MissingMedia(name.into()))?;
        if bytes.len() as u64 > maximum {
            return Err(ms_package::Error::MissingMedia(
                "media exceeds requested limit".into(),
            ));
        }
        Ok(bytes.clone())
    }
}

pub fn installer_media_native(
    payload: &[u8],
    profile: u8,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use ms_package::authoring::*;
    let mut builder = InstallerBuilder::new(
        InstallerIdentity {
            product_code: "{ABE5397E-765C-4CEF-9DB0-203029DF0240}".into(),
            package_code: "{FF219795-EE82-47A0-83F5-8611DF99B6F7}".into(),
            upgrade_code: "{19D5683B-80A8-4A7A-A5FB-C34BC77032F1}".into(),
            name: "Worker Fixture".into(),
            manufacturer: "Test".into(),
            version: "1.0.0".into(),
            directory_name: "WorkerFixture".into(),
            architecture: InstallerArchitecture::X64,
            context: InstallationContext::PerUser,
        },
        WriteOptions::default(),
    )?;
    builder.add_file(
        "Payload",
        "payload.txt",
        "{19D5683B-80A8-4A7A-A5FB-C34BC77032F2}",
        payload,
    )?;
    builder.add_file(
        "Second",
        "second.txt",
        "{19D5683B-80A8-4A7A-A5FB-C34BC77032F3}",
        &b"second"[..],
    )?;
    let layout = match profile {
        0 => InstallerMediaLayout::ExternalCabinet {
            name: "external.cab".into(),
        },
        1 => InstallerMediaLayout::Loose,
        2 => InstallerMediaLayout::Cabinets {
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
        _ => return Err("unknown media profile".into()),
    };
    let mut media = MemoryMedia::default();
    let mut bytes = Vec::new();
    builder.write_with_media(layout, &mut media, &mut bytes)?;
    let mut reader = ms_package::InstallerPackage::open(Cursor::new(&bytes), 1000)?;
    for file in reader.files()? {
        let expected = if file.id == "Payload" {
            payload
        } else {
            &b"second"[..]
        };
        if reader.read_file(&file, &mut media, 1 << 20)? != expected {
            return Err("resolved media payload mismatch".into());
        }
    }
    drop(reader);
    // Length-delimited artifact set: deterministic native/Worker parity includes every external artifact.
    let mut result = Vec::new();
    for (name, data) in std::iter::once(("fixture.msi".to_owned(), bytes)).chain(media.0) {
        result.extend_from_slice(&(name.len() as u64).to_le_bytes());
        result.extend_from_slice(name.as_bytes());
        result.extend_from_slice(&(data.len() as u64).to_le_bytes());
        result.extend_from_slice(&data);
    }
    Ok(result)
}

#[wasm_bindgen]
pub fn installer_media(payload: &[u8], profile: u8) -> Result<Vec<u8>, JsValue> {
    installer_media_native(payload, profile).map_err(|error| JsValue::from_str(&error.to_string()))
}

/// Deterministic mixed payload spanning three APPX hash blocks.
pub fn compression_payload(edited: bool) -> Vec<u8> {
    (0..131_073u32)
        .map(|index| {
            if index % 4096 < 2048 {
                b'A' + u8::from(edited)
            } else {
                ((index.wrapping_mul(1664525).wrapping_add(index >> 5)) >> 11) as u8
            }
        })
        .collect()
}
