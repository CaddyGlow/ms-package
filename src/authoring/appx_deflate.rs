//! APPX block-aware raw DEFLATE using the portable ms-compress encoder.
use std::io::{Cursor, Write};

use super::WriteError;
use base64::Engine;
use ms_compress::zlib::{Deflate, DeflateFlush, Status};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek};

// Account for the fixed output buffer and the encoder's 32-KiB window, hash,
// predecessor and pending-output allocations at its fixed default mem_level.
pub(super) const WORKSPACE: u64 = 512 << 10;

pub(super) struct Encoded {
    pub bytes: Vec<u8>,
    pub blocks: Vec<u64>,
}

pub(super) fn encode(data: &[u8], maximum: u64) -> Result<Encoded, WriteError> {
    let mut encoder = Deflate::new(6, false, 15);
    let mut encoded = Encoded {
        bytes: Vec::new(),
        blocks: Vec::new(),
    };
    for block in data.chunks(65536) {
        let start = encoded.bytes.len();
        run(
            &mut encoder,
            block,
            DeflateFlush::FullFlush,
            &mut encoded.bytes,
            maximum,
        )?;
        encoded.blocks.push((encoded.bytes.len() - start) as u64);
    }
    // The final empty BFINAL stream marker belongs to the ZIP compressed stream,
    // but not to the compressed Size of the last decoded block. This matches
    // Microsoft's msix-packaging DeflateStream::Write(0) finalization.
    run(
        &mut encoder,
        &[],
        DeflateFlush::Finish,
        &mut encoded.bytes,
        maximum,
    )?;
    Ok(encoded)
}

fn run(
    encoder: &mut Deflate,
    input: &[u8],
    flush: DeflateFlush,
    output: &mut Vec<u8>,
    maximum: u64,
) -> Result<(), WriteError> {
    let mut consumed = 0usize;
    let mut buffer = [0; 65536];
    loop {
        let before_in = encoder.total_in();
        let before_out = encoder.total_out();
        let status = encoder
            .compress(&input[consumed..], &mut buffer, flush)
            .map_err(|e| WriteError::InvalidInput(format!("APPX deflate: {e:?}")))?;
        let read = (encoder.total_in() - before_in) as usize;
        let written = (encoder.total_out() - before_out) as usize;
        consumed = consumed
            .checked_add(read)
            .ok_or(WriteError::LimitExceeded("codec input bytes"))?;
        if (output.len() as u64)
            .checked_add(written as u64)
            .is_none_or(|n| n > maximum)
        {
            return Err(WriteError::LimitExceeded("compressed bytes"));
        }
        output.extend_from_slice(&buffer[..written]);
        if status == Status::StreamEnd {
            return Ok(());
        }
        if flush != DeflateFlush::Finish && consumed == input.len() && written < buffer.len() {
            return Ok(());
        }
        if read == 0 && written == 0 {
            return Err(WriteError::InvalidInput(
                "APPX encoder made no progress".into(),
            ));
        }
    }
}

/// ZIP's public raw-copy API accepts a ZipFile rather than raw encoded bytes.
/// Generate a bounded one-entry ZIP32 carrier with ZIP itself, then replace only
/// the compression metadata and decoded CRC/size. The final archive's headers
/// and central directory are still emitted and finalized by the ZIP backend.
pub(super) fn copy_into<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    name: &str,
    decoded: &[u8],
    encoded: &[u8],
) -> Result<(), WriteError> {
    let mut carrier = zip::ZipWriter::new(Cursor::new(Vec::new()));
    carrier.start_file(
        name,
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .last_modified_time(zip::DateTime::default()),
    )?;
    carrier.write_all(encoded)?;
    let mut bytes = carrier.finish()?.into_inner();
    let end = bytes
        .len()
        .checked_sub(22)
        .ok_or_else(|| WriteError::InvalidInput("ZIP carrier footer".into()))?;
    let central = u32::from_le_bytes(
        bytes[end + 16..end + 20]
            .try_into()
            .map_err(|_| WriteError::InvalidInput("ZIP carrier offset".into()))?,
    ) as usize;
    let crc = ms_compress::zlib::crc32::crc32(0, decoded);
    let size =
        u32::try_from(decoded.len()).map_err(|_| WriteError::LimitExceeded("ZIP32 file bytes"))?;
    for (offset, value) in [
        (14, crc),
        (22, size),
        (central + 16, crc),
        (central + 24, size),
    ] {
        let field = bytes
            .get_mut(offset..offset + 4)
            .ok_or_else(|| WriteError::InvalidInput("ZIP carrier header".into()))?;
        field.copy_from_slice(&value.to_le_bytes());
    }
    for offset in [8, central + 10] {
        bytes
            .get_mut(offset..offset + 2)
            .ok_or_else(|| WriteError::InvalidInput("ZIP carrier method".into()))?
            .copy_from_slice(&8u16.to_le_bytes());
    }
    let mut source = zip::ZipArchive::new(Cursor::new(bytes))?;
    zip.raw_copy_file(source.by_index_raw(0)?)?;
    Ok(())
}

/// Check the physical APPX block boundaries in compressed inputs. The public
/// reader validates decoded hashes, but does not retain Block/@Size values.
pub(super) fn preflight<R: Read + Seek>(
    source: &mut R,
    options: &super::WriteOptions,
) -> Result<(), WriteError> {
    let map = {
        let mut archive =
            archive_core::Archive::open(&mut *source, super::reader_limits(&options.limits))
                .map_err(crate::Error::from)?;
        let ids: Vec<_> = archive.entries().iter().map(|entry| entry.id).collect();
        let mut compressed = false;
        for id in ids {
            compressed |= matches!(
                archive
                    .entry_metadata(id)
                    .map_err(crate::Error::from)?
                    .format,
                Some(archive_core::EntryFormatMetadata::Zip {
                    compression_method: 8,
                    ..
                })
            );
        }
        if !compressed {
            return Ok(());
        }
        let metadata_budget = options
            .limits
            .max_scratch_bytes
            .checked_sub(WORKSPACE)
            .ok_or(WriteError::LimitExceeded("codec scratch bytes"))?;
        let entry = archive
            .entries()
            .iter()
            .find(|entry| entry.name == "AppxBlockMap.xml")
            .ok_or_else(|| WriteError::InvalidInput("missing AppxBlockMap.xml".into()))?
            .id;
        archive
            .read_entry(
                entry,
                options.limits.max_metadata_bytes.min(metadata_budget),
            )
            .map_err(crate::Error::from)?
    };
    let document = super::parse(&map, options.limits.max_metadata_bytes)?;
    let mut zip = zip::ZipArchive::new(source)?;
    for index in 0..zip.len() {
        let mut file = zip.by_index_raw(index)?;
        if file.compression() != zip::CompressionMethod::DEFLATE {
            continue;
        }
        // The block map and content types have no block records and are already
        // decoded under archive metadata limits by the package opener.
        if matches!(
            file.name(),
            "AppxBlockMap.xml" | "[Content_Types].xml" | "AppxSignature.p7x"
        ) {
            continue;
        }
        let record = document
            .root_element()
            .children()
            .find(|node| {
                node.is_element()
                    && node
                        .attribute("Name")
                        .is_some_and(|name| name.replace('\\', "/") == file.name())
            })
            .ok_or_else(|| {
                WriteError::InvalidInput(format!("missing compressed block record {}", file.name()))
            })?;
        let header = record
            .attribute("LfhSize")
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| {
                WriteError::InvalidInput("missing compressed local header size".into())
            })?;
        if header != file.data_start() - file.header_start() {
            return Err(WriteError::InvalidInput(
                "compressed local header size mismatch".into(),
            ));
        }
        let expected_size = record
            .attribute("Size")
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| WriteError::InvalidInput("compressed decoded size".into()))?;
        let maximum = if file.name() == "AppxManifest.xml" {
            options.limits.max_metadata_bytes
        } else {
            options.limits.max_file_bytes
        };
        if expected_size != file.size() || expected_size > maximum {
            return Err(WriteError::LimitExceeded("compressed decoded bytes"));
        }
        let mut decoded_total = 0u64;
        let mut compressed_total = 0u64;
        let mut file_hash = Sha256::new();
        let mut expected_file_hash = None;
        for block in record.children().filter(|node| node.is_element()) {
            if block.has_tag_name((
                "http://schemas.microsoft.com/appx/2021/blockmap",
                "FileHash",
            )) {
                if expected_file_hash.is_some() {
                    return Err(WriteError::InvalidInput("duplicate APPX file hash".into()));
                }
                expected_file_hash =
                    Some(block.attribute("Hash").ok_or_else(|| {
                        WriteError::InvalidInput("missing APPX file hash".into())
                    })?);
                continue;
            }
            if !block.has_tag_name(("http://schemas.microsoft.com/appx/2010/blockmap", "Block")) {
                return Err(WriteError::Unsupported("APPX block map extensions".into()));
            }
            let size = block
                .attribute("Size")
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| WriteError::InvalidInput("missing compressed block Size".into()))?;
            compressed_total = compressed_total
                .checked_add(size)
                .ok_or(WriteError::LimitExceeded("compressed block bytes"))?;
            let budget = options
                .limits
                .max_scratch_bytes
                .checked_sub(map.len() as u64)
                .and_then(|n| n.checked_sub(WORKSPACE))
                .ok_or(WriteError::LimitExceeded("codec scratch bytes"))?;
            if size == 0 || size > budget || compressed_total > file.compressed_size() {
                return Err(WriteError::LimitExceeded("compressed block bytes"));
            }
            let mut bytes = vec![
                0;
                usize::try_from(size).map_err(|_| WriteError::LimitExceeded(
                    "compressed block bytes"
                ))?
            ];
            file.read_exact(&mut bytes)?;
            let expected = expected_size
                .checked_sub(decoded_total)
                .ok_or_else(|| WriteError::InvalidInput("compressed block coverage".into()))?
                .min(65536);
            if expected == 0 {
                return Err(WriteError::InvalidInput("extra compressed block".into()));
            }
            let mut inflate = ms_compress::zlib::Inflate::new(false, 15);
            let mut consumed = 0usize;
            let mut produced = 0u64;
            let mut hash = Sha256::new();
            let mut output = [0; 8192];
            loop {
                let before_in = inflate.total_in();
                let before_out = inflate.total_out();
                let status = inflate
                    .decompress(
                        &bytes[consumed..],
                        &mut output,
                        ms_compress::zlib::InflateFlush::NoFlush,
                    )
                    .map_err(|e| {
                        WriteError::InvalidInput(format!("compressed APPX block: {e:?}"))
                    })?;
                let read = (inflate.total_in() - before_in) as usize;
                let written = (inflate.total_out() - before_out) as usize;
                consumed = consumed
                    .checked_add(read)
                    .ok_or(WriteError::LimitExceeded("codec input bytes"))?;
                produced = produced
                    .checked_add(written as u64)
                    .ok_or(WriteError::LimitExceeded("codec decoded bytes"))?;
                if produced > expected || status == Status::StreamEnd {
                    return Err(WriteError::InvalidInput(
                        "compressed APPX block boundary".into(),
                    ));
                }
                hash.update(&output[..written]);
                file_hash.update(&output[..written]);
                if consumed == bytes.len() && written < output.len() {
                    break;
                }
                if read == 0 && written == 0 {
                    return Err(WriteError::InvalidInput(
                        "compressed APPX block made no progress".into(),
                    ));
                }
            }
            if produced != expected
                || block.attribute("Hash")
                    != Some(
                        base64::engine::general_purpose::STANDARD
                            .encode(hash.finalize())
                            .as_str(),
                    )
            {
                return Err(WriteError::InvalidInput(
                    "compressed APPX block hash or size".into(),
                ));
            }
            decoded_total = decoded_total
                .checked_add(produced)
                .ok_or(WriteError::LimitExceeded("codec decoded bytes"))?;
        }
        if decoded_total != expected_size
            || compressed_total.checked_add(2) != Some(file.compressed_size())
        {
            return Err(WriteError::InvalidInput(
                "compressed APPX block coverage".into(),
            ));
        }
        if expected_file_hash.is_some_and(|expected| {
            base64::engine::general_purpose::STANDARD.encode(file_hash.finalize()) != expected
        }) {
            return Err(WriteError::InvalidInput("compressed APPX file hash".into()));
        }
        let mut tail = [0; 2];
        file.read_exact(&mut tail)?;
        if tail != [3, 0] {
            return Err(WriteError::Unsupported("APPX deflate final marker".into()));
        }
    }
    Ok(())
}
