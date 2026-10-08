//! Produces deterministic boundary artifacts for independent DEFLATE qualification.
#[cfg(feature = "write")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use ms_package::authoring::{AppxBuilder, AppxCompression, AppxEditor, WriteOptions};
    use std::io::Cursor;
    let destination = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/data/cache/ms-package-deflate-native-20261009.msix".into());
    let manifest = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Authoring.Test" Publisher="CN=Test" Version="1.0.0.0" ProcessorArchitecture="x64"/><Properties><DisplayName>Authoring Test</DisplayName><PublisherDisplayName>Test</PublisherDisplayName><Logo>logo.png</Logo></Properties><Resources><Resource Language="en-us"/></Resources><Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.26100.0"/></Dependencies></Package>"#;
    let mut builder = AppxBuilder::new(manifest.as_slice(), WriteOptions::default())?;
    builder.set_compression(AppxCompression::Deflate);
    builder.add_file("logo.png", &b"test logo"[..])?;
    for size in [0, 1, 65535, 65536, 65537, 131072, 196609] {
        builder.add_file(
            &format!("repetitive-{size}.bin"),
            Cursor::new(vec![42; size]),
        )?;
        let mut state = 0x12345678u32;
        let random: Vec<u8> = (0..size)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        builder.add_file(&format!("incompressible-{size}.bin"), Cursor::new(random))?;
    }
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output)?;
    std::fs::write(&destination, output.get_ref())?;
    let mut editor = AppxEditor::open(output, WriteOptions::default())?;
    editor.set_compression(AppxCompression::Deflate);
    editor.replace_file("repetitive-65537.bin", Cursor::new(vec![11; 65537]))?;
    editor.rename_file("incompressible-131072.bin", "renamed-131072.bin")?;
    let mut edited = Cursor::new(Vec::new());
    editor.write(&mut edited)?;
    std::fs::write(format!("{destination}.edited.msix"), edited.into_inner())?;
    Ok(())
}
#[cfg(not(feature = "write"))]
fn main() {
    eprintln!("Run with --features write");
}
