#![cfg(feature = "write")]
use ms_package::authoring::{InstallerMediaSink, WriteError, WriteLimits, write_installer_media};
use std::{
    collections::BTreeMap,
    io::{self, Write},
};

#[derive(Clone, Copy, Default, PartialEq)]
enum Failure {
    #[default]
    None,
    Write,
    Flush,
    Finish,
}

struct MediaWriter {
    bytes: Vec<u8>,
    failure: Failure,
    flushed: bool,
}
impl Write for MediaWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = if self.failure == Failure::Write {
            let remaining = 2usize.saturating_sub(self.bytes.len());
            if remaining == 0 {
                return Err(io::Error::other("partial media write failure"));
            }
            remaining.min(bytes.len())
        } else {
            bytes.len()
        };
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.failure == Failure::Flush {
            return Err(io::Error::other("media flush failure"));
        }
        self.flushed = true;
        Ok(())
    }
}

#[derive(Default)]
struct MemorySink {
    files: BTreeMap<String, Vec<u8>>,
    opened: Vec<String>,
    failure: Failure,
}
impl InstallerMediaSink for MemorySink {
    type Writer = MediaWriter;
    fn create(&mut self, name: &str) -> io::Result<Self::Writer> {
        self.opened.push(name.into());
        Ok(MediaWriter {
            bytes: Vec::new(),
            failure: if name == "two.cab" {
                self.failure
            } else {
                Failure::None
            },
            flushed: false,
        })
    }
    fn finish(&mut self, name: &str, writer: Self::Writer) -> io::Result<()> {
        assert!(
            writer.flushed,
            "sink finalization occurred without successful writer flush"
        );
        if writer.failure == Failure::Finish {
            return Err(io::Error::other("media finalization failure"));
        }
        self.files.insert(name.into(), writer.bytes);
        Ok(())
    }
}

fn artifacts() -> Vec<(String, Vec<u8>)> {
    vec![
        ("one.cab".into(), b"one".to_vec()),
        ("two.cab".into(), b"second".to_vec()),
    ]
}

#[test]
fn caller_media_sink_reports_only_flushed_and_finalized_artifacts() {
    let mut sink = MemorySink::default();
    let report = write_installer_media(&artifacts(), &mut sink, &WriteLimits::default()).unwrap();
    assert_eq!(report.bytes_written, 9);
    assert_eq!(
        report
            .completed
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["one.cab", "two.cab"]
    );
    assert_eq!(sink.files["two.cab"], b"second");
}

#[test]
fn partial_write_flush_and_finalization_errors_retain_completion_context() {
    for failure in [Failure::Write, Failure::Flush, Failure::Finish] {
        let mut sink = MemorySink {
            failure,
            ..Default::default()
        };
        let error =
            write_installer_media(&artifacts(), &mut sink, &WriteLimits::default()).unwrap_err();
        let WriteError::Media {
            completed,
            incomplete,
            bytes_written,
            ..
        } = error
        else {
            panic!("missing partial artifact report");
        };
        assert_eq!(completed, ["one.cab"]);
        assert_eq!(incomplete, "two.cab");
        assert_eq!(bytes_written, if failure == Failure::Write { 5 } else { 9 });
        assert_eq!(sink.files.len(), 1);
    }
}

#[test]
fn media_path_conflicts_and_limits_fail_before_opening_any_destination() {
    for names in [
        ["../one.cab", "two.cab"],
        ["C:/one.cab", "two.cab"],
        ["NUL.cab", "two.cab"],
        ["one.cab", "ONE.cab"],
        ["Product", "Product/payload.txt"],
    ] {
        let artifacts: Vec<_> = names
            .into_iter()
            .map(|name| (name.into(), b"x".to_vec()))
            .collect();
        let mut sink = MemorySink::default();
        assert!(write_installer_media(&artifacts, &mut sink, &WriteLimits::default()).is_err());
        assert!(sink.opened.is_empty());
    }
    let limits = WriteLimits {
        max_output_bytes: 8,
        ..Default::default()
    };
    let mut sink = MemorySink::default();
    assert!(write_installer_media(&artifacts(), &mut sink, &limits).is_err());
    assert!(sink.opened.is_empty());
}

#[test]
fn loose_relative_names_are_explicit_and_exact_resource_boundary_succeeds() {
    let artifacts = vec![("Product/payload.txt".into(), b"hello".to_vec())];
    let limits = WriteLimits {
        max_entries: 1,
        max_file_bytes: 5,
        max_total_bytes: 5,
        max_output_bytes: 5,
        max_scratch_bytes: 5,
        ..Default::default()
    };
    let mut sink = MemorySink::default();
    assert_eq!(
        write_installer_media(&artifacts, &mut sink, &limits)
            .unwrap()
            .bytes_written,
        5
    );
    assert_eq!(sink.files["Product/payload.txt"], b"hello");
}
