//! Fixed native maintenance program. No AgentExecution, shell text or env input.
use std::{
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

const SCRIPT: &str = include_str!("firewall_windows.ps1");
// Do not search PATH or trust a caller-controlled SystemRoot to select a
// privileged executable. Other Windows directory layouts remain unavailable.
const SYSTEM: &str = r"C:\Windows\System32";
const MAX_INPUT: usize = 16_384;
const MAX_OUTPUT: usize = 4096;

#[derive(Serialize)]
struct Binding {
    name: String,
    ip: String,
    port: u16,
    digest: p::SchemaDigest,
}

#[derive(Serialize)]
struct Request {
    schema_version: p::SchemaVersion,
    mode: &'static str,
    host_program: PathBuf,
    web_program: PathBuf,
    host_config: PathBuf,
    web_config: PathBuf,
    previous: Option<Binding>,
    desired: Option<Binding>,
    request_digest: p::SchemaDigest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: p::SchemaVersion,
    request_digest: p::SchemaDigest,
    completed: bool,
}

fn unavailable() -> p::Error {
    p::Error("native firewall observation is unavailable or unsettled; inspect the original operation before retrying".into())
}

fn binding(
    network: &p::ProductNetworkConfig,
    program: &std::path::Path,
) -> p::Result<Option<Binding>> {
    network.validate()?;
    network
        .firewall
        .as_ref()
        .map(|rule| {
            Ok(Binding {
                name: format!(
                    "forme.web.lan.{}",
                    p::sha256_content_digest(rule.rule_ref.as_bytes())
                        .0
                        .trim_start_matches("sha256:")
                ),
                ip: network.bind.ip().to_string(),
                port: network.bind.port(),
                digest: p::canonical_digest(&(
                    "forme-native-web-rule-v1",
                    &rule.rule_ref,
                    network.bind,
                    program,
                ))?,
            })
        })
        .transpose()
}

fn request(
    mode: &'static str,
    networks: Option<(&p::ProductNetworkConfig, &p::ProductNetworkConfig)>,
) -> p::Result<Request> {
    let (host_program, web_program, host_config, web_config) =
        super::registration::verify_network_installation()?;
    let (previous, desired) = match networks {
        Some((previous, desired)) => (
            binding(previous, &web_program)?,
            binding(desired, &web_program)?,
        ),
        None => (None, None),
    };
    let request_digest = p::canonical_digest(&(
        "forme-native-firewall-request-v1",
        mode,
        &host_program,
        &web_program,
        &host_config,
        &web_config,
        &previous,
        &desired,
    ))?;
    Ok(Request {
        schema_version: p::SchemaVersion(1),
        mode,
        host_program,
        web_program,
        host_config,
        web_config,
        previous,
        desired,
        request_digest,
    })
}

pub(super) fn available() -> p::Result<()> {
    run(&request("check", None)?)
}

pub(super) fn verify(
    previous: &p::ProductNetworkConfig,
    desired: &p::ProductNetworkConfig,
) -> p::Result<()> {
    run(&request("verify", Some((previous, desired)))?)
}

pub(super) fn reconcile(
    previous: &p::ProductNetworkConfig,
    desired: &p::ProductNetworkConfig,
) -> p::Result<p::NativeFirewallReceipt> {
    // Repeat SCM and privilege/path checks immediately before the network effect.
    verify(previous, desired)?;
    Ok(p::NativeFirewallReceipt {
        schema_version: p::SchemaVersion(1),
        bind: desired.bind,
        rule_ref: desired.firewall.as_ref().map(|rule| rule.rule_ref.clone()),
        network_digest: desired.validation_digest()?,
        enabled: !desired.bind.ip().is_loopback(),
        removed_rule_refs: previous
            .firewall
            .as_ref()
            .filter(|old| desired.firewall.as_ref() != Some(old))
            .map(|rule| vec![rule.rule_ref.clone()])
            .unwrap_or_default(),
    })
}

fn run(request: &Request) -> p::Result<()> {
    let bytes = serde_json::to_vec(request).map_err(|_| unavailable())?;
    if bytes.len() > MAX_INPUT {
        return Err(unavailable());
    }
    let program = PathBuf::from(SYSTEM).join(r"WindowsPowerShell\v1.0\powershell.exe");
    super::reject_linked_ancestors(&program)?;
    if !program.is_file() {
        return Err(unavailable());
    }
    let command_source = format!("{SCRIPT}\nInvoke-ForMeFirewallInput\n");
    if command_source.encode_utf16().count() > 24_000 {
        return Err(unavailable());
    }
    let mut child = Command::new(&program)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &command_source,
        ])
        .env_clear()
        .env("SystemRoot", r"C:\Windows")
        .env("WINDIR", r"C:\Windows")
        .env(
            "PSModulePath",
            PathBuf::from(SYSTEM).join(r"WindowsPowerShell\v1.0\Modules"),
        )
        .current_dir(SYSTEM)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| unavailable())?;
    let input = child.stdin.take();
    let output = child.stdout.take();
    let (Some(mut input), Some(mut output)) = (input, output) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(unavailable());
    };
    let writer = thread::spawn(move || input.write_all(&bytes));
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        output
            .by_ref()
            .take((MAX_OUTPUT + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(25);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(unavailable());
            }
        }
    };
    // The fixed in-process NetSecurity program never spawns descendant tools.
    // Both pipe workers are joined, including on failure/timeout.
    let written = writer.join();
    let read = reader.join();
    let written = written.map_err(|_| unavailable())?;
    let bytes = read
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())?;
    if written.is_err() || !status?.success() || bytes.len() > MAX_OUTPUT {
        return Err(unavailable());
    }
    let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
    if receipt.schema_version != p::SchemaVersion(1)
        || receipt.request_digest != request.request_digest
        || !receipt.completed
    {
        return Err(unavailable());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(ip: &str) -> p::ProductNetworkConfig {
        p::ProductNetworkConfig {
            schema_version: p::SchemaVersion(1),
            bind: format!("{ip}:8765").parse().unwrap(),
            allowed_hosts: vec![format!("{ip}:8765")],
            tls: Some(p::ProductNetworkTls {
                schema_version: p::SchemaVersion(1),
                certificate_path: r"C:\forme\tls\cert.pem".into(),
                private_key_path: r"C:\forme\tls\key.pem".into(),
            }),
            firewall: Some(p::ProductFirewallBinding {
                schema_version: p::SchemaVersion(1),
                rule_ref: "firewall:fixture".into(),
            }),
        }
    }

    #[test]
    fn native_rule_identity_is_exact_and_binding_changes_with_scope_or_program() {
        let program = std::path::Path::new(r"C:\Program Files\ForMe\bin\forme-webd.exe");
        let a = binding(&network("192.0.2.10"), program).unwrap().unwrap();
        let b = binding(&network("192.0.2.11"), program).unwrap().unwrap();
        assert_eq!(a.name, b.name);
        assert_eq!(a.name.len(), "forme.web.lan.".len() + 64);
        assert_ne!(a.digest, b.digest);
        assert_ne!(
            a.digest,
            binding(
                &network("192.0.2.10"),
                std::path::Path::new(r"C:\other\forme-webd.exe")
            )
            .unwrap()
            .unwrap()
            .digest
        );
        assert!(binding(
            &p::ProductNetworkConfig::loopback("127.0.0.1:8765".parse().unwrap()),
            program
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn development_program_cannot_advertise_native_firewall_support() {
        assert!(available().is_err());
    }
}
