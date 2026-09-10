use std::io::Write;
use std::process::{Command, Stdio};

use crate::Result;

pub(crate) const PREVIEW_SCRIPT: &str = r#"label={2}; path={3}; created={4}; note={5}; display_path=$path; case "$path" in "$HOME") display_path="~" ;; "$HOME"/*) display_path="~/${path#"$HOME"/}" ;; esac; cols=${FZF_PREVIEW_COLUMNS:-80}; case "$cols" in ""|*[!0-9]*) cols=80 ;; esac; [ "$cols" -gt 1 ] && cols=$((cols - 1)); sep=; i=0; while [ "$i" -lt "$cols" ]; do sep="${sep}-"; i=$((i + 1)); done; printf "Label: %s\nPath: %s\nCreated: %s\n" "$label" "$display_path" "$created"; [ -n "$note" ] && printf "Note: %s\n" "$note"; printf "\n%s\n" "$sep"; if [ -d "$path" ]; then /bin/ls -la "$path"; elif command -v bat >/dev/null 2>&1; then bat --style=numbers --color=always "$path"; else /bin/cat "$path"; fi"#;

fn style_args() -> Vec<String> {
    [
        "--height=90%",
        "--layout=reverse",
        "--border=rounded",
        "--preview-border=rounded",
        "--info=inline-right",
        "--pointer=▶",
        "--marker=✓",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

pub(crate) fn picker_args() -> Vec<String> {
    let mut args = style_args();
    args.extend([
        "--border-label= waystone ".to_string(),
        "--preview-label= metadata + preview ".to_string(),
        "--prompt=waystone › ".to_string(),
    ]);
    args
}

pub(crate) fn style_args_with_labels(border_label: &str, prompt: &str) -> Vec<String> {
    let mut args = style_args();
    args.extend([
        format!("--border-label={border_label}"),
        format!("--prompt={prompt}"),
    ]);
    args
}

pub(crate) fn run_fzf(args: &[String], input: &str) -> Result<Option<String>> {
    let mut child = Command::new("fzf")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input.as_bytes())?;
        if !input.ends_with('\n') {
            stdin.write_all(b"\n")?;
        }
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let selection = String::from_utf8_lossy(&output.stdout)
        .trim_end_matches('\n')
        .to_string();
    if selection.is_empty() {
        Ok(None)
    } else {
        Ok(Some(selection))
    }
}
