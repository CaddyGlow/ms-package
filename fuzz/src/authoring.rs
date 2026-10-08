//! Stateful edit oracles compare independent expected payloads after every save.
use ms_package::authoring::{
    AppxBuilder, AppxCompression, AppxEditor, InstallationContext, InstallerArchitecture,
    InstallerBuilder, InstallerCabinetSpec, InstallerIdentity, InstallerMediaLayout,
    InstallerMediaSink, InstallerPayloadEditor, WriteOptions,
};
use std::{
    collections::BTreeMap,
    io::{self, Cursor},
};

const MANIFEST: &[u8] = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Fuzz.Authoring" Publisher="CN=Fuzz" Version="1.0.0.0" ProcessorArchitecture="x64"/></Package>"#;

fn options() -> WriteOptions {
    let mut options = WriteOptions::default();
    options.limits.max_entries = 128;
    options.limits.max_file_bytes = 4096;
    options.limits.max_total_bytes = 1 << 16;
    options.limits.max_metadata_bytes = 1 << 16;
    options.limits.max_output_bytes = 1 << 18;
    options.limits.max_scratch_bytes = 1 << 20;
    options.limits.max_rows = 4096;
    options
}

fn guid(domain: u32, value: usize) -> String {
    format!("{{{domain:08X}-0000-4000-8000-{value:012X}}}")
}

pub fn authoring(data: &[u8]) {
    // Each input has at most 32 operations and never supplies host paths.
    appx_edits(&data[..data.len().min(256)]);
    installer_edits(&data[..data.len().min(256)]);
}

fn appx_edits(data: &[u8]) {
    let mut expected = BTreeMap::new();
    let mut builder = AppxBuilder::new(MANIFEST, options()).unwrap();
    builder.set_compression(if data.first().is_some_and(|byte| byte & 4 != 0) {
        AppxCompression::Deflate
    } else {
        AppxCompression::Stored
    });
    for index in 0..4 {
        let name = format!("p{index}");
        builder.add_file(&name, &b"initial"[..]).unwrap();
        expected.insert(name, b"initial".to_vec());
    }
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output).unwrap();
    for operation in data.as_chunks::<8>().0 {
        let name = format!("p{}", operation[1] % 8);
        let target = format!("p{}", operation[2] % 8);
        let payload = &operation[4..4 + usize::from(operation[3] % 5)];
        let mut editor = AppxEditor::open(output, options()).unwrap();
        editor.set_compression(if operation[0] & 4 != 0 {
            AppxCompression::Deflate
        } else {
            AppxCompression::Stored
        });
        let result = match operation[0] % 4 {
            0 => editor.add_file(&name, payload),
            1 => editor.replace_file(&name, payload),
            2 => editor.remove_file(&name),
            _ => editor.rename_file(&name, &target),
        };
        let permitted = match operation[0] % 4 {
            0 => !expected.contains_key(&name),
            1 | 2 => expected.contains_key(&name),
            _ => {
                expected.contains_key(&name) && (name == target || !expected.contains_key(&target))
            }
        };
        assert_eq!(result.is_ok(), permitted, "APPX operation admission");
        if result.is_ok() {
            match operation[0] % 4 {
                0 | 1 => {
                    expected.insert(name, payload.to_vec());
                }
                2 => {
                    expected.remove(&name);
                }
                _ => {
                    let bytes = expected.remove(&name).unwrap();
                    expected.insert(target, bytes);
                }
            }
        }
        output = Cursor::new(Vec::new());
        editor.write(&mut output).unwrap();
        let mut reader =
            ms_package::AppxPackage::open(Cursor::new(output.get_ref()), super::limits(), 1 << 16)
                .unwrap();
        reader.validate(1 << 16).unwrap();
        let members: Vec<_> = reader
            .entries()
            .iter()
            .filter(|entry| entry.name.starts_with('p'))
            .map(|entry| (entry.name.clone(), entry.id))
            .collect();
        assert_eq!(members.len(), expected.len(), "APPX member set");
        for (name, id) in members {
            assert_eq!(
                &reader.read_entry(id, 4096).unwrap(),
                expected.get(&name).unwrap()
            );
        }
    }
}

struct Payload {
    name: String,
    bytes: Vec<u8>,
}
#[derive(Default)]
struct Media(BTreeMap<String, Vec<u8>>);
impl InstallerMediaSink for Media {
    type Writer = Vec<u8>;
    fn create(&mut self, _: &str) -> io::Result<Self::Writer> {
        Ok(Vec::new())
    }
    fn finish(&mut self, name: &str, writer: Self::Writer) -> io::Result<()> {
        assert!(
            self.0.insert(name.into(), writer).is_none(),
            "duplicate media artifact"
        );
        Ok(())
    }
}
impl ms_package::MediaResolver for Media {
    fn resolve(&mut self, name: &str, max: u64) -> ms_package::Result<Vec<u8>> {
        let bytes = self
            .0
            .get(name)
            .expect("declared media artifact must exist");
        assert!(bytes.len() as u64 <= max, "resolver media bound");
        Ok(bytes.clone())
    }
}
fn layout(mode: u8, files: usize) -> InstallerMediaLayout {
    match mode {
        0 => InstallerMediaLayout::Embedded,
        1 => InstallerMediaLayout::ExternalCabinet {
            name: "payload.cab".into(),
        },
        2 => InstallerMediaLayout::Loose,
        _ => InstallerMediaLayout::Cabinets {
            cabinets: (0..files)
                .map(|index| InstallerCabinetSpec {
                    name: format!("media{index}.cab"),
                    file_count: 1,
                    embedded: index % 2 == 0,
                })
                .collect(),
        },
    }
}

fn installer_edits(data: &[u8]) {
    let media_mode = data.first().copied().unwrap_or(0) % 4;
    let identity = InstallerIdentity {
        product_code: guid(1, 0),
        package_code: guid(2, 0),
        upgrade_code: guid(3, 0),
        name: "Fuzz authoring".into(),
        manufacturer: "Fuzz".into(),
        version: "1.0.0".into(),
        directory_name: "FuzzAuthoring".into(),
        architecture: InstallerArchitecture::X64,
        context: InstallationContext::PerUser,
    };
    let mut builder = InstallerBuilder::new(identity, options()).unwrap();
    let mut expected = BTreeMap::new();
    for index in 0..4 {
        let id = format!("p{index}");
        let name = format!("p{index}.txt");
        builder
            .add_file(&id, &name, &guid(4, index), &b"initial"[..])
            .unwrap();
        expected.insert(
            id,
            Payload {
                name,
                bytes: b"initial".to_vec(),
            },
        );
    }
    let mut media = Media::default();
    let mut output = Vec::new();
    builder
        .write_with_media(layout(media_mode, expected.len()), &mut media, &mut output)
        .unwrap();
    for (step, operation) in data.as_chunks::<8>().0.iter().enumerate() {
        let id = format!("p{}", operation[1] % 8);
        let target = format!("r{}.txt", operation[2] % 8);
        let payload = &operation[4..4 + usize::from(operation[3] % 5)];
        // A non-empty file set is the supported installer profile.
        if operation[0] % 4 == 2 && expected.len() == 1 {
            continue;
        }
        let mut editor = InstallerPayloadEditor::open_with_media(
            Cursor::new(output),
            &guid(2, step + 1),
            options(),
            &mut media,
        )
        .unwrap();
        let result = match operation[0] % 4 {
            0 => editor.add_file(&id, &format!("{id}.txt"), &guid(4, step + 16), payload),
            1 => editor.replace_file(&id, payload),
            2 => editor.remove_file(&id),
            _ => editor.rename_file(&id, &target, &guid(4, step + 16)),
        };
        let permitted = match operation[0] % 4 {
            0 => !expected.contains_key(&id) && !payload.starts_with(b"MZ"),
            1 => expected.contains_key(&id) && !payload.starts_with(b"MZ"),
            2 => expected.contains_key(&id),
            _ => expected.contains_key(&id) && expected.values().all(|entry| entry.name != target),
        };
        assert_eq!(result.is_ok(), permitted, "MSI operation admission");
        if result.is_ok() {
            match operation[0] % 4 {
                0 => {
                    expected.insert(
                        id.clone(),
                        Payload {
                            name: format!("{id}.txt"),
                            bytes: payload.to_vec(),
                        },
                    );
                }
                1 => expected.get_mut(&id).unwrap().bytes = payload.to_vec(),
                2 => {
                    expected.remove(&id);
                }
                _ => expected.get_mut(&id).unwrap().name = target,
            }
        }
        output = Vec::new();
        media = Media::default();
        editor
            .write_with_media(layout(media_mode, expected.len()), &mut media, &mut output)
            .unwrap();
        let mut reader =
            ms_package::InstallerPackage::open_bounded(Cursor::new(&output), 4096, 1 << 18)
                .unwrap();
        let files = reader.files().unwrap();
        assert_eq!(files.len(), expected.len(), "MSI file set");
        for file in files {
            let entry = expected.get(&file.id).unwrap();
            assert!(file.path.ends_with(&format!("/{}", entry.name)));
            assert_eq!(
                reader.read_file(&file, &mut media, 4096).unwrap(),
                entry.bytes
            );
        }
    }
}
