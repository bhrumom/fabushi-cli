use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use std::io;
use unicode_width::UnicodeWidthChar;

pub fn enable_terminal_modes() -> io::Result<()> {
    terminal::enable_raw_mode()
}

pub fn restore_terminal_modes() -> io::Result<()> {
    terminal::disable_raw_mode()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerAction {
    None,
    Submitted(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerInput {
    text: String,
    cursor: usize,
    placeholder: String,
    hints: Vec<(String, String)>,
}

impl ComposerInput {
    pub fn new_with_placeholder(placeholder: String) -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            placeholder,
            hints: Vec::new(),
        }
    }

    pub fn set_slash_commands_enabled(&mut self, _enabled: bool) {}

    pub fn set_hint_items(&mut self, items: Vec<(&'static str, &'static str)>) {
        self.hints = items
            .into_iter()
            .map(|(key, label)| (key.to_string(), label.to_string()))
            .collect();
    }

    pub fn flush_paste_burst_if_due(&mut self) -> bool {
        false
    }

    pub fn handle_paste(&mut self, text: String) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.insert_text(&normalized);
    }

    pub fn text(&self) -> String {
        self.text.clone()
    }

    pub fn replace_text(&mut self, text: String) {
        self.cursor = text.chars().count();
        self.text = text;
    }

    pub fn input(&mut self, key: KeyEvent) -> ComposerAction {
        match key.code {
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.insert_text("\n");
            }
            KeyCode::Enter => {
                let submitted = std::mem::take(&mut self.text);
                self.cursor = 0;
                return ComposerAction::Submitted(submitted);
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) =>
            {
                self.insert_text(&character.to_string());
            }
            KeyCode::Backspace => self.remove_before_cursor(),
            KeyCode::Delete => self.remove_at_cursor(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(self.text.chars().count());
            }
            KeyCode::Home => self.move_to_line_start(),
            KeyCode::End => self.move_to_line_end(),
            _ => {}
        }
        ComposerAction::None
    }

    pub fn desired_height(&self, width: u16) -> u16 {
        let content_width = width.saturating_sub(2).max(1);
        let (row, _) = visual_position(&self.text, self.text.chars().count(), content_width);
        row.saturating_add(1).saturating_add(3)
    }

    pub fn render_ref(&self, area: Rect, buffer: &mut Buffer) {
        if area.width < 2 || area.height < 2 {
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        block.render(area, buffer);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let reserve_hint = (!self.hints.is_empty() && inner.height > 1) as u16;
        let text_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.saturating_sub(reserve_hint),
        };
        if text_area.height > 0 {
            if self.text.is_empty() {
                Paragraph::new(Line::from(Span::styled(
                    self.placeholder.clone(),
                    Style::default().fg(Color::DarkGray),
                )))
                .wrap(Wrap { trim: false })
                .render(text_area, buffer);
            } else {
                Paragraph::new(Text::from(self.text.clone()))
                    .wrap(Wrap { trim: false })
                    .render(text_area, buffer);
            }
        }

        if reserve_hint == 1 {
            let mut spans = Vec::new();
            for (index, (key, label)) in self.hints.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::raw("   "));
                }
                spans.push(Span::styled(key.clone(), Style::default().fg(Color::Cyan)));
                spans.push(Span::styled(
                    format!(" {label}"),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            Paragraph::new(Line::from(spans)).render(
                Rect {
                    x: inner.x,
                    y: inner.y + inner.height - 1,
                    width: inner.width,
                    height: 1,
                },
                buffer,
            );
        }
    }

    pub fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        if area.width < 3 || area.height < 3 {
            return None;
        }
        let width = area.width.saturating_sub(2).max(1);
        let text_height = area.height.saturating_sub(3).max(1);
        let (row, column) = visual_position(&self.text, self.cursor, width);
        let row = row.min(text_height.saturating_sub(1));
        let column = column.min(width.saturating_sub(1));
        Some((area.x + 1 + column, area.y + 1 + row))
    }

    fn insert_text(&mut self, value: &str) {
        let byte = byte_index(&self.text, self.cursor);
        self.text.insert_str(byte, value);
        self.cursor += value.chars().count();
    }

    fn remove_before_cursor(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = byte_index(&self.text, self.cursor - 1);
        let end = byte_index(&self.text, self.cursor);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
    }

    fn remove_at_cursor(&mut self) {
        if self.cursor >= self.text.chars().count() {
            return;
        }
        let start = byte_index(&self.text, self.cursor);
        let end = byte_index(&self.text, self.cursor + 1);
        self.text.replace_range(start..end, "");
    }

    fn move_to_line_start(&mut self) {
        let characters = self.text.chars().collect::<Vec<_>>();
        self.cursor = characters[..self.cursor]
            .iter()
            .rposition(|character| *character == '\n')
            .map(|index| index + 1)
            .unwrap_or(0);
    }

    fn move_to_line_end(&mut self) {
        let characters = self.text.chars().collect::<Vec<_>>();
        self.cursor = characters[self.cursor..]
            .iter()
            .position(|character| *character == '\n')
            .map(|offset| self.cursor + offset)
            .unwrap_or(characters.len());
    }
}

fn byte_index(text: &str, character_index: usize) -> usize {
    text.char_indices()
        .nth(character_index)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn visual_position(text: &str, cursor: usize, width: u16) -> (u16, u16) {
    let width = width.max(1);
    let mut row = 0u16;
    let mut column = 0u16;
    for character in text.chars().take(cursor) {
        if character == '\n' {
            row = row.saturating_add(1);
            column = 0;
            continue;
        }
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0) as u16;
        if character_width == 0 {
            continue;
        }
        if column.saturating_add(character_width) > width {
            row = row.saturating_add(1);
            column = 0;
        }
        column = column.saturating_add(character_width);
        if column >= width {
            row = row.saturating_add(1);
            column = 0;
        }
    }
    (row, column)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new_with_kind(code, modifiers, KeyEventKind::Press)
    }

    #[test]
    fn composer_edits_unicode_and_submits() {
        let mut composer = ComposerInput::new_with_placeholder("prompt".into());
        composer.input(key(KeyCode::Char('法'), KeyModifiers::NONE));
        composer.input(key(KeyCode::Char('布'), KeyModifiers::NONE));
        composer.input(key(KeyCode::Left, KeyModifiers::NONE));
        composer.input(key(KeyCode::Char('施'), KeyModifiers::NONE));
        assert_eq!(composer.text(), "法施布");
        assert_eq!(
            composer.input(key(KeyCode::Enter, KeyModifiers::NONE)),
            ComposerAction::Submitted("法施布".into())
        );
        assert!(composer.text().is_empty());
    }

    #[test]
    fn shift_enter_preserves_multiline_input() {
        let mut composer = ComposerInput::new_with_placeholder("prompt".into());
        composer.input(key(KeyCode::Char('a'), KeyModifiers::NONE));
        composer.input(key(KeyCode::Enter, KeyModifiers::SHIFT));
        composer.input(key(KeyCode::Char('b'), KeyModifiers::NONE));
        assert_eq!(composer.text(), "a\nb");
        assert!(composer.desired_height(20) >= 5);
    }

    #[test]
    fn paste_normalizes_platform_newlines() {
        let mut composer = ComposerInput::new_with_placeholder("prompt".into());
        composer.handle_paste("a\r\nb\rc".into());
        assert_eq!(composer.text(), "a\nb\nc");
    }
}
