fn main() -> Result<(), Box<dyn std::error::Error>> {
    use ms_package_authoring_worker_check::*;
    let path = std::path::PathBuf::from(std::env::args().nth(1).ok_or("output path required")?);
    let directory = path.parent().ok_or("output parent required")?;
    let payload = b"worker authoring payload";
    let replacement = b"edited worker payload";
    let package = create_native(payload, 1 << 20)?;
    std::fs::write(&path, &package)?;
    std::fs::write(
        directory.join("native-edited.msix"),
        edit_native(&package, replacement)?,
    )?;
    let bundle = create_bundle_native(payload, 1 << 20)?;
    std::fs::write(directory.join("native.msixbundle"), &bundle)?;
    std::fs::write(
        directory.join("native-edited.msixbundle"),
        edit_bundle_native(&bundle, replacement)?,
    )?;
    let database = create_database_native(payload, 1 << 20)?;
    std::fs::write(directory.join("native.msi"), &database)?;
    std::fs::write(
        directory.join("native-edited.msi"),
        edit_database_native(&database, replacement)?,
    )?;
    Ok(())
}
