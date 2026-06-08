//! Minimal crossterm checkbox multiselect for `pc install`.
//! ↑/↓ move, space toggles, a toggles all, enter confirms, q/esc cancels.

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute, terminal,
};
use std::io::{self, Write};

pub struct Item {
    pub label: String,
    pub hint: String,
    pub checked: bool,
}

/// Returns the indices the user confirmed, or `None` if they cancelled.
pub fn multiselect(prompt: &str, mut items: Vec<Item>) -> Option<Vec<usize>> {
    if items.is_empty() {
        return Some(vec![]);
    }
    let mut stdout = io::stdout();
    let mut cursor_idx = 0usize;

    terminal::enable_raw_mode().ok()?;
    let _ = execute!(stdout, cursor::Hide);

    let result = loop {
        render(&mut stdout, prompt, &items, cursor_idx);
        let ev = match event::read() {
            Ok(e) => e,
            Err(_) => break None,
        };
        if let Event::Key(k) = ev {
            match (k.code, k.modifiers) {
                (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                    cursor_idx = cursor_idx.checked_sub(1).unwrap_or(items.len() - 1);
                }
                (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                    cursor_idx = (cursor_idx + 1) % items.len();
                }
                (KeyCode::Char(' '), _) => {
                    items[cursor_idx].checked = !items[cursor_idx].checked;
                }
                (KeyCode::Char('a'), _) => {
                    let all = items.iter().all(|i| i.checked);
                    for i in items.iter_mut() {
                        i.checked = !all;
                    }
                }
                (KeyCode::Enter, _) => {
                    break Some(
                        items
                            .iter()
                            .enumerate()
                            .filter(|(_, i)| i.checked)
                            .map(|(idx, _)| idx)
                            .collect(),
                    );
                }
                (KeyCode::Esc, _) | (KeyCode::Char('q'), _) => break None,
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => break None,
                _ => {}
            }
        }
    };

    // Clear the rendered block.
    let lines = items.len() as u16 + 2;
    let _ = execute!(
        stdout,
        cursor::MoveToColumn(0),
        terminal::Clear(terminal::ClearType::FromCursorDown),
        cursor::Show
    );
    let _ = lines;
    let _ = terminal::disable_raw_mode();
    result
}

fn render(stdout: &mut io::Stdout, prompt: &str, items: &[Item], cursor_idx: usize) {
    // Move to start of our block and clear downward, then redraw.
    let _ = execute!(
        stdout,
        cursor::MoveToColumn(0),
        terminal::Clear(terminal::ClearType::FromCursorDown)
    );
    let _ = write!(stdout, "{}\r\n", prompt);
    for (i, item) in items.iter().enumerate() {
        let pointer = if i == cursor_idx { ">" } else { " " };
        let box_ = if item.checked { "[x]" } else { "[ ]" };
        let _ = write!(stdout, "{} {} {}  {}\r\n", pointer, box_, item.label, item.hint);
    }
    let _ = write!(
        stdout,
        "  (↑/↓ move · space toggle · a all · enter confirm · q cancel)\r"
    );
    // Move cursor back up to the top of the block for the next redraw.
    let up = items.len() as u16 + 1;
    let _ = execute!(stdout, cursor::MoveUp(up));
    let _ = stdout.flush();
}
