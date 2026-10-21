//! HTTP secrets responsibilities; no independent runtime or writer.
use super::*;

pub(super) fn load_or_create_token() -> p::Result<(LocalBearerSecret, Option<PathBuf>)> {
    if let Ok(token) = std::env::var("FORME_GATEWAY_TOKEN") {
        return Ok((LocalBearerSecret::new(token)?, None));
    }
    let path = std::env::var_os("FORME_GATEWAY_TOKEN_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(default_token_path);
    if let Ok(token) = fs::read_to_string(&path) {
        return Ok((LocalBearerSecret::new(token.trim().to_owned())?, Some(path)));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            p::Error(format!("failed to create gateway state directory: {error}"))
        })?;
    }
    let token = generate_token()?;
    match create_secret_file(&path, token.as_bytes()) {
        Ok(()) => Ok((LocalBearerSecret::new(token)?, Some(path))),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let existing = fs::read_to_string(&path).map_err(|read_error| {
                p::Error(format!("failed to read gateway token: {read_error}"))
            })?;
            Ok((
                LocalBearerSecret::new(existing.trim().to_owned())?,
                Some(path),
            ))
        }
        Err(error) => Err(p::Error(format!("failed to store gateway token: {error}"))),
    }
}

pub(super) fn create_secret_file(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()
}

pub(super) fn generate_token() -> p::Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|error| p::Error(format!("failed to obtain gateway token entropy: {error}")))?;
    let mut token = String::with_capacity(bytes.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        token.push(HEX[(byte >> 4) as usize] as char);
        token.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(token)
}

pub(crate) fn default_token_path() -> PathBuf {
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join("forme").join("gateway.token");
    }
    #[cfg(not(windows))]
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime).join("forme").join("gateway.token");
    }
    std::env::temp_dir().join("forme").join("gateway.token")
}
