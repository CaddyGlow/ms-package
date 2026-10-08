//! Retained outputs accepted by independent Microsoft packaging tools.
use std::io::Cursor;

#[test]
fn independently_unpacked_stored_package_retains_payload_integrity() {
    let bytes = include_bytes!("fixtures/authoring/stored.msix");
    let mut package =
        ms_package::AppxPackage::open(Cursor::new(bytes), Default::default(), 1 << 20).unwrap();
    package.validate(1 << 20).unwrap();
    let id = package
        .entries()
        .iter()
        .find(|entry| entry.name == "payload.txt")
        .unwrap()
        .id;
    assert_eq!(
        package.read_entry(id, 1 << 20).unwrap(),
        b"worker authoring payload"
    );
}

#[test]
fn independently_unbundled_architecture_packages_validate_separately() {
    let bytes = include_bytes!("fixtures/authoring/stored.msixbundle");
    let mut bundle =
        ms_package::AppxBundle::open(Cursor::new(bytes), Default::default(), 1 << 20).unwrap();
    bundle.validate(1 << 20).unwrap();
    let names: Vec<_> = bundle
        .packages()
        .iter()
        .map(|package| package.file_name.clone())
        .collect();
    assert_eq!(names.len(), 2);
    for name in names {
        bundle
            .select(&name, Default::default(), 1 << 20, 1 << 20)
            .unwrap()
            .validate(1 << 20)
            .unwrap();
    }
}
