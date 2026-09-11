use std::io::{self, Stderr, Write};

use crossterm::cursor::{MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::style::{Attribute, Print, SetAttribute};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue};

use crate::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EntryFormResult {
    pub(crate) label: String,
    pub(crate) path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Label,
    Path,
}

impl Field {
    fn next(self) -> Self {
        match self {
            Self::Label => Self::Path,
            Self::Path => Self::Label,
        }
    }
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter(output: &mut Stderr) -> Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(output, EnterAlternateScreen, Show)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut output = io::stderr();
        let _ = execute!(output, Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

pub(crate) fn entry_form(
    title: &str,
    initial_label: &str,
    initial_path: &str,
    path_required: bool,
) -> Result<Option<EntryFormResult>> {
    let mut output = io::stderr();
    let _guard = TerminalGuard::enter(&mut output)?;
    let mut label = initial_label.to_string();
    let mut path = initial_path.to_string();
    let mut label_cursor = char_len(&label);
    let mut path_cursor = char_len(&path);
    let mut active = if initial_path.is_empty() {
        Field::Path
    } else {
        Field::Label
    };
    let mut message = String::new();

    loop {
        draw(
            &mut output,
            title,
            &label,
            label_cursor,
            &path,
            path_cursor,
            active,
            &message,
        )?;
        match event::read()? {
            Event::Key(key) if is_cancel(key) => return Ok(None),
            Event::Key(key) if is_submit(key) => {
                if path_required && path.trim().is_empty() {
                    message = "Path is required".to_string();
                    active = Field::Path;
                    continue;
                }
                return Ok(Some(EntryFormResult { label, path }));
            }
            Event::Key(key) => handle_key(
                key,
                &mut active,
                &mut label,
                &mut label_cursor,
                &mut path,
                &mut path_cursor,
                &mut message,
            ),
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
}

fn is_cancel(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Esc)
        || (key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')))
}

fn is_submit(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Enter)
}

fn handle_key(
    key: KeyEvent,
    active: &mut Field,
    label: &mut String,
    label_cursor: &mut usize,
    path: &mut String,
    path_cursor: &mut usize,
    message: &mut String,
) {
    match key.code {
        KeyCode::Tab | KeyCode::BackTab => *active = active.next(),
        KeyCode::Backspace => {
            with_active_text(
                *active,
                label,
                label_cursor,
                path,
                path_cursor,
                delete_before_cursor,
            );
            message.clear();
        }
        KeyCode::Delete => {
            with_active_text(
                *active,
                label,
                label_cursor,
                path,
                path_cursor,
                delete_at_cursor,
            );
            message.clear();
        }
        KeyCode::Left => with_active_cursor(*active, label_cursor, path_cursor, |cursor| {
            *cursor = cursor.saturating_sub(1);
        }),
        KeyCode::Right => with_active_text(
            *active,
            label,
            label_cursor,
            path,
            path_cursor,
            |text, cursor| {
                *cursor = (*cursor + 1).min(char_len(text));
            },
        ),
        KeyCode::Home => {
            with_active_cursor(*active, label_cursor, path_cursor, |cursor| *cursor = 0)
        }
        KeyCode::End => with_active_text(
            *active,
            label,
            label_cursor,
            path,
            path_cursor,
            |text, cursor| {
                *cursor = char_len(text);
            },
        ),
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            with_active_text(
                *active,
                label,
                label_cursor,
                path,
                path_cursor,
                |text, cursor| {
                    insert_at_cursor(text, cursor, ch);
                },
            );
            message.clear();
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            with_active_text(
                *active,
                label,
                label_cursor,
                path,
                path_cursor,
                |text, cursor| {
                    text.clear();
                    *cursor = 0;
                },
            );
            message.clear();
        }
        _ => {}
    }
}

fn with_active_text(
    active: Field,
    label: &mut String,
    label_cursor: &mut usize,
    path: &mut String,
    path_cursor: &mut usize,
    operation: impl FnOnce(&mut String, &mut usize),
) {
    match active {
        Field::Label => operation(label, label_cursor),
        Field::Path => operation(path, path_cursor),
    }
}

fn with_active_cursor(
    active: Field,
    label_cursor: &mut usize,
    path_cursor: &mut usize,
    operation: impl FnOnce(&mut usize),
) {
    match active {
        Field::Label => operation(label_cursor),
        Field::Path => operation(path_cursor),
    }
}

fn draw(
    output: &mut Stderr,
    title: &str,
    label: &str,
    label_cursor: usize,
    path: &str,
    path_cursor: usize,
    active: Field,
    message: &str,
) -> Result<()> {
    let (cols, rows) = terminal::size()?;
    let width = cols.saturating_sub(8).clamp(40, 100);
    let left = cols.saturating_sub(width) / 2;
    let top = rows.saturating_sub(12) / 2;
    let inner_width = width.saturating_sub(4) as usize;

    queue!(output, Clear(ClearType::All), MoveTo(left, top))?;
    queue!(
        output,
        Print(format!(
            "┌{}┐",
            "─".repeat(width.saturating_sub(2) as usize)
        ))
    )?;
    draw_line(output, left, top + 1, width, title, true)?;
    draw_line(output, left, top + 2, width, "", false)?;
    draw_field(
        output,
        left,
        top + 3,
        width,
        "Label",
        label,
        label_cursor,
        active == Field::Label,
    )?;
    draw_field(
        output,
        left,
        top + 5,
        width,
        "Path",
        path,
        path_cursor,
        active == Field::Path,
    )?;
    draw_line(output, left, top + 7, width, message, false)?;
    draw_line(
        output,
        left,
        top + 8,
        width,
        "Tab switch · ←/→ move · Del/Bksp edit · Enter save · Esc cancel",
        false,
    )?;
    queue!(
        output,
        MoveTo(left, top + 9),
        Print(format!(
            "└{}┘",
            "─".repeat(width.saturating_sub(2) as usize)
        ))
    )?;

    let active_cursor = match active {
        Field::Label => label_cursor,
        Field::Path => path_cursor,
    };
    let active_width = inner_width.saturating_sub(active_input_offset(active) as usize);
    let (_, cursor_col) = visible_value(
        active_value(active, label, path),
        active_cursor,
        active_width,
    );
    let cursor_x = left + 2 + active_input_offset(active) + cursor_col as u16;
    let cursor_y = match active {
        Field::Label => top + 3,
        Field::Path => top + 5,
    };
    queue!(output, MoveTo(cursor_x.min(left + width - 2), cursor_y))?;
    output.flush()?;

    let _ = inner_width;
    Ok(())
}

fn draw_line(
    output: &mut Stderr,
    left: u16,
    row: u16,
    width: u16,
    text: &str,
    bold: bool,
) -> Result<()> {
    let inner_width = width.saturating_sub(4) as usize;
    let rendered = truncate(text, inner_width);
    queue!(output, MoveTo(left, row), Print("│ "))?;
    if bold {
        queue!(output, SetAttribute(Attribute::Bold))?;
    }
    queue!(output, Print(format!("{rendered:<inner_width$}")))?;
    if bold {
        queue!(output, SetAttribute(Attribute::Reset))?;
    }
    queue!(output, Print(" │"))?;
    Ok(())
}

fn draw_field(
    output: &mut Stderr,
    left: u16,
    row: u16,
    width: u16,
    name: &str,
    value: &str,
    cursor: usize,
    active: bool,
) -> Result<()> {
    let inner_width = width.saturating_sub(4) as usize;
    let prefix = format!("{name}: ");
    let value_width = inner_width.saturating_sub(prefix.len());
    let (rendered, _) = visible_value(value, cursor, value_width);
    queue!(output, MoveTo(left, row), Print("│ "))?;
    if active {
        queue!(output, SetAttribute(Attribute::Reverse))?;
    }
    queue!(
        output,
        Print(prefix),
        Print(format!("{rendered:<value_width$}"))
    )?;
    if active {
        queue!(output, SetAttribute(Attribute::Reset))?;
    }
    queue!(output, Print(" │"))?;
    Ok(())
}

fn active_input_offset(active: Field) -> u16 {
    match active {
        Field::Label => "Label: ".len() as u16,
        Field::Path => "Path: ".len() as u16,
    }
}

fn active_value<'a>(active: Field, label: &'a str, path: &'a str) -> &'a str {
    match active {
        Field::Label => label,
        Field::Path => path,
    }
}

fn truncate(value: &str, width: usize) -> String {
    let mut rendered: String = value.chars().take(width).collect();
    if value.chars().count() > width && width > 0 {
        rendered.pop();
        rendered.push('…');
    }
    rendered
}

fn visible_value(value: &str, cursor: usize, width: usize) -> (String, usize) {
    let length = char_len(value);
    let cursor = cursor.min(length);
    let start = if cursor >= width && width > 0 {
        cursor - width + 1
    } else {
        0
    };
    let rendered: String = value.chars().skip(start).take(width).collect();
    (rendered, cursor.saturating_sub(start))
}

fn char_len(value: &str) -> usize {
    value.chars().count()
}

fn byte_index(value: &str, char_index: usize) -> usize {
    value
        .char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or(value.len())
}

fn insert_at_cursor(value: &mut String, cursor: &mut usize, ch: char) {
    let idx = byte_index(value, *cursor);
    value.insert(idx, ch);
    *cursor += 1;
}

fn delete_before_cursor(value: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    let start = byte_index(value, *cursor - 1);
    let end = byte_index(value, *cursor);
    value.replace_range(start..end, "");
    *cursor -= 1;
}

fn delete_at_cursor(value: &mut String, cursor: &mut usize) {
    if *cursor >= char_len(value) {
        return;
    }
    let start = byte_index(value, *cursor);
    let end = byte_index(value, *cursor + 1);
    value.replace_range(start..end, "");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_text_at_cursor() {
        let mut value = "abcd".to_string();
        let mut cursor = 2;
        insert_at_cursor(&mut value, &mut cursor, 'X');
        assert_eq!(value, "abXcd");
        assert_eq!(cursor, 3);

        delete_before_cursor(&mut value, &mut cursor);
        assert_eq!(value, "abcd");
        assert_eq!(cursor, 2);

        delete_at_cursor(&mut value, &mut cursor);
        assert_eq!(value, "abd");
        assert_eq!(cursor, 2);
    }

    #[test]
    fn renders_visible_value_around_cursor() {
        assert_eq!(visible_value("abcdef", 2, 4), ("abcd".to_string(), 2));
        assert_eq!(visible_value("abcdef", 5, 4), ("cdef".to_string(), 3));
    }
}
