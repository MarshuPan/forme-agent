//! Bounded PNG decoding shared by Host and core, independent of a desktop driver.
use forme_protocol as p;
#[cfg(test)]
#[path = "desktop_png_tests.rs"]
mod tests;
use std::io::Cursor;

const DECODE_MEMORY_BYTES: usize = 128 * 1024 * 1024;

/// Verify the complete encoded object; matching IHDR alone is insufficient.
pub fn validate_desktop_png(bytes: &[u8], width: u32, height: u32) -> p::Result<()> {
    if bytes.len() as u64 > p::DESKTOP_OUTPUT_MAX_BYTES
        || width == 0
        || height == 0
        || u64::from(width) * u64::from(height) > p::DESKTOP_OUTPUT_MAX_PIXELS
    {
        return Err(invalid());
    }
    check_chunks(bytes)?;
    // Divide the work budget between decoder internals and decoded output.
    // Native 8-bit RGBA at the full pixel limit still fits the output half.
    let max_output = DECODE_MEMORY_BYTES / 2;
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: DECODE_MEMORY_BYTES - max_output,
        },
    );
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    decoder.ignore_checksums(false);
    let mut reader = decoder.read_info().map_err(|_| invalid())?;
    if reader.info().width != width
        || reader.info().height != height
        || reader.info().animation_control.is_some()
    {
        return Err(invalid());
    }
    let size = reader.output_buffer_size().ok_or_else(invalid)?;
    if size > max_output {
        return Err(invalid());
    }
    let mut output = vec![0; size];
    let info = reader.next_frame(&mut output).map_err(|_| invalid())?;
    if info.width != width || info.height != height || info.buffer_size() != size {
        return Err(invalid());
    }
    reader.finish().map_err(|_| invalid())
}

fn check_chunks(bytes: &[u8]) -> p::Result<()> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(invalid());
    }
    let mut offset = 8;
    let mut chunks = 0;
    let mut image_data = false;
    let mut data_ended = false;
    let mut palette = false;
    while offset < bytes.len() {
        let prefix = bytes.get(offset..offset + 8).ok_or_else(invalid)?;
        let size = u32::from_be_bytes(prefix[..4].try_into().map_err(|_| invalid())?) as usize;
        let kind = &prefix[4..];
        let end = offset
            .checked_add(12)
            .and_then(|value| value.checked_add(size))
            .ok_or_else(invalid)?;
        if end > bytes.len()
            || chunks >= 65536
            || !kind.iter().all(u8::is_ascii_alphabetic)
            || !kind[2].is_ascii_uppercase()
            || (chunks == 0 && (kind != b"IHDR" || size != 13))
            || (chunks > 0 && kind == b"IHDR")
            || matches!(kind, b"acTL" | b"fcTL" | b"fdAT")
            || (kind[0].is_ascii_uppercase()
                && !matches!(kind, b"IHDR" | b"PLTE" | b"IDAT" | b"IEND"))
        {
            return Err(invalid());
        }
        let expected = u32::from_be_bytes(bytes[end - 4..end].try_into().map_err(|_| invalid())?);
        if crc32fast::hash(&bytes[offset + 4..end - 4]) != expected {
            return Err(invalid());
        }
        if kind == b"PLTE" {
            if palette || image_data || size == 0 || size > 768 || !size.is_multiple_of(3) {
                return Err(invalid());
            }
            palette = true;
        }
        if kind == b"IDAT" {
            if data_ended {
                return Err(invalid());
            }
            image_data = true;
        } else if image_data {
            data_ended = true;
        }
        if kind == b"IEND" {
            return if size == 0 && image_data && end == bytes.len() {
                Ok(())
            } else {
                Err(invalid())
            };
        }
        offset = end;
        chunks += 1;
    }
    Err(invalid())
}
fn invalid() -> p::Error {
    p::Error("desktop PNG did not pass complete validation".into())
}
