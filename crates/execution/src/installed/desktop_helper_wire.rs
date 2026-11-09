//! Safe framing. V2 control and raw writes share one absolute deadline.
#![cfg(windows)]
use super::windows_desktop_helper_peer_native::read_bounded;
use forme_protocol as p;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::File,
    io::Write,
    time::{Duration, Instant},
};
const MAX_FRAME_BYTES: usize = 256 * 1024;
const DESKTOP_PIPE_PREFIX: &str = r"\\.\pipe\forme-desktop-helper-v1-";
pub(crate) fn helper_pipe_argument() -> p::Result<String> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.len() != 3
        || arguments[0] != "--desktop-session-helper"
        || arguments[1] != "--pipe"
    {
        return Err(p::Error("desktop helper arguments are invalid".into()));
    }
    let pipe = arguments[2].clone();
    if !pipe.starts_with(DESKTOP_PIPE_PREFIX)
        || pipe.len() > 256
        || pipe.chars().any(char::is_control)
    {
        return Err(p::Error("desktop helper pipe is invalid".into()));
    }
    Ok(pipe)
}

pub(crate) const fn valid_frame_length(length: usize) -> bool {
    length > 0 && length <= MAX_FRAME_BYTES
}

pub(crate) fn write_frame<T: Serialize>(stream: &mut File, value: &T) -> p::Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| p::Error("desktop helper frame cannot be encoded".into()))?;
    if !valid_frame_length(bytes.len()) {
        return Err(p::Error("desktop helper frame is outside its bound".into()));
    }
    let length = u32::try_from(bytes.len())
        .map_err(|_| p::Error("desktop helper frame is too large".into()))?;
    stream
        .write_all(&length.to_le_bytes())
        .map_err(|_| p::Error("desktop helper frame cannot be written".into()))?;
    stream
        .write_all(&bytes)
        .map_err(|_| p::Error("desktop helper frame cannot be written".into()))?;
    stream
        .flush()
        .map_err(|_| p::Error("desktop helper frame cannot be flushed".into()))
}

pub(crate) fn read_frame<T: DeserializeOwned>(
    stream: &mut File,
    deadline: Instant,
) -> p::Result<T> {
    let mut length = [0u8; 4];
    read_bounded(stream, &mut length, deadline)?;
    let length = u32::from_le_bytes(length) as usize;
    if !valid_frame_length(length) {
        return Err(p::Error("desktop helper frame length is invalid".into()));
    }
    let mut bytes = vec![0u8; length];
    read_bounded(stream, &mut bytes, deadline)?;
    serde_json::from_slice(&bytes).map_err(|_| p::Error("desktop helper frame is malformed".into()))
}

pub(crate) fn read_output_bytes(
    stream: &mut File,
    length: u32,
    deadline: Instant,
) -> p::Result<Vec<u8>> {
    if length == 0 || length > p::DESKTOP_OUTPUT_CHUNK_BYTES {
        return Err(invalid());
    }
    let mut bytes = vec![0; length as usize];
    read_bounded(stream, &mut bytes, deadline)?;
    Ok(bytes)
}
/// Both endpoints must be in PIPE_NOWAIT mode before invoking this writer.
pub(crate) fn write_output_frame<T: Serialize>(
    stream: &mut File,
    value: &T,
    deadline: Instant,
) -> p::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    if !valid_frame_length(bytes.len()) {
        return Err(invalid());
    }
    write_deadline(stream, &(bytes.len() as u32).to_le_bytes(), deadline)?;
    write_deadline(stream, &bytes, deadline)
}
pub(crate) fn write_output_bytes(
    stream: &mut File,
    bytes: &[u8],
    deadline: Instant,
) -> p::Result<()> {
    if bytes.is_empty() || bytes.len() > p::DESKTOP_OUTPUT_CHUNK_BYTES as usize {
        return Err(invalid());
    }
    write_deadline(stream, bytes, deadline)
}
fn write_deadline(stream: &mut impl Write, bytes: &[u8], deadline: Instant) -> p::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        if Instant::now() >= deadline {
            return Err(invalid());
        }
        match stream.write(&bytes[offset..]) {
            Ok(0) => {}
            Ok(count) => {
                offset += count;
                continue;
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) || e.raw_os_error() == Some(232) => {}
            Err(_) => return Err(invalid()),
        }
        std::thread::sleep(
            Duration::from_millis(5).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
    if Instant::now() >= deadline {
        return Err(invalid());
    }
    Ok(())
}
fn invalid() -> p::Error {
    p::Error("desktop output write exceeded its boundary or lost its peer".into())
}
#[cfg(test)]
#[path = "desktop_helper_wire_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "desktop_output_pipe_tests.rs"]
mod pipe_tests;
