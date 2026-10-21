use super::*;

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("forme-bridge-keys-{}", random_nonce().unwrap()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn bridge_credentials_are_bounded_regular_files_and_never_echo_private_input() {
    let fixture = Fixture::new();
    let token = fixture.0.join("token");
    let key = fixture.0.join("key");
    let secret = "repository-private-bridge-fixture-0123456789";
    std::fs::write(&token, format!("{secret}\r\n")).unwrap();
    assert_eq!(read_secret(&token, "bridge credential").unwrap(), secret);
    std::fs::write(&key, [97; 32]).unwrap();
    let raw = read_signing_key(&key).unwrap();
    std::fs::write(&key, hex_encode(&[97; 32])).unwrap();
    assert_eq!(read_signing_key(&key).unwrap().to_bytes(), raw.to_bytes());
    for value in [secret.repeat(1000), format!("{secret} "), "short".into()] {
        std::fs::write(&token, value).unwrap();
        let error = read_secret(&token, "bridge credential")
            .unwrap_err()
            .to_string();
        assert!(!error.contains(secret));
        assert!(!error.contains(&fixture.0.to_string_lossy().to_string()));
    }
    std::fs::write(&key, [97; 65]).unwrap();
    assert!(read_signing_key(&key).is_err());
    assert!(read_signing_key(&fixture.0).is_err());
    assert!(read_secret(Path::new("relative-token"), "bridge credential").is_err());
    let missing = fixture.0.join("missing");
    assert!(read_signing_key(&missing).is_err());
    assert!(!missing.exists());
}
