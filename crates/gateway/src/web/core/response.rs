//! Strict, bounded HTTP/1 response framing for the private loopback channel.
use forme_protocol as p;

use super::{CoreBridgeResponse, MAX_RESPONSE_BYTES};

pub(super) const MAX_WIRE_BYTES: usize = MAX_RESPONSE_BYTES + 512 * 1024;
const MAX_HEADER_BYTES: usize = 16 * 1024;

fn invalid() -> p::Error {
    p::Error("core bridge response framing is invalid".into())
}

pub(super) fn parse_response(response: &[u8]) -> p::Result<CoreBridgeResponse> {
    if response.len() > MAX_WIRE_BYTES {
        return Err(invalid());
    }
    let separator = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(invalid)?;
    if separator > MAX_HEADER_BYTES {
        return Err(invalid());
    }
    let header = std::str::from_utf8(&response[..separator]).map_err(|_| invalid())?;
    let mut lines = header.split("\r\n");
    let status_line = lines.next().ok_or_else(invalid)?;
    let (version, rest) = status_line.split_once(' ').ok_or_else(invalid)?;
    let (code, reason) = rest.split_once(' ').ok_or_else(invalid)?;
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1")
        || code.len() != 3
        || !code.bytes().all(|b| b.is_ascii_digit())
        || reason.bytes().any(|b| b.is_ascii_control())
    {
        return Err(invalid());
    }
    let status: u16 = code.parse().map_err(|_| invalid())?;
    if !(200..=599).contains(&status) {
        return Err(invalid());
    }
    let mut length = None;
    let mut transfer = None;
    let mut content_type = None;
    let mut content_disposition = None;
    for line in lines {
        let (name, raw) = line.split_once(':').ok_or_else(invalid)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            || raw.bytes().any(|b| b.is_ascii_control() && b != b'\t')
        {
            return Err(invalid());
        }
        let value = raw.trim_matches([' ', '\t']);
        match name.to_ascii_lowercase().as_str() {
            "content-length" => {
                if length.is_some()
                    || value.is_empty()
                    || !value.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(invalid());
                }
                length = Some(value.parse::<usize>().map_err(|_| invalid())?);
            }
            "transfer-encoding" => {
                if transfer.is_some() || !value.eq_ignore_ascii_case("chunked") {
                    return Err(invalid());
                }
                transfer = Some(());
            }
            "content-type" => {
                if content_type.is_some() || value.is_empty() {
                    return Err(invalid());
                }
                content_type = Some(value.to_owned());
            }
            "content-disposition" => {
                if content_disposition.is_some() || value.is_empty() {
                    return Err(invalid());
                }
                content_disposition = Some(value.to_owned());
            }
            "content-encoding" if !value.eq_ignore_ascii_case("identity") => return Err(invalid()),
            _ => {}
        }
    }
    if length.is_some() && transfer.is_some() {
        return Err(invalid());
    }
    let raw = &response[separator + 4..];
    let body = if status == 204 || status == 304 {
        if !raw.is_empty()
            || transfer.is_some()
            || (status == 204 && length.is_some_and(|n| n != 0))
        {
            return Err(invalid());
        }
        Vec::new()
    } else if transfer.is_some() {
        decode_chunked(raw)?
    } else {
        if length.is_some_and(|n| n != raw.len()) || raw.len() > MAX_RESPONSE_BYTES {
            return Err(invalid());
        }
        raw.to_vec()
    };
    Ok(CoreBridgeResponse {
        status,
        content_type,
        content_disposition,
        body,
    })
}

fn decode_chunked(mut raw: &[u8]) -> p::Result<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let end = raw
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(invalid)?;
        // The project-owned server emits no extensions or trailers. Reject
        // ambiguous framing rather than attempting a permissive interpretation.
        if end == 0 || end > 16 || !raw[..end].iter().all(u8::is_ascii_hexdigit) {
            return Err(invalid());
        }
        let size =
            usize::from_str_radix(std::str::from_utf8(&raw[..end]).map_err(|_| invalid())?, 16)
                .map_err(|_| invalid())?;
        raw = &raw[end + 2..];
        if size == 0 {
            return if raw == b"\r\n" {
                Ok(body)
            } else {
                Err(invalid())
            };
        }
        if size > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
            return Err(invalid());
        }
        let chunk = raw.get(..size).ok_or_else(invalid)?;
        if raw.get(size..size + 2) != Some(b"\r\n") {
            return Err(invalid());
        }
        body.extend_from_slice(chunk);
        raw = &raw[size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_exact_fixed_chunked_and_no_content_responses() {
        assert_eq!(
            parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
                .unwrap()
                .body,
            b"{}"
        );
        assert_eq!(
            parse_response(
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Disposition: attachment\r\n\r\n{}"
            )
            .unwrap()
            .content_disposition,
            Some("attachment".into())
        );
        assert_eq!(
            parse_response(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n"
            )
            .unwrap()
            .body,
            b"{}"
        );
        assert!(parse_response(b"HTTP/1.1 204 No Content\r\n\r\n")
            .unwrap()
            .body
            .is_empty());
    }

    #[test]
    fn rejects_ambiguous_truncated_trailing_or_malformed_responses() {
        for raw in [
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nContent-Length: +2\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nBad Header: abc\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\n folded: abc\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nNo colon\r\n\r\n{}",
            "NOTHTTP 200 OK\r\n\r\n{}",
            "HTTP/1.1 101 Switching Protocols\r\n\r\n",
            "HTTP/1.1 204 No Content\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nContent-Disposition: attachment\r\nContent-Disposition: attachment\r\n\r\n{}",
        ] {
            assert!(parse_response(raw.as_bytes()).is_err(), "accepted malformed response");
        }
        for chunks in [
            "2\r\n{}XX0\r\n\r\n",
            "2\r\n{\r\n",
            "0\r\n",
            "0\r\n\r\nextra",
            "FFFFFFFFFFFFFFFF\r\n",
            "0\r\nX: y\r\n\r\n",
        ] {
            assert!(parse_response(
                format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{chunks}").as_bytes()
            )
            .is_err());
        }
    }
}
