#![forbid(unsafe_code)]

mod host;

fn main() {
    if let Err(error) = host::run() {
        eprintln!("forme-hostd: {}", error.0);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::host::{config::development_process_workers, http::read_request};

    #[test]
    fn service_mode_never_accepts_the_development_process_switch() {
        let flag = vec!["--development-process-workers".into()];
        assert!(development_process_workers(flag.clone(), true).is_err());
        assert!(development_process_workers(flag, false).unwrap());
        assert!(!development_process_workers(Vec::new(), true).unwrap());
        assert!(!development_process_workers(Vec::new(), false).unwrap());
        assert!(development_process_workers(vec!["--unknown".into()], false).is_err());
    }

    #[test]
    fn ordinary_http11_is_parsed_and_ambiguous_or_browser_requests_are_rejected() {
        let mut request = std::io::Cursor::new(b"GET /v1/host/status HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer example\r\n\r\n");
        let parsed = read_request(&mut request).unwrap();
        assert_eq!(parsed.path, "/v1/host/status");
        for request in [
            "POST /v1/host/plan HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n",
            "POST /v1/host/plan HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
            "GET /healthz HTTP/1.1\r\nOrigin: http://example.invalid\r\n\r\n",
            "GET /healthz HTTP/1.1\r\n\r\nextra",
        ] {
            assert!(read_request(&mut std::io::Cursor::new(request)).is_err());
        }
    }
}
