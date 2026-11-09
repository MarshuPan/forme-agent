use super::*;
use std::io;

struct Writer {
    bytes: Vec<u8>,
    calls: usize,
    block: bool,
    fail: bool,
}
impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.calls += 1;
        if self.fail {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        if self.block || self.calls.is_multiple_of(3) {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let count = bytes.len().min(3);
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!("bounded pipe writer must not perform a blocking flush")
    }
}
#[test]
fn bounded_output_writes_keep_exact_bytes_across_partial_and_busy_writes() {
    let mut writer = Writer {
        bytes: vec![],
        calls: 0,
        block: false,
        fail: false,
    };
    let bytes = b"original-header-and-pixel-stream";
    write_deadline(&mut writer, bytes, Instant::now() + Duration::from_secs(1)).unwrap();
    assert_eq!(writer.bytes, bytes);
}
#[test]
fn bounded_output_deadline_is_shared_and_never_extended_by_progress() {
    let mut writer = Writer {
        bytes: vec![],
        calls: 0,
        block: false,
        fail: false,
    };
    let deadline = Instant::now() + Duration::from_millis(20);
    write_deadline(&mut writer, b"head", deadline).unwrap();
    writer.block = true;
    let began = Instant::now();
    assert!(write_deadline(&mut writer, b"pixels", deadline).is_err());
    assert!(began.elapsed() < Duration::from_secs(1));
    let calls = writer.calls;
    assert!(write_deadline(&mut writer, b"finish", deadline).is_err());
    assert_eq!(writer.calls, calls);
    assert_eq!(writer.bytes, b"head");
}
#[test]
fn bounded_output_rejects_disconnection_without_retrying() {
    let mut writer = Writer {
        bytes: vec![],
        calls: 0,
        block: false,
        fail: true,
    };
    assert!(write_deadline(
        &mut writer,
        b"header",
        Instant::now() + Duration::from_secs(1)
    )
    .is_err());
    assert_eq!(writer.calls, 1);
    assert!(writer.bytes.is_empty());
}
