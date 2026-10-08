//! Experimental file-only MSI for independent installation qualification.

#[cfg(feature = "write")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use ms_package::authoring::{
        InstallationContext, InstallerArchitecture, InstallerBuilder, InstallerIdentity,
        WriteOptions,
    };
    use std::io::Cursor;

    let path = std::env::args_os()
        .nth(1)
        .ok_or("supply a distinct output .msi path")?;
    let identity = InstallerIdentity {
        product_code: "{A09C9465-494B-4C24-BEFC-BCF975E23526}".into(),
        package_code: "{CE6F227A-D92D-45F9-BCAA-F432D2A2BFC6}".into(),
        upgrade_code: "{29079377-0EF5-4891-B138-B27804FAB594}".into(),
        name: "ms-package authoring qualification".into(),
        manufacturer: "ms-package".into(),
        version: "1.0.0".into(),
        directory_name: "ms-package-authoring-test".into(),
        architecture: InstallerArchitecture::X64,
        context: InstallationContext::PerUser,
    };
    let mut builder = InstallerBuilder::new(identity, WriteOptions::default())?;
    builder.add_file(
        "Payload",
        "payload.txt",
        "{BC2C8871-6A34-44B4-A757-19A564CEAD66}",
        Cursor::new(b"ms-package file-only qualification payload\r\n"),
    )?;
    builder.write(std::fs::File::create(path)?)?;
    Ok(())
}

#[cfg(not(feature = "write"))]
fn main() {
    eprintln!("enable the write feature to run this example");
}
