use std::collections::HashSet;
use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const PREVIEW_SCRIPT: &str = r#"label={2}; path={3}; created={4}; note={5}; display_path=$path; case "$path" in "$HOME") display_path="~" ;; "$HOME"/*) display_path="~/${path#"$HOME"/}" ;; esac; cols=${FZF_PREVIEW_COLUMNS:-80}; case "$cols" in ""|*[!0-9]*) cols=80 ;; esac; [ "$cols" -gt 1 ] && cols=$((cols - 1)); sep=; i=0; while [ "$i" -lt "$cols" ]; do sep="${sep}-"; i=$((i + 1)); done; printf "Label: %s\nPath: %s\nCreated: %s\n" "$label" "$display_path" "$created"; [ -n "$note" ] && printf "Note: %s\n" "$note"; printf "\n%s\n" "$sep"; if [ -d "$path" ]; then /bin/ls -la "$path"; elif command -v bat >/dev/null 2>&1; then bat --style=numbers --color=always "$path"; else /bin/cat "$path"; fi"#;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Record {
    label: String,
    path: String,
    created: String,
    note: String,
}

struct App {
    command_name: String,
    home: PathBuf,
    dir: PathBuf,
    file: PathBuf,
    open_commands: Vec<String>,
}

impl App {
    fn from_env() -> Result<Self> {
        let args0 = env::args().next().unwrap_or_else(|| "waystone".to_string());
        let command_name = Path::new(&args0)
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("waystone")
            .to_string();

        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("waystone: HOME is not set")?;
        let xdg_state_home = env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/state"));
        let dir = env::var_os("WAYSTONE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| xdg_state_home.join("waystone"));
        let file = env::var_os("WAYSTONE_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| dir.join("paths.tsv"));
        let open_commands = env::var("WAYSTONE_OPEN_COMMANDS")
            .unwrap_or_else(|_| "nvim less bat yazi".to_string())
            .split_whitespace()
            .map(str::to_string)
            .collect();

        Ok(Self {
            command_name,
            home,
            dir,
            file,
            open_commands,
        })
    }

    fn init_state(&self) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent)?;
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)?;
        Ok(())
    }

    fn usage(&self) -> String {
        format!(
            r#"Usage:
  {0} [command...]            Fuzzy-pick a waystone and open it here
  {0} add <path> [label]       Save a path with an optional label
  {0} capture [label]          Save stdin to a temp file and mark it
  {0} import                   Add newline-delimited paths from stdin
  {0} list                     Print saved waystones
  {0} pick                     Fuzzy-pick a waystone and copy its path
  {0} open [command...]        Fuzzy-pick a waystone and open it here
                               Without a command, fuzzy-pick an opener
  {0} remove                   Fuzzy-select waystones to remove from registry
  {0} prune                    Remove waystones whose paths no longer exist
  {0} edit                     Open the TSV registry in $EDITOR
  {0} relabel [label]          Fuzzy-select a waystone and update its label
  {0} note [note]              Fuzzy-select a waystone and update its note
  {0} move <destination-dir>   Move selected path and update its waystone

Examples:
  {0} add ~/notes/todo.md "todo"
  rg TODO . | {0} capture "todo results"
  fd -e md . | {0} import
  {0}
  {0} nvim
  {0} pick
  {0} open
  {0} open less
  wisp {0}
  wisp {0} nvim
  wisp {0} less
  {0} remove
  {0} relabel "better label"
  {0} note "why this path matters"
  {0} move ~/Notes/archive
"#,
            self.command_name
        )
    }

    fn resolve_path(&self, raw: &str) -> Result<PathBuf> {
        let expanded = expand_home(raw, &self.home);
        if expanded.is_dir() {
            return Ok(fs::canonicalize(expanded)?);
        }
        if expanded.exists() {
            let dir = expanded
                .parent()
                .ok_or_else(|| format!("waystone: could not resolve parent for {raw}"))?;
            let base = expanded
                .file_name()
                .ok_or_else(|| format!("waystone: could not resolve basename for {raw}"))?;
            return Ok(fs::canonicalize(dir)?.join(base));
        }
        Err(format!("waystone: path does not exist: {raw}").into())
    }

    fn display_path(&self, path: &Path) -> String {
        match path.strip_prefix(&self.home) {
            Ok(relative) if !relative.as_os_str().is_empty() => {
                format!("~/{}", relative.display())
            }
            _ => path.display().to_string(),
        }
    }

    fn append_waystone(&self, path: &Path, label: &str, note: &str) -> Result<()> {
        let mut file = OpenOptions::new().append(true).open(&self.file)?;
        writeln!(
            file,
            "{}\t{}\t{}\t{}",
            tsv_field(label),
            path.display(),
            timestamp(),
            tsv_field(note)
        )?;
        Ok(())
    }

    fn read_records(&self) -> Result<Vec<Record>> {
        let content = fs::read_to_string(&self.file)?;
        Ok(content.lines().map(parse_record).collect())
    }

    fn write_records(&self, records: &[Record]) -> Result<()> {
        let tmp = self.temp_path("paths.tsv.tmp");
        {
            let mut file = File::create(&tmp)?;
            for record in records {
                writeln!(
                    file,
                    "{}\t{}\t{}\t{}",
                    tsv_field(&record.label),
                    record.path,
                    tsv_field(&record.created),
                    tsv_field(&record.note)
                )?;
            }
        }
        fs::rename(tmp, &self.file)?;
        Ok(())
    }

    fn temp_path(&self, suffix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        self.file
            .with_file_name(format!(
                ".{}.{}.{}",
                self.file
                    .file_name()
                    .and_then(OsStr::to_str)
                    .unwrap_or("waystone"),
                std::process::id(),
                nonce
            ))
            .with_extension(suffix)
    }

    fn rows(&self) -> Result<Vec<String>> {
        Ok(self
            .read_records()?
            .into_iter()
            .enumerate()
            .map(|(idx, record)| {
                let label = if record.label.is_empty() {
                    "(untitled)".to_string()
                } else {
                    record.label
                };
                format!(
                    "{}\t{}\t{}\t{}\t{}",
                    idx + 1,
                    label,
                    record.path,
                    record.created,
                    record.note
                )
            })
            .collect())
    }

    fn select_waystone_path(&self) -> Result<String> {
        if fs::metadata(&self.file)?.len() == 0 {
            return Err("waystone: no saved waystones".into());
        }

        let selection = self
            .select_waystone_rows(false)?
            .ok_or("waystone: selection cancelled")?;
        field(&selection, 2)
            .map(str::to_string)
            .ok_or_else(|| "waystone: selected row is malformed".into())
    }

    fn select_waystone_rows(&self, multi: bool) -> Result<Option<String>> {
        if fs::metadata(&self.file)?.len() == 0 {
            return Err("waystone: no saved waystones".into());
        }

        let mut args = picker_args();
        args.extend([
            "--delimiter".to_string(),
            "\t".to_string(),
            "--with-nth=2".to_string(),
            "--preview".to_string(),
            PREVIEW_SCRIPT.to_string(),
            "--preview-window=right:60%:wrap".to_string(),
        ]);
        if multi {
            args.push("--multi".to_string());
        }

        run_fzf(&args, &self.rows()?.join("\n"))
    }

    fn select_open_command(&self) -> Result<String> {
        let args = style_args_with_labels(" open with ", "open with › ");
        run_fzf(&args, &self.open_commands.join("\n"))?
            .ok_or_else(|| "waystone: opener selection cancelled".into())
    }

    fn cmd_add(&self, args: &[String]) -> Result<()> {
        if args.is_empty() {
            eprint!("{}", self.usage());
            return Err("waystone: add requires a path".into());
        }
        let path = self.resolve_path(&args[0])?;
        let label = args
            .get(1)
            .cloned()
            .unwrap_or_else(|| self.display_path(&path));
        self.append_waystone(&path, &label, "")?;
        println!("Marked: {} -> {}", label, path.display());
        Ok(())
    }

    fn cmd_capture(&self, args: &[String]) -> Result<()> {
        let label = args.first().map(String::as_str).unwrap_or("capture");
        let path = self
            .dir
            .join("captures")
            .join(format!("{}.txt", capture_stamp()));

        if io::stdin().is_terminal() {
            let content = run_capture("pbpaste", &[]).unwrap_or_default();
            let candidate = content.lines().next().unwrap_or("");
            let clipboard_path = if candidate.is_empty() {
                None
            } else {
                self.resolve_path(candidate).ok()
            };
            if let Some(resolved) = clipboard_path {
                self.append_waystone(&resolved, label, "")?;
                println!("Marked clipboard path: {} -> {}", label, resolved.display());
                return Ok(());
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, content)?;
        } else {
            let mut content = String::new();
            io::stdin().read_to_string(&mut content)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, content)?;
        }

        self.append_waystone(&path, label, "")?;
        println!("Captured: {} -> {}", label, path.display());
        Ok(())
    }

    fn cmd_import(&self) -> Result<()> {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        let mut imported = 0usize;
        let mut skipped = 0usize;

        for raw_path in input.lines().filter(|line| !line.is_empty()) {
            match self.resolve_path(raw_path) {
                Ok(path) => {
                    let label = self.display_path(&path);
                    self.append_waystone(&path, &label, "")?;
                    imported += 1;
                }
                Err(_) => {
                    eprintln!("waystone: skipped missing path: {raw_path}");
                    skipped += 1;
                }
            }
        }

        println!("Imported: {imported} marked, {skipped} skipped");
        Ok(())
    }

    fn cmd_list(&self) -> Result<()> {
        for record in self.read_records()? {
            println!(
                "{}\t{}\t{}\t{}",
                record.label, record.path, record.created, record.note
            );
        }
        Ok(())
    }

    fn cmd_pick(&self) -> Result<()> {
        let path = self.select_waystone_path()?;
        pipe_to("pbcopy", &path)?;
        println!("{path}");
        notify("Copied path to clipboard");
        Ok(())
    }

    fn cmd_open(&self, args: &[String]) -> Result<()> {
        let path = self.select_waystone_path()?;
        let mut command: Vec<String> = if args.is_empty() {
            vec![self.select_open_command()?]
        } else {
            args.to_vec()
        };
        command.push(path);

        let program = command.remove(0);
        let err = Command::new(program).args(command).exec();
        Err(Box::new(err))
    }

    fn cmd_remove(&self) -> Result<()> {
        let selection = self
            .select_waystone_rows(true)?
            .ok_or("waystone: selection cancelled")?;
        let numbers: HashSet<usize> = selection
            .lines()
            .filter_map(|line| field(line, 0)?.parse::<usize>().ok())
            .collect();
        let count = numbers.len();
        let records: Vec<_> = self
            .read_records()?
            .into_iter()
            .enumerate()
            .filter(|(idx, _)| !numbers.contains(&(idx + 1)))
            .map(|(_, record)| record)
            .collect();
        self.write_records(&records)?;
        println!("Removed {count} waystone(s)");
        Ok(())
    }

    fn cmd_prune(&self) -> Result<()> {
        let mut removed = 0usize;
        let records: Vec<_> = self
            .read_records()?
            .into_iter()
            .filter(|record| {
                let exists = Path::new(&record.path).exists();
                if !exists {
                    removed += 1;
                }
                exists
            })
            .collect();
        self.write_records(&records)?;
        println!("Pruned {removed} missing waystone(s)");
        Ok(())
    }

    fn cmd_edit(&self) -> Result<()> {
        let editor = env::var("EDITOR").unwrap_or_else(|_| "nvim".to_string());
        let status = Command::new(editor).arg(&self.file).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("waystone: editor exited with status {status}").into())
        }
    }

    fn cmd_relabel(&self, args: &[String]) -> Result<()> {
        let selection = self
            .select_waystone_rows(false)?
            .ok_or("waystone: selection cancelled")?;
        let line_number = selected_line_number(&selection)?;
        let new_label = match args.first() {
            Some(label) => label.clone(),
            None => prompt("New label: ")?,
        };
        let mut records = self.read_records()?;
        let record = records
            .get_mut(line_number - 1)
            .ok_or("waystone: selected row no longer exists")?;
        record.label = new_label.clone();
        self.write_records(&records)?;
        println!("Relabeled waystone {line_number} -> {new_label}");
        Ok(())
    }

    fn cmd_note(&self, args: &[String]) -> Result<()> {
        let selection = self
            .select_waystone_rows(false)?
            .ok_or("waystone: selection cancelled")?;
        let line_number = selected_line_number(&selection)?;
        let new_note = match args.first() {
            Some(note) => note.clone(),
            None => prompt("New note: ")?,
        };
        let mut records = self.read_records()?;
        let record = records
            .get_mut(line_number - 1)
            .ok_or("waystone: selected row no longer exists")?;
        record.note = new_note;
        self.write_records(&records)?;
        println!("Updated note for waystone {line_number}");
        Ok(())
    }

    fn cmd_move(&self, args: &[String]) -> Result<()> {
        if args.is_empty() {
            eprint!("{}", self.usage());
            return Err("waystone: move requires a destination directory".into());
        }
        let destination_dir = self.resolve_path(&args[0])?;
        if !destination_dir.is_dir() {
            return Err(format!("waystone: destination is not a directory: {}", args[0]).into());
        }
        let selection = self
            .select_waystone_rows(false)?
            .ok_or("waystone: selection cancelled")?;
        let line_number = selected_line_number(&selection)?;
        let old_path =
            PathBuf::from(field(&selection, 2).ok_or("waystone: selected row is malformed")?);
        let file_name = old_path
            .file_name()
            .ok_or("waystone: selected path has no basename")?;
        let new_path = destination_dir.join(file_name);
        if new_path.exists() {
            return Err(format!(
                "waystone: destination already exists: {}",
                new_path.display()
            )
            .into());
        }
        fs::rename(&old_path, &new_path)?;
        let mut records = self.read_records()?;
        let record = records
            .get_mut(line_number - 1)
            .ok_or("waystone: selected row no longer exists")?;
        record.path = new_path.display().to_string();
        self.write_records(&records)?;
        println!("Moved: {} -> {}", old_path.display(), new_path.display());
        Ok(())
    }

    fn dispatch(&self, args: &[String]) -> Result<()> {
        let command = args.first().map(String::as_str).unwrap_or("");
        match command {
            "" => self.cmd_open(&[]),
            "add" => self.cmd_add(&args[1..]),
            "capture" => self.cmd_capture(&args[1..]),
            "import" => self.cmd_import(),
            "list" => self.cmd_list(),
            "pick" => self.cmd_pick(),
            "open" => self.cmd_open(&args[1..]),
            "remove" | "rm" => self.cmd_remove(),
            "prune" => self.cmd_prune(),
            "edit" => self.cmd_edit(),
            "relabel" | "label" => self.cmd_relabel(&args[1..]),
            "note" => self.cmd_note(&args[1..]),
            "move" | "mv" => self.cmd_move(&args[1..]),
            "-h" | "--help" | "help" => {
                print!("{}", self.usage());
                Ok(())
            }
            other if command_resolves(other) => self.cmd_open(args),
            other => {
                eprintln!("waystone: unknown command or opener: {other}");
                eprint!("{}", self.usage());
                Err("waystone: unknown command or opener".into())
            }
        }
    }
}

fn expand_home(raw: &str, home: &Path) -> PathBuf {
    if raw == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.join(rest);
    }
    PathBuf::from(raw)
}

fn tsv_field(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\t' | '\r' | '\n' => ' ',
            _ => ch,
        })
        .collect()
}

fn timestamp() -> String {
    run_capture("date", &["+%Y-%m-%dT%H:%M:%S%z"]).unwrap_or_else(|_| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "0".to_string())
    })
}

fn capture_stamp() -> String {
    run_capture("date", &["+%Y%m%d-%H%M%S"]).unwrap_or_else(|_| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "0".to_string())
    })
}

fn run_capture(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(format!("waystone: {program} exited with status {}", output.status).into());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end_matches('\n')
        .to_string())
}

fn parse_record(line: &str) -> Record {
    let mut fields = line.splitn(4, '\t');
    Record {
        label: fields.next().unwrap_or_default().to_string(),
        path: fields.next().unwrap_or_default().to_string(),
        created: fields.next().unwrap_or_default().to_string(),
        note: fields.next().unwrap_or_default().to_string(),
    }
}

fn field(line: &str, index: usize) -> Option<&str> {
    line.split('\t').nth(index)
}

fn selected_line_number(selection: &str) -> Result<usize> {
    field(selection, 0)
        .ok_or("waystone: selected row is malformed")?
        .parse::<usize>()
        .map_err(|_| "waystone: selected row has invalid line number".into())
}

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

fn picker_args() -> Vec<String> {
    let mut args = style_args();
    args.extend([
        "--border-label= waystone ".to_string(),
        "--preview-label= metadata + preview ".to_string(),
        "--prompt=waystone › ".to_string(),
    ]);
    args
}

fn style_args_with_labels(border_label: &str, prompt: &str) -> Vec<String> {
    let mut args = style_args();
    args.extend([
        format!("--border-label={border_label}"),
        format!("--prompt={prompt}"),
    ]);
    args
}

fn run_fzf(args: &[String], input: &str) -> Result<Option<String>> {
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

fn pipe_to(program: &str, input: &str) -> Result<()> {
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

fn notify(message: &str) {
    let escaped = message.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{escaped}\" with title \"waystone\"");
    let _ = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim_end_matches(['\r', '\n']).to_string())
}

fn command_resolves(command: &str) -> bool {
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

fn real_main() -> Result<()> {
    let app = App::from_env()?;
    app.init_state()?;
    let args: Vec<String> = env::args().skip(1).collect();
    app.dispatch(&args)
}

fn main() {
    if let Err(error) = real_main() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tsv_record() {
        assert_eq!(
            parse_record("Label\t/path\tcreated\tnote with spaces"),
            Record {
                label: "Label".to_string(),
                path: "/path".to_string(),
                created: "created".to_string(),
                note: "note with spaces".to_string(),
            }
        );
    }

    #[test]
    fn sanitizes_tsv_fields() {
        assert_eq!(tsv_field("a\tb\nc\r"), "a b c ");
    }

    #[test]
    fn expands_home_prefix() {
        assert_eq!(
            expand_home("~/notes", Path::new("/Users/example")),
            PathBuf::from("/Users/example/notes")
        );
        assert_eq!(
            expand_home("/tmp", Path::new("/Users/example")),
            PathBuf::from("/tmp")
        );
    }
}
