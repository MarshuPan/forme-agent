use forme_protocol::{sha256_content_digest, sha256_reader_digest};
use std::io::{self, Read};
struct ShortReads<'a> {
    bytes: &'a [u8],
    interrupted: bool,
}
impl Read for ShortReads<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::ErrorKind::Interrupted.into());
        }
        let n = self.bytes.len().min(out.len()).min(7);
        out[..n].copy_from_slice(&self.bytes[..n]);
        self.bytes = &self.bytes[n..];
        Ok(n)
    }
}
#[test]
fn stream_digest_matches_content_digest_across_buffer_and_short_read_boundaries() {
    for size in [0, 1, 65535, 65536, 65537, 150000] {
        let bytes = vec![0x9d; size];
        assert_eq!(
            sha256_reader_digest(ShortReads {
                bytes: &bytes,
                interrupted: false
            })
            .unwrap(),
            sha256_content_digest(&bytes)
        );
    }
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("private source"))
        }
    }
    let error = sha256_reader_digest(Broken).unwrap_err().to_string();
    assert!(!error.contains("private source"));
}
