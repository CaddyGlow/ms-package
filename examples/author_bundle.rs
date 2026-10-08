//! Experimental stored unsigned bundle creation for independent unbundle checks.
#[cfg(feature = "write")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use base64::Engine;
    use ms_package::authoring::{AppxBuilder, AppxBundleBuilder, AppxBundleEditor, WriteOptions};
    use std::{fs::File, io::Cursor};

    let path = std::env::args_os()
        .nth(1)
        .ok_or("usage: author_bundle OUTPUT.msixbundle [EDITED.msixbundle]")?;
    let edited_path = std::env::args_os().nth(2);
    let options = WriteOptions::default();
    let mut bundle = AppxBundleBuilder::new("Authoring.Test", "CN=Test", "1.0.0.0", options)?;
    for architecture in ["x86", "x64"] {
        let manifest = format!(
            r#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10" xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10" IgnorableNamespaces="uap"><Identity Name="Authoring.Test" Publisher="CN=Test" Version="1.0.0.0" ProcessorArchitecture="{architecture}"/><Properties><DisplayName>Authoring Test</DisplayName><PublisherDisplayName>Test</PublisherDisplayName><Logo>logo.png</Logo></Properties><Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.26100.0"/></Dependencies><Resources><Resource Language="en-us"/></Resources><Applications><Application Id="App" Executable="authoring.exe" EntryPoint="Authoring.Test.App"><uap:VisualElements DisplayName="Authoring Test" Description="Package format validation fixture" BackgroundColor="transparent" Square150x150Logo="logo150.png" Square44x44Logo="logo44.png"/></Application></Applications></Package>"#
        );
        let mut package = AppxBuilder::new(manifest.into_bytes(), options)?;
        package.add_file(
            "authoring.exe",
            &b"format validation fixture; not an executable"[..],
        )?;
        package.add_file("payload.txt", &b"bundle payload"[..])?;
        let image = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAADIAAAAyCAYAAAAeP4ixAAAAIElEQVR4nO3BAQEAAACCIP+vbkhAAQAAAAAAAAAAwKMBJ0IAAdXu39oAAAAASUVORK5CYII=")?;
        package.add_file("logo.png", &image[..])?;
        let image = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAJYAAACWCAYAAAA8AXHiAAAAbklEQVR4nO3BMQEAAADCoPVPbQsvoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAPgZYDUAAaMExRQAAAAASUVORK5CYII=")?;
        package.add_file("logo150.png", &image[..])?;
        let image = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAACwAAAAsCAYAAAAehFoBAAAAHklEQVR4nO3BMQEAAADCoPVPbQZ/oAAAAAAAAADgMx5sAAFelnniAAAAAElFTkSuQmCC")?;
        package.add_file("logo44.png", &image[..])?;
        let mut bytes = Cursor::new(Vec::new());
        package.write(&mut bytes)?;
        bundle.add_package(
            format!("authoring-{architecture}.msix"),
            Cursor::new(bytes.into_inner()),
        )?;
    }
    let resource_manifest = br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Authoring.Test" Publisher="CN=Test" Version="1.0.0.0" ResourceId="language-fr"/><Properties><DisplayName>Authoring Test resources</DisplayName><PublisherDisplayName>Test</PublisherDisplayName><Logo>logo.png</Logo><ResourcePackage>true</ResourcePackage></Properties><Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.26100.0"/></Dependencies><Resources><Resource Language="fr-fr"/></Resources></Package>"#;
    let mut resource = AppxBuilder::new(resource_manifest.as_slice(), options)?;
    resource.add_file("localized.txt", &b"French resource fixture"[..])?;
    let logo = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAADIAAAAyCAYAAAAeP4ixAAAAIElEQVR4nO3BAQEAAACCIP+vbkhAAQAAAAAAAAAAwKMBJ0IAAdXu39oAAAAASUVORK5CYII=")?;
    resource.add_file("logo.png", &logo[..])?;
    let mut bytes = Cursor::new(Vec::new());
    resource.write(&mut bytes)?;
    bundle.add_package("authoring-fr.msix", Cursor::new(bytes.into_inner()))?;
    let report = bundle.write(File::create(&path)?)?;
    println!("{report:?}");
    if let Some(edited_path) = edited_path {
        let mut editor = AppxBundleEditor::open(File::open(path)?, options)?;
        editor.edit_package("authoring-fr.msix", |resource| {
            resource.replace_file("localized.txt", &b"Edited French resource fixture"[..])
        })?;
        println!("{:?}", editor.write(File::create(edited_path)?)?);
    }
    Ok(())
}

#[cfg(not(feature = "write"))]
fn main() {
    eprintln!("Run with --features write");
}
