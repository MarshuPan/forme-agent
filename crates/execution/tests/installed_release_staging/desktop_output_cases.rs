//! Real signed/staged bytes and Windows custody; no screenshot or service claim.
#![cfg(windows)]
use super::*;
use forme_execution::installed::{
    DesktopHelperAdapter, WindowsDesktopHelperAdapter, WindowsDesktopSessionCatalog,
};
const DECLARATION: &str = "share/forme/desktop-output-v2.json";
const HELPER: &str = "bin/forme-executord.exe";

fn with_declaration(change: impl FnOnce(&mut serde_json::Value)) -> Fixture {
    let mut f = Fixture::new();
    let helper = f
        .release
        .files
        .iter()
        .find(|file| file.relative_path == HELPER)
        .unwrap();
    let mut value = serde_json::json!({"schema_version":2,"helper_digest":helper.digest,"helper_bytes":helper.size});
    change(&mut value);
    let bytes = serde_json::to_vec(&value).unwrap();
    let path = f.source().join("content").join(DECLARATION);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, &bytes).unwrap();
    f.release.files.push(p::ReleaseFile {
        schema_version: p::SchemaVersion(1),
        relative_path: DECLARATION.into(),
        digest: p::sha256_content_digest(&bytes),
        size: bytes.len() as u64,
    });
    f.release
        .files
        .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    fs::write(
        f.source().join("manifest.json"),
        serde_json::to_vec(&f.release).unwrap(),
    )
    .unwrap();
    let signing = SigningKey::from_bytes(&[0x36; 32]);
    fs::write(
        f.source().join("signature.ed25519"),
        signing
            .sign(f.release.digest().unwrap().0.as_bytes())
            .to_bytes(),
    )
    .unwrap();
    f
}
fn verified(f: &Fixture) -> forme_execution::installed::VerifiedStagedRelease {
    let journal = f.stage();
    f.stager()
        .verified_staged(&journal, RELEASE, &f.release.digest().unwrap())
        .unwrap()
}

#[test]
fn desktop_output_signed_helper_and_declaration_stay_in_original_file_custody() {
    let f = with_declaration(|_| {});
    let release = verified(&f);
    let helper = WindowsDesktopHelperAdapter::from_verified_release(&release).unwrap();
    let catalog = WindowsDesktopSessionCatalog::from_verified_release(&release).unwrap();
    assert!(helper.supports_output());
    for relative in [HELPER, DECLARATION] {
        let path = release.content_root().join(relative);
        assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
        assert!(fs::rename(&path, path.with_extension("substitution")).is_err());
        assert!(fs::remove_file(&path).is_err());
    }
    for directory in ["bin", "share/forme"] {
        let path = release.content_root().join(directory);
        assert!(fs::rename(&path, path.with_extension("substitution")).is_err());
    }
    drop(helper);
    assert!(fs::OpenOptions::new()
        .write(true)
        .open(release.content_root().join(HELPER))
        .is_err());
    drop(catalog);
    assert!(fs::OpenOptions::new()
        .write(true)
        .open(release.content_root().join(HELPER))
        .is_ok());
    release.recheck().unwrap();
}

#[test]
fn desktop_output_unsigned_path_and_signed_release_without_declaration_stay_disabled() {
    let f = with_declaration(|_| {});
    let helper = WindowsDesktopHelperAdapter::new(f.source().join("content").join(HELPER)).unwrap();
    assert!(!helper.supports_output());
    let f = Fixture::new();
    let release = verified(&f);
    let helper = WindowsDesktopHelperAdapter::from_verified_release(&release).unwrap();
    assert!(!helper.supports_output());
}

#[test]
fn desktop_output_even_valid_signatures_cannot_bind_wrong_schema_digest_size_or_fields() {
    for change in 0..5 {
        let f = with_declaration(|value| match change {
            0 => value["schema_version"] = serde_json::json!(1),
            1 => {
                value["helper_digest"] =
                    serde_json::json!(p::sha256_content_digest(b"different helper"))
            }
            2 => value["helper_bytes"] = serde_json::json!(1),
            3 => value["additional_authority"] = serde_json::json!(true),
            _ => value["helper_digest"] = serde_json::json!("a".repeat(5000)),
        });
        let release = verified(&f);
        assert!(WindowsDesktopHelperAdapter::from_verified_release(&release).is_err());
        assert!(WindowsDesktopSessionCatalog::from_verified_release(&release).is_err());
    }
}

#[test]
fn desktop_output_rechecks_replaced_signed_bytes_before_acquiring_support() {
    for relative in [HELPER, DECLARATION] {
        let f = with_declaration(|_| {});
        let release = verified(&f);
        let path = release.content_root().join(relative);
        let mut bytes = fs::read(&path).unwrap();
        bytes[0] ^= 1;
        fs::write(&path, &bytes).unwrap();
        assert!(WindowsDesktopHelperAdapter::from_verified_release(&release).is_err());
        assert!(WindowsDesktopSessionCatalog::from_verified_release(&release).is_err());
    }
}

#[test]
fn desktop_output_signed_helper_with_an_external_hardlink_is_rejected() {
    let f = with_declaration(|_| {});
    let release = verified(&f);
    fs::hard_link(
        release.content_root().join(HELPER),
        f.root.path().join("linked-helper.exe"),
    )
    .unwrap();
    assert!(WindowsDesktopHelperAdapter::from_verified_release(&release).is_err());
}
