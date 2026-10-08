//! XML backends use alloc-compatible no_std libraries; package I/O remains std.
use super::WriteError;
use std::{cell::Cell, rc::Rc};

pub(super) fn parse(data: &[u8], max_bytes: u64) -> Result<roxmltree::Document<'_>, WriteError> {
    if data.len() as u64 > max_bytes {
        return Err(WriteError::LimitExceeded("XML metadata bytes"));
    }
    let text = std::str::from_utf8(data).map_err(|e| WriteError::InvalidInput(e.to_string()))?;
    // Even an empty DTD is outside the supported metadata profile.
    if text.contains("<!DOCTYPE") {
        return Err(WriteError::Unsupported("XML document types".into()));
    }
    roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: u32::try_from(max_bytes).unwrap_or(u32::MAX),
            ..Default::default()
        },
    )
    .map_err(|e| WriteError::InvalidInput(e.to_string()))
}

struct BoundedXml {
    bytes: Vec<u8>,
    limit: u64,
    length: Rc<Cell<u64>>,
    dynamic_limit: Rc<Cell<u64>>,
}

impl woxml::Write for BoundedXml {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, woxml::Error> {
        if (self.bytes.len() as u64)
            .checked_add(bytes.len() as u64)
            .is_none_or(|size| size > self.limit.min(self.dynamic_limit.get()))
        {
            return Err(woxml::Error::WriteAllEof);
        }
        self.bytes.extend_from_slice(bytes);
        self.length.set(self.bytes.len() as u64);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> Result<(), woxml::Error> {
        Ok(())
    }
}

fn writer_error(error: woxml::Error) -> WriteError {
    if matches!(error, woxml::Error::WriteAllEof) {
        WriteError::LimitExceeded("generated XML metadata")
    } else {
        WriteError::InvalidInput(error.to_string())
    }
}

pub(super) struct MetadataWriter {
    writer: woxml::XmlWriter<'static, BoundedXml>,
    length: Rc<Cell<u64>>,
    dynamic_limit: Rc<Cell<u64>>,
}

impl MetadataWriter {
    pub(super) fn len(&self) -> u64 {
        self.length.get()
    }

    pub(super) fn set_limit(&mut self, limit: u64) {
        self.dynamic_limit.set(limit);
    }

    pub(super) fn new(limit: u64) -> Self {
        let length = Rc::new(Cell::new(0));
        let dynamic_limit = Rc::new(Cell::new(limit));
        Self {
            writer: woxml::XmlWriter::compact_mode(BoundedXml {
                bytes: Vec::new(),
                limit,
                length: Rc::clone(&length),
                dynamic_limit: Rc::clone(&dynamic_limit),
            }),
            length,
            dynamic_limit,
        }
    }

    pub(super) fn start(&mut self, name: &'static str) -> Result<(), WriteError> {
        self.writer.begin_elem(name).map_err(writer_error)
    }

    pub(super) fn attribute(&mut self, name: &str, value: &str) -> Result<(), WriteError> {
        if value
            .chars()
            .any(|c| c.is_control() || c == '\u{FFFE}' || c == '\u{FFFF}')
        {
            return Err(WriteError::InvalidInput(
                "invalid XML attribute characters".into(),
            ));
        }
        self.writer.attr_esc(name, value).map_err(writer_error)
    }

    pub(super) fn end(&mut self) -> Result<(), WriteError> {
        self.writer.end_elem().map_err(writer_error)
    }

    pub(super) fn finish(mut self) -> Result<Vec<u8>, WriteError> {
        self.writer.close().map_err(writer_error)?;
        self.writer.flush().map_err(writer_error)?;
        Ok(self.writer.into_inner().bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_attributes_round_trip_and_metadata_is_bounded() {
        let mut xml = MetadataWriter::new(1024);
        xml.start("Root").unwrap();
        xml.attribute("Value", "<&\"'é>").unwrap();
        xml.end().unwrap();
        let bytes = xml.finish().unwrap();
        let document = parse(&bytes, 1024).unwrap();
        assert_eq!(document.root_element().attribute("Value"), Some("<&\"'é>"));
        let mut limited = MetadataWriter::new(4);
        assert!(limited.start("Root").is_err());
    }

    #[test]
    fn document_types_and_namespace_errors_fail_closed() {
        assert!(parse(b"<!DOCTYPE Root><Root/>", 1024).is_err());
        assert!(parse(b"<unknown:Root/>", 1024).is_err());
    }
}
