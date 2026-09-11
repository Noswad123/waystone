use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::app::{App, WaystonePickerAction, WaystonePickerMode};
use crate::form::entry_form;
use crate::process::{command_resolves, notify, pipe_to, prompt, run_capture};
use crate::record::{field, selected_line_number};
use crate::time::capture_stamp;
use crate::Result;

impl App {
    fn cmd_add(&self, args: &[String]) -> Result<()> {
        if args.is_empty() {
            eprint!("{}", self.usage());
            return Err("waystone: add requires a path".into());
        }
        let path = self.resolve_path(&args[0])?;
        let label = args
            .get(1)
            .cloned()
            .unwrap_or_else(|| path.display().to_string());
        self.append_waystone(&path, &label, "")?;
        println!("Marked: {} -> {}", label, path.display());
        Ok(())
    }

    fn new_entry_form(&self) -> Result<Option<String>> {
        let Some(form) = entry_form("New waystone", "", "", true)? else {
            return Ok(None);
        };
        let path = self.resolve_path(form.path.trim())?;
        let label = if form.label.trim().is_empty() {
            path.display().to_string()
        } else {
            form.label
        };
        self.append_waystone(&path, &label, "")?;
        Ok(Some(format!("Marked: {} -> {}", label, path.display())))
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

    fn cmd_select(&self, args: &[String]) -> Result<()> {
        match args.first().map(String::as_str) {
            Some("--action") => {
                if let Some((action, path)) = self.run_editor_action_picker()? {
                    println!("{action}\t{path}");
                }
                Ok(())
            }
            Some("-h" | "--help") => {
                eprintln!("Usage: {} select [--action]", self.command_name);
                Ok(())
            }
            Some(other) => Err(format!("waystone: unknown select option: {other}").into()),
            None => {
                println!("{}", self.select_waystone_path()?);
                Ok(())
            }
        }
    }

    fn cmd_open(&self, args: &[String]) -> Result<()> {
        let Some(path) = self.run_open_picker()? else {
            return Ok(());
        };
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

    fn run_open_picker(&self) -> Result<Option<String>> {
        let mut mode = WaystonePickerMode::Select;
        let mut filtered_rows: Option<String> = None;
        loop {
            match self.select_waystone_action(mode, filtered_rows.as_deref())? {
                WaystonePickerAction::Open(path) => return Ok(Some(path)),
                WaystonePickerAction::Search => {
                    filtered_rows = None;
                    mode = WaystonePickerMode::Search;
                }
                WaystonePickerAction::BackToSelection(query) => {
                    let matches = self.filtered_rows(&query)?.join("\n");
                    filtered_rows = if matches.is_empty() {
                        None
                    } else {
                        Some(matches)
                    };
                    mode = WaystonePickerMode::Select;
                }
                WaystonePickerAction::EditAndExit(path) => {
                    self.edit_target(&path)?;
                    return Ok(None);
                }
                WaystonePickerAction::EditAndReturn(path) => {
                    self.edit_target(&path)?;
                    filtered_rows = None;
                    mode = WaystonePickerMode::Select;
                }
                WaystonePickerAction::Delete(line_number) => {
                    self.remove_line_number(line_number)?;
                    println!("Removed waystone {line_number}");
                    filtered_rows = None;
                    mode = WaystonePickerMode::Select;
                }
                WaystonePickerAction::Rename(line_number) => {
                    if let Some(message) = self.rename_line_number_form(line_number)? {
                        println!("{message}");
                    }
                    filtered_rows = None;
                    mode = WaystonePickerMode::Select;
                }
                WaystonePickerAction::NewEntry => {
                    if let Some(message) = self.new_entry_form()? {
                        println!("{message}");
                    }
                    filtered_rows = None;
                    mode = WaystonePickerMode::Select;
                }
                WaystonePickerAction::Cancel => return Err("waystone: selection cancelled".into()),
            }
        }
    }

    fn run_editor_action_picker(&self) -> Result<Option<(&'static str, String)>> {
        let mut mode = WaystonePickerMode::Select;
        let mut filtered_rows: Option<String> = None;
        loop {
            match self.select_waystone_action(mode, filtered_rows.as_deref())? {
                WaystonePickerAction::Open(path) => return Ok(Some(("open", path))),
                WaystonePickerAction::Search => {
                    filtered_rows = None;
                    mode = WaystonePickerMode::Search;
                }
                WaystonePickerAction::BackToSelection(query) => {
                    let matches = self.filtered_rows(&query)?.join("\n");
                    filtered_rows = if matches.is_empty() {
                        None
                    } else {
                        Some(matches)
                    };
                    mode = WaystonePickerMode::Select;
                }
                WaystonePickerAction::EditAndExit(path) => return Ok(Some(("edit", path))),
                WaystonePickerAction::EditAndReturn(path) => {
                    return Ok(Some(("edit-return", path)))
                }
                WaystonePickerAction::Delete(line_number) => {
                    self.remove_line_number(line_number)?;
                    eprintln!("Removed waystone {line_number}");
                    filtered_rows = None;
                    mode = WaystonePickerMode::Select;
                }
                WaystonePickerAction::Rename(line_number) => {
                    if let Some(message) = self.rename_line_number_form(line_number)? {
                        eprintln!("{message}");
                    }
                    filtered_rows = None;
                    mode = WaystonePickerMode::Select;
                }
                WaystonePickerAction::NewEntry => {
                    if let Some(message) = self.new_entry_form()? {
                        eprintln!("{message}");
                    }
                    filtered_rows = None;
                    mode = WaystonePickerMode::Select;
                }
                WaystonePickerAction::Cancel => return Ok(None),
            }
        }
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

    fn remove_line_number(&self, line_number: usize) -> Result<()> {
        let mut records = self.read_records()?;
        if line_number == 0 || line_number > records.len() {
            return Err("waystone: selected row no longer exists".into());
        }
        records.remove(line_number - 1);
        self.write_records(&records)
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
        self.edit_target(&self.file.display().to_string())
    }

    fn edit_target(&self, target: &str) -> Result<()> {
        let shell = env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
        let status = Command::new(shell)
            .arg("-c")
            .arg("exec ${EDITOR:-nvim} \"$1\"")
            .arg("waystone-editor")
            .arg(target)
            .status()?;
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

    fn rename_line_number_form(&self, line_number: usize) -> Result<Option<String>> {
        let records = self.read_records()?;
        let record = records
            .get(line_number - 1)
            .ok_or("waystone: selected row no longer exists")?;
        let Some(form) = entry_form("Rename waystone", &record.label, &record.path, true)? else {
            return Ok(None);
        };

        let mut records = records;
        let record = records
            .get_mut(line_number - 1)
            .ok_or("waystone: selected row no longer exists")?;
        record.label = form.label;
        record.path = self.resolve_path(form.path.trim())?.display().to_string();
        self.write_records(&records)?;
        Ok(Some(format!("Renamed waystone {line_number}")))
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

    pub(crate) fn dispatch(&self, args: &[String]) -> Result<()> {
        let command = args.first().map(String::as_str).unwrap_or("");
        match command {
            "" => self.cmd_open(&[]),
            "add" => self.cmd_add(&args[1..]),
            "capture" => self.cmd_capture(&args[1..]),
            "import" => self.cmd_import(),
            "list" => self.cmd_list(),
            "pick" => self.cmd_pick(),
            "select" => self.cmd_select(&args[1..]),
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
