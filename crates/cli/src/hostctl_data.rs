//! Explicit local maintenance; no HTTP/admin shell, implicit retry or in-place
//! database replacement. Paths are operator inputs, never browser payloads.
use forme_gateway::offline_data::{OfflineCoreMaintenance, OfflineRestorePlan};
use forme_protocol as p;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub fn run(args: &[String]) -> Result<(), String> {
    let [command, flag, config, tail @ ..] = args else {
        return Err(usage());
    };
    if flag != "--config" {
        return Err(usage());
    }
    let config = checked(config)?;
    match (command.as_str(), tail) {
        ("inspect-data", []) => {
            let data = OfflineCoreMaintenance::open(&config, None).map_err(|e| e.0)?;
            print_json(data.identity())
        }
        ("verify-data", [source_flag, source]) if source_flag == "--source" => {
            let data =
                OfflineCoreMaintenance::open(&config, Some(&checked(source)?)).map_err(|e| e.0)?;
            print_json(&data.verify().map_err(|e| e.0)?)
        }
        ("backup-data", [target_flag, target]) if target_flag == "--target" => {
            let data = OfflineCoreMaintenance::open(&config, None).map_err(|e| e.0)?;
            print_json(&data.backup(&checked(target)?).map_err(|e| e.0)?)
        }
        ("plan-restore-data", [source_flag, source, target_flag, target])
            if source_flag == "--source" && target_flag == "--target" =>
        {
            let target = checked(target)?;
            let data =
                OfflineCoreMaintenance::open(&config, Some(&checked(source)?)).map_err(|e| e.0)?;
            let plan = data.plan_restore(&target).map_err(|e| e.0)?;
            persist(&sidecar(&target, "restore-plan.json"), &plan)?;
            print_json(&plan)
        }
        ("restore-data", [source_flag, source, target_flag, target, approve_flag, approved])
            if source_flag == "--source"
                && target_flag == "--target"
                && approve_flag == "--approve" =>
        {
            let target = checked(target)?;
            let plan: OfflineRestorePlan =
                serde_json::from_slice(&read(&sidecar(&target, "restore-plan.json"))?)
                    .map_err(|_| "restore plan is invalid".to_owned())?;
            let approved = p::SchemaDigest(approved.clone());
            p::validate_product_digest(&approved).map_err(|e| e.0)?;
            if approved != plan.digest {
                return Err("restore approval differs from the prepared plan".into());
            }
            // Reject an old receipt before attempting any effect. A partial
            // target also remains fenced by create_new in the store facade.
            let receipt_path = sidecar(&target, "restore-receipt.json");
            if fs::symlink_metadata(&receipt_path).is_ok() {
                return Err("restore receipt already exists; inspect the original result".into());
            }
            let data =
                OfflineCoreMaintenance::open(&config, Some(&checked(source)?)).map_err(|e| e.0)?;
            let receipt = data.restore(&target, &plan, &approved).map_err(|e| e.0)?;
            persist(&receipt_path, &receipt)?;
            print_json(&receipt)
        }
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage: forme-hostctl <inspect-data|backup-data|verify-data|plan-restore-data|restore-data> --config <absolute-core-config> [--source <backup>] [--target <new-database>] [--approve <prepared-digest>]".into()
}

fn checked(value: impl AsRef<Path>) -> Result<PathBuf, String> {
    let path = value.as_ref();
    if !path.is_absolute()
        || path.components().any(|p| {
            matches!(p, Component::ParentDir | Component::CurDir)
                || matches!(p,Component::Normal(n) if n.to_string_lossy().contains(':'))
        })
    {
        return Err("maintenance path must be absolute and direct".into());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err("maintenance path cannot be indirect".into());
                    }
                }
                if meta.file_type().is_symlink() {
                    return Err("maintenance path cannot be indirect".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("maintenance path is unavailable".into()),
        }
    }
    Ok(path.to_owned())
}
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
}
fn read(path: &Path) -> Result<Vec<u8>, String> {
    checked(path)?;
    let f = fs::File::open(path).map_err(|_| "maintenance record is unavailable".to_owned())?;
    let meta = f
        .metadata()
        .map_err(|_| "maintenance record is unavailable".to_owned())?;
    if !meta.is_file() || meta.len() > 64 * 1024 {
        return Err("maintenance record exceeds its bound".into());
    }
    let mut bytes = Vec::new();
    f.take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "maintenance record is unavailable".to_owned())?;
    if bytes.len() > 64 * 1024 {
        return Err("maintenance record exceeds its bound".into());
    }
    Ok(bytes)
}
fn persist<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    checked(path)?;
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|_| "maintenance record encoding failed".to_owned())?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "maintenance record target must be new".to_owned())?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| {
            "maintenance record persistence is unconfirmed; inspect the original operation"
                .to_owned()
        })
}
fn print_json<T: serde::Serialize>(value: &T) -> Result<(), String> {
    let encoded = serde_json::to_string_pretty(value)
        .map_err(|_| "maintenance result encoding failed".to_owned())?;
    println!("{encoded}");
    Ok(())
}
