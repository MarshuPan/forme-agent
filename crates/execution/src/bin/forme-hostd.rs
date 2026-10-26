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
    use super::host::{
        config::{launch_from_args, Launch},
        http::read_request,
    };

    #[test]
    fn service_mode_never_accepts_the_development_process_switch() {
        assert!(matches!(
            launch_from_args(vec!["--development-process-workers".into()]).unwrap(),
            Launch::Development {
                process_workers: true
            }
        ));
        assert!(matches!(
            launch_from_args(Vec::new()).unwrap(),
            Launch::Development {
                process_workers: false
            }
        ));
        assert!(matches!(
            launch_from_args(vec![
                "--development-config".into(),
                std::env::current_dir()
                    .unwrap()
                    .join("absolute.json")
                    .into_os_string(),
            ])
            .unwrap(),
            Launch::DevelopmentConfig(_)
        ));
        assert!(launch_from_args(vec!["--config".into(), "relative.json".into()]).is_err());
        assert!(
            launch_from_args(vec!["--development-config".into(), "relative.json".into()]).is_err()
        );
        assert!(launch_from_args(vec![
            "--config".into(),
            "--development-process-workers".into()
        ])
        .is_err());
        assert!(launch_from_args(vec!["--unknown".into()]).is_err());
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
