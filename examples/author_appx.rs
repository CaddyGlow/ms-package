//! Experimental stored unsigned package creation; no deployment qualification.
#[cfg(feature = "write")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use ms_package::authoring::{AppxBuilder, AppxEditor, WriteOptions};
    use std::io::Cursor;
    let manifest = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Example.Authoring" Publisher="CN=Example" Version="1.0.0.0" ProcessorArchitecture="neutral"/></Package>"#;
    let mut builder = AppxBuilder::new(manifest.as_slice(), WriteOptions::default())?;
    builder.add_file("hello.txt", Cursor::new(b"hello"))?;
    let mut package = Cursor::new(Vec::new());
    builder.write(&mut package)?;
    let mut editor = AppxEditor::open(package, WriteOptions::default())?;
    editor.replace_file("hello.txt", Cursor::new(b"updated"))?;
    let mut rebuilt = Cursor::new(Vec::new());
    println!("{:?}", editor.write(&mut rebuilt)?);
    Ok(())
}

#[cfg(not(feature = "write"))]
fn main() {
    eprintln!("Run with --features write");
}
