use ms_package::InstallerPackage;
use std::io::Cursor;

#[test]
fn null_required_column_type_is_rejected_without_panicking() {
    let bytes = include_bytes!("fixtures/msi-null-column-type.msi");
    let result = InstallerPackage::open_bounded(Cursor::new(bytes.as_slice()), 128, 1 << 20);
    assert!(result.is_err());
}
