use std::env;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::picker::{
    picker_args, run_fzf, style_args_with_labels, HELP_POPUP_SCRIPT, PREVIEW_SCRIPT,
};
use crate::record::{field, parse_record, selected_line_number, tsv_field, Record};
use crate::time::timestamp;
use crate::Result;

pub(crate) struct App {
    pub(crate) command_name: String,
    pub(crate) home: PathBuf,
    pub(crate) dir: PathBuf,
    pub(crate) file: PathBuf,
    pub(crate) open_commands: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaystonePickerMode {
    Select,
    Search,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WaystonePickerAction {
    Open(String),
    Search,
    BackToSelection(String),
    EditAndExit(String),
    EditAndReturn(String),
    Delete(usize),
    Rename(usize),
    NewEntry,
    Cancel,
}

impl App {
    pub(crate) fn from_env() -> Result<Self> {
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

    pub(crate) fn init_state(&self) -> Result<()> {
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

    pub(crate) fn usage(&self) -> String {
        format!(
            r#"Usage:
  {0} [command...]            Fuzzy-pick a waystone and open it here
  {0} add <path> [label]       Save a path with an optional label
  {0} capture [label]          Save stdin to a temp file and mark it
  {0} import                   Add newline-delimited paths from stdin
  {0} list                     Print saved waystones
  {0} pick                     Fuzzy-pick a waystone and copy its path
  {0} select [--action]        Fuzzy-pick and print a path or editor action
  {0} open [command...]        Fuzzy-pick a waystone and open it here
                               Without a command, fuzzy-pick an opener
  {0} remove                   Fuzzy-select waystones to remove from registry
  {0} prune                    Remove waystones whose paths no longer exist
  {0} edit                     Open the TSV registry in $EDITOR
  {0} relabel [label]          Fuzzy-select a waystone and update its label
  {0} note [note]              Fuzzy-select a waystone and update its note
  {0} move <destination-dir>   Move selected path and update its waystone

Interactive open picker:
  / search · Esc return/quit · ? keybindings

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

    pub(crate) fn resolve_path(&self, raw: &str) -> Result<PathBuf> {
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

    pub(crate) fn display_path(&self, path: &Path) -> String {
        match path.strip_prefix(&self.home) {
            Ok(relative) if !relative.as_os_str().is_empty() => {
                format!("~/{}", relative.display())
            }
            _ => path.display().to_string(),
        }
    }

    pub(crate) fn append_waystone(&self, path: &Path, label: &str, note: &str) -> Result<()> {
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

    pub(crate) fn read_records(&self) -> Result<Vec<Record>> {
        let content = fs::read_to_string(&self.file)?;
        Ok(content.lines().map(parse_record).collect())
    }

    pub(crate) fn write_records(&self, records: &[Record]) -> Result<()> {
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

    pub(crate) fn temp_path(&self, suffix: &str) -> PathBuf {
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

    pub(crate) fn rows(&self) -> Result<Vec<String>> {
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

    pub(crate) fn select_waystone_path(&self) -> Result<String> {
        if fs::metadata(&self.file)?.len() == 0 {
            return Err("waystone: no saved waystones".into());
        }

        let selection = self
            .select_waystone_rows(false)?
            .ok_or("waystone: selection cancelled")?;
        crate::record::field(&selection, 2)
            .map(str::to_string)
            .ok_or_else(|| "waystone: selected row is malformed".into())
    }

    pub(crate) fn select_waystone_rows(&self, multi: bool) -> Result<Option<String>> {
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

    pub(crate) fn select_waystone_action(
        &self,
        mode: WaystonePickerMode,
        rows: Option<&str>,
    ) -> Result<WaystonePickerAction> {
        let all_rows;
        let input = match rows {
            Some(rows) => rows,
            None => {
                all_rows = self.rows()?.join("\n");
                &all_rows
            }
        };

        let mut args = picker_args();
        args.extend([
            "--delimiter".to_string(),
            "\t".to_string(),
            "--with-nth=2".to_string(),
            "--preview".to_string(),
            PREVIEW_SCRIPT.to_string(),
            "--preview-window=right:60%:wrap".to_string(),
        ]);

        match mode {
            WaystonePickerMode::Select => {
                args.extend([
                    "--no-input".to_string(),
                    "--expect=e,E,d,r,n,/".to_string(),
                    "--bind=j:down,k:up".to_string(),
                    format!("--bind=?:execute({HELP_POPUP_SCRIPT})"),
                    "--footer= / search · Esc quit · ? help ".to_string(),
                ]);
                let output = match run_fzf(&args, input)? {
                    Some(output) => output,
                    None => return Ok(WaystonePickerAction::Cancel),
                };
                self.picker_action_from_expected_output(&output)
            }
            WaystonePickerMode::Search => {
                args.extend([
                    "--prompt=search › ".to_string(),
                    "--print-query".to_string(),
                    "--bind=esc:print(esc)+accept".to_string(),
                    format!("--bind=?:execute({HELP_POPUP_SCRIPT})"),
                    "--footer= Esc selection · ? help ".to_string(),
                ]);
                let output = match run_fzf(&args, input)? {
                    Some(output) => output,
                    None => return Ok(WaystonePickerAction::Cancel),
                };
                picker_action_from_search_output(&output)
            }
        }
    }

    pub(crate) fn filtered_rows(&self, query: &str) -> Result<Vec<String>> {
        if query.is_empty() {
            return self.rows();
        }

        let args = vec!["--filter".to_string(), query.to_string()];
        Ok(run_fzf(&args, &self.rows()?.join("\n"))?
            .map(|output| output.lines().map(str::to_string).collect())
            .unwrap_or_default())
    }

    fn picker_action_from_expected_output(&self, output: &str) -> Result<WaystonePickerAction> {
        let (key, row) = split_expected_output(output);
        match key {
            "/" => Ok(WaystonePickerAction::Search),
            "e" => path_from_optional_row(row).map(WaystonePickerAction::EditAndExit),
            "E" => path_from_optional_row(row).map(WaystonePickerAction::EditAndReturn),
            "d" => line_number_from_optional_row(row).map(WaystonePickerAction::Delete),
            "r" => line_number_from_optional_row(row).map(WaystonePickerAction::Rename),
            "n" => Ok(WaystonePickerAction::NewEntry),
            "" => path_from_optional_row(row).map(WaystonePickerAction::Open),
            _ => Ok(WaystonePickerAction::Cancel),
        }
    }

    pub(crate) fn select_open_command(&self) -> Result<String> {
        let args = style_args_with_labels(" open with ", "open with › ");
        run_fzf(&args, &self.open_commands.join("\n"))?
            .ok_or_else(|| "waystone: opener selection cancelled".into())
    }
}

fn split_expected_output(output: &str) -> (&str, Option<&str>) {
    match output.split_once('\n') {
        Some((key, row)) if !row.is_empty() => (key, Some(row)),
        Some((key, _)) => (key, None),
        None if looks_like_row(output) => ("", Some(output)),
        None => (output, None),
    }
}

fn picker_action_from_search_output(output: &str) -> Result<WaystonePickerAction> {
    let (query, rest) = output.split_once('\n').unwrap_or(("", output));
    if rest == "esc" || rest.starts_with("esc\n") {
        Ok(WaystonePickerAction::BackToSelection(query.to_string()))
    } else {
        path_from_row(rest).map(WaystonePickerAction::Open)
    }
}

fn looks_like_row(output: &str) -> bool {
    field(output, 0)
        .and_then(|value| value.parse::<usize>().ok())
        .is_some()
}

fn path_from_row(row: &str) -> Result<String> {
    field(row, 2)
        .map(str::to_string)
        .ok_or_else(|| "waystone: selected row is malformed".into())
}

fn path_from_optional_row(row: Option<&str>) -> Result<String> {
    path_from_row(row.ok_or("waystone: selected row is malformed")?)
}

fn line_number_from_optional_row(row: Option<&str>) -> Result<usize> {
    selected_line_number(row.ok_or("waystone: selected row is malformed")?)
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn parses_expected_output_from_interactive_and_filter_modes() {
        assert_eq!(
            split_expected_output("\n1\tLabel\t/path\tcreated\tnote"),
            ("", Some("1\tLabel\t/path\tcreated\tnote"))
        );
        assert_eq!(
            split_expected_output("d\n1\tLabel\t/path\tcreated\tnote"),
            ("d", Some("1\tLabel\t/path\tcreated\tnote"))
        );
        assert_eq!(split_expected_output("n"), ("n", None));
        assert_eq!(
            split_expected_output("1\tLabel\t/path\tcreated\tnote"),
            ("", Some("1\tLabel\t/path\tcreated\tnote"))
        );
    }

    #[test]
    fn parses_search_escape_with_query() {
        assert_eq!(
            picker_action_from_search_output("lab\nesc\n1\tLabel\t/path\tcreated\tnote").unwrap(),
            WaystonePickerAction::BackToSelection("lab".to_string())
        );
        assert_eq!(
            picker_action_from_search_output("lab\n1\tLabel\t/path\tcreated\tnote").unwrap(),
            WaystonePickerAction::Open("/path".to_string())
        );
    }
}
