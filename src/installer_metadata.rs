//! Validate MSI metadata sizes before the upstream parser allocates string buffers.
use crate::{Error, Result};
use std::io::{Read, Seek};

// MSI's packed table stream names for _StringPool and _StringData.
const STRING_POOL: &str = "\u{4840}\u{3f3f}\u{4577}\u{446c}\u{3e6a}\u{44b2}\u{482f}";
const STRING_DATA: &str = "\u{4840}\u{3f3f}\u{4577}\u{446c}\u{3b6a}\u{45e4}\u{4824}";

pub(crate) fn preflight<R: Read + Seek>(reader: &mut R, max_metadata: u64) -> Result<()> {
    let mut storage = cfb::CompoundFile::open(reader)?;
    let mut metadata_bytes = 0_u64;
    let mut data_bytes = None;
    for entry in storage.walk() {
        if !entry.is_stream() {
            continue;
        }
        if entry.name().starts_with('\u{4840}') || entry.name().starts_with('\u{5}') {
            metadata_bytes = metadata_bytes
                .checked_add(entry.len())
                .ok_or(Error::Limit("MSI metadata bytes"))?;
            if metadata_bytes > max_metadata {
                return Err(Error::Limit("MSI metadata bytes"));
            }
        }
        if entry.name() == STRING_DATA {
            data_bytes = Some(entry.len());
        }
    }
    let Some(data_bytes) = data_bytes else {
        // The MSI parser will report missing mandatory streams.
        return Ok(());
    };
    if !storage.exists(STRING_POOL) {
        return Ok(());
    }
    let mut pool = storage.open_stream(STRING_POOL)?;
    let mut header = [0; 4];
    pool.read_exact(&mut header)?;
    let mut declared_bytes = 0_u64;
    loop {
        let mut record = [0; 4];
        // A clean EOF is valid; a partial record is malformed.
        if pool.read(&mut record[..1])? == 0 {
            break;
        }
        pool.read_exact(&mut record[1..])?;
        let mut length = u32::from(u16::from_le_bytes([record[0], record[1]]));
        let references = u16::from_le_bytes([record[2], record[3]]);
        if length == 0 && references > 0 {
            let mut extended = [0; 4];
            pool.read_exact(&mut extended)?;
            length = u32::from_le_bytes(extended);
        }
        declared_bytes = declared_bytes
            .checked_add(u64::from(length))
            .ok_or(Error::Limit("MSI string data bytes"))?;
        if declared_bytes > data_bytes {
            return Err(Error::Malformed(
                "MSI string lengths exceed the string data stream".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn storage(pool: &[u8], data: &[u8]) -> Cursor<Vec<u8>> {
        let mut compound = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        compound
            .create_stream(STRING_POOL)
            .unwrap()
            .write_all(pool)
            .unwrap();
        compound
            .create_stream(STRING_DATA)
            .unwrap()
            .write_all(data)
            .unwrap();
        compound.into_inner()
    }

    #[test]
    fn rejects_extended_string_length_before_allocating_it() {
        let pool = [0xe4, 0x04, 0, 0, 0, 0, 1, 0, 0xff, 0xff, 0xff, 0xff];
        assert!(matches!(
            preflight(&mut storage(&pool, b""), 1024),
            Err(Error::Malformed(_))
        ));
    }

    #[test]
    fn cumulative_lengths_must_fit_the_data_stream() {
        let pool = [0xe4, 0x04, 0, 0, 2, 0, 1, 0, 2, 0, 1, 0];
        assert!(preflight(&mut storage(&pool, b"abcd"), 1024).is_ok());
        assert!(matches!(
            preflight(&mut storage(&pool, b"abc"), 1024),
            Err(Error::Malformed(_))
        ));
    }

    #[test]
    fn metadata_budget_excludes_payload_but_includes_string_data() {
        let mut compound = cfb::CompoundFile::open(storage(&[0; 4], b"")).unwrap();
        compound
            .create_stream("cabinet")
            .unwrap()
            .write_all(&[0; 4096])
            .unwrap();
        let mut bytes = compound.into_inner();
        assert!(preflight(&mut bytes, 4).is_ok());
        assert!(matches!(preflight(&mut bytes, 3), Err(Error::Limit(_))));
    }

    #[test]
    fn rejects_truncated_pool_record() {
        assert!(preflight(&mut storage(&[0, 0, 0, 0, 1], b""), 1024).is_err());
    }
    #[test]
    fn package_opener_rejects_invalid_pool_before_the_msi_parser() {
        let package =
            msi::Package::create(msi::PackageType::Installer, Cursor::new(Vec::new())).unwrap();
        let mut compound = cfb::CompoundFile::open(package.into_inner().unwrap()).unwrap();
        compound
            .create_stream(STRING_POOL)
            .unwrap()
            .write_all(&[0xe4, 0x04, 0, 0, 0, 0, 1, 0, 0, 0x40, 0, 0])
            .unwrap();
        compound.create_stream(STRING_DATA).unwrap();
        assert!(matches!(
            crate::InstallerPackage::open_with_metadata_limit(
                compound.into_inner(),
                100,
                1 << 20,
                1 << 16,
            ),
            Err(Error::Malformed(_))
        ));
    }
}
