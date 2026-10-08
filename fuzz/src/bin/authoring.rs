fn main() {
    loop {
        honggfuzz::fuzz!(|data: &[u8]| {
            ms_package_fuzz::authoring(data);
        });
    }
}
