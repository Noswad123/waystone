use std::env;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::Result;

pub(crate) fn run_capture(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(format!("waystone: {program} exited with status {}", output.status).into());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end_matches('\n')
        .to_string())
}

pub(crate) fn pipe_to(program: &str, input: &str) -> Result<()> {
    let mut child = Command::new(program).stdin(Stdio::piped()).spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input.as_bytes())?;
        stdin.write_all(b"\n")?;
    }
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("waystone: {program} exited with status {status}").into())
    }
}

pub(crate) fn notify(message: &str) {
    let escaped = message.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{escaped}\" with title \"waystone\"");
    let _ = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

pub(crate) fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim_end_matches(['\r', '\n']).to_string())
}

pub(crate) fn command_resolves(command: &str) -> bool {
    let path = Path::new(command);
    if command.contains('/') {
        return is_executable(path);
    }
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| is_executable(&dir.join(command))))
        .unwrap_or(false)
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}
