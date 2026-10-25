#![cfg(target_os = "macos")]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::Path;

use forme_protocol as p;

const MAX_XML_BYTES: usize = 256 * 1024;

pub(super) fn sandbox_profile(
    workspace: &Path,
    temporary: &Path,
    inputs: &[&Path],
    program: &Path,
) -> p::Result<String> {
    let mut rules = vec![
        "(version 1)".to_owned(),
        "(deny default)".to_owned(),
        "(allow file-read-metadata)".to_owned(),
        "(allow sysctl-read)".to_owned(),
        "(allow file-read* (subpath \"/System\"))".to_owned(),
        "(allow file-read* (subpath \"/usr\"))".to_owned(),
        "(allow file-read* (subpath \"/bin\"))".to_owned(),
        "(allow file-read* (subpath \"/sbin\"))".to_owned(),
        "(allow file-read* (subpath \"/private/etc\"))".to_owned(),
        "(allow file-read* (subpath \"/private/var/db/dyld\"))".to_owned(),
        "(allow file-read* (literal \"/dev/null\"))".to_owned(),
        "(deny network*)".to_owned(),
    ];
    rules.push(format!(
        "(allow file-read* (subpath {}))",
        profile_literal(workspace)?
    ));
    rules.push(format!(
        "(allow file-write* (subpath {}))",
        profile_literal(workspace)?
    ));
    rules.push(format!(
        "(allow file-read* (subpath {}))",
        profile_literal(temporary)?
    ));
    rules.push(format!(
        "(allow file-write* (subpath {}))",
        profile_literal(temporary)?
    ));
    for input in inputs {
        rules.push(format!(
            "(allow file-read* (literal {}))",
            profile_literal(input)?
        ));
    }
    rules.push(format!(
        "(allow file-read* (literal {}))",
        profile_literal(program)?
    ));
    rules.push(format!(
        "(allow process-exec (literal {}))",
        profile_literal(program)?
    ));
    Ok(rules.join("\n") + "\n")
}

pub(super) fn launchd_plist(
    label: &str,
    sandbox: &Path,
    program: &Path,
    args: &[String],
    workspace: &Path,
    environment: &BTreeMap<String, String>,
) -> p::Result<String> {
    let mut arguments = vec![
        "/usr/bin/sandbox-exec".to_owned(),
        "-f".to_owned(),
        sandbox_string(sandbox)?,
        "--".to_owned(),
        sandbox_string(program)?,
    ];
    arguments.extend(args.iter().cloned());
    let mut output = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<plist version=\"1.0\"><dict>\n",
    ));
    xml_key_value(&mut output, "Label", label)?;
    xml_key_value(&mut output, "UserName", "_formeexec")?;
    xml_key_value(&mut output, "GroupName", "_formeexec")?;
    output.push_str("  <key>ProgramArguments</key><array>\n");
    for argument in &arguments {
        xml_string(&mut output, argument)?;
    }
    output.push_str("  </array>\n");
    output.push_str("  <key>WorkingDirectory</key>");
    xml_value(&mut output, workspace)?;
    output.push('\n');
    output.push_str("  <key>EnvironmentVariables</key><dict>\n");
    for (key, value) in environment {
        xml_key_value(&mut output, key, value)?;
    }
    output.push_str(concat!(
        "  </dict>\n",
        "  <key>RunAtLoad</key><true/>\n",
        "  <key>LaunchOnlyOnce</key><true/>\n",
        "  <key>AbandonProcessGroup</key><false/>\n",
        "  <key>ProcessType</key><string>Background</string>\n",
        "  <key>HardResourceLimits</key><dict><key>NumberOfFiles</key><integer>4096</integer></dict>\n",
        "  <key>StandardOutPath</key><string>/dev/null</string>\n",
        "  <key>StandardErrorPath</key><string>/dev/null</string>\n",
        "</dict></plist>\n",
    ));
    if output.len() > MAX_XML_BYTES {
        return Err(p::Error("macOS worker launch plist is too large".into()));
    }
    Ok(output)
}

fn profile_literal(path: &Path) -> p::Result<String> {
    let value = path
        .to_str()
        .ok_or_else(|| p::Error("macOS worker path is not valid UTF-8".into()))?;
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(p::Error("macOS worker path is invalid".into()));
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

fn sandbox_string(path: &Path) -> p::Result<String> {
    let value = path
        .to_str()
        .ok_or_else(|| p::Error("macOS worker launch value is not valid UTF-8".into()))?;
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(p::Error("macOS worker launch value is invalid".into()));
    }
    Ok(value.to_owned())
}

fn xml_key_value(output: &mut String, key: &str, value: &str) -> p::Result<()> {
    if key.is_empty() || key.chars().any(char::is_control) {
        return Err(p::Error("macOS worker plist key is invalid".into()));
    }
    output.push_str("  <key>");
    xml_escape(output, key)?;
    output.push_str("</key><string>");
    xml_escape(output, value)?;
    output.push_str("</string>\n");
    Ok(())
}

fn xml_string(output: &mut String, value: &str) -> p::Result<()> {
    output.push_str("    <string>");
    xml_escape(output, value)?;
    output.push_str("</string>\n");
    Ok(())
}

fn xml_value(output: &mut String, path: &Path) -> p::Result<()> {
    output.push_str("<string>");
    xml_escape(output, &sandbox_string(path)?)?;
    output.push_str("</string>");
    Ok(())
}

fn xml_escape(output: &mut String, value: &str) -> p::Result<()> {
    if value.chars().any(char::is_control) {
        return Err(p::Error("macOS worker plist value is invalid".into()));
    }
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&apos;"),
            _ => output.push(character),
        }
    }
    Ok(())
}
