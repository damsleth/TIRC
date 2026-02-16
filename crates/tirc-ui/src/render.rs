//! Overlay and popup rendering functions.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use tirc_config::Theme;

use crate::palette::PaletteEngine;

/// Selection editor state for theme/keymap pickers.
#[derive(Debug, Clone, Default)]
pub struct SelectionEditor {
    pub open: bool,
    pub options: Vec<String>,
    pub selected: usize,
    pub active: String,
}

impl SelectionEditor {
    pub fn with_options(options: Vec<String>, active: String) -> Self {
        let selected = options
            .iter()
            .position(|option| option == &active)
            .unwrap_or(0);
        Self {
            open: false,
            options,
            selected,
            active,
        }
    }

    pub fn set_options(&mut self, options: Vec<String>, active: String) {
        self.options = options;
        self.active = active.clone();
        self.selected = self
            .options
            .iter()
            .position(|option| option == &active)
            .unwrap_or(0);
    }

    pub fn move_up(&mut self) {
        if self.options.is_empty() {
            return;
        }
        self.selected = self
            .selected
            .checked_sub(1)
            .unwrap_or(self.options.len().saturating_sub(1));
    }

    pub fn move_down(&mut self) {
        if self.options.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.options.len();
    }

    pub fn selected_value(&self) -> Option<String> {
        self.options.get(self.selected).cloned()
    }
}

/// Palette mode for join/msg workflows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PaletteMode {
    #[default]
    Normal,
    JoinProfile,
    JoinChannel {
        profile: String,
    },
    MsgProfile,
}

/// Render the help overlay popup.
pub fn render_help_overlay(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(80, 70, area);
    frame.render_widget(Clear, popup);

    let lines = vec![
        Line::from("TIRC Help"),
        Line::from(""),
        Line::from("Dismiss: Esc, q, or Ctrl-H"),
        Line::from("Ctrl-P: Open command palette"),
        Line::from("Ctrl-H: Toggle help"),
        Line::from("Ctrl-T: Open theme editor"),
        Line::from("Ctrl-Y: Open keymap editor"),
        Line::from("Ctrl-X: Close focused buffer (with confirmation)"),
        Line::from("Ctrl-B: Toggle left sidebar (buffers tree)"),
        Line::from("Ctrl-R: Toggle right sidebar (nicklist)"),
        Line::from("Alt-Left / Alt-Right: Cycle buffers"),
        Line::from("Ctrl-A / Ctrl-E: Start/end of input"),
        Line::from("Ctrl-K / Ctrl-U / Ctrl-W: Kill operations"),
        Line::from(""),
        Line::from("Sidebar: Enter to collapse/expand or switch buffer"),
        Line::from("Mouse: Click to select, scroll in any pane"),
        Line::from("Nicklist: / to filter, Enter to confirm, Esc to clear"),
        Line::from(""),
        Line::from("/join /part /query /msg /connect /disconnect /theme /keymap /quit"),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title("Help").borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        popup,
    );
}

/// Render the command palette popup.
pub fn render_palette(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    palette: &PaletteEngine,
    mode: &PaletteMode,
) {
    let popup = centered_rect(70, 55, area);
    frame.render_widget(Clear, popup);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(3)])
        .split(popup);

    let palette_title = match mode {
        PaletteMode::Normal => "Command Palette",
        PaletteMode::JoinProfile => "Join: Choose Profile",
        PaletteMode::JoinChannel { .. } => "Join: Choose Channel",
        PaletteMode::MsgProfile => "Message: Choose Profile",
    };

    frame.render_widget(
        Paragraph::new(palette.query.clone())
            .block(Block::default().title(palette_title).borders(Borders::ALL)),
        layout[0],
    );

    let items = palette
        .results
        .iter()
        .enumerate()
        .take(12)
        .map(|(index, item)| {
            if index == palette.selected {
                ListItem::new(Line::from(Span::styled(
                    item.clone(),
                    Style::default()
                        .bg(parse_color(&theme.palette.accent))
                        .fg(parse_color(&theme.palette.background))
                        .add_modifier(Modifier::BOLD),
                )))
            } else {
                ListItem::new(item.clone())
            }
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        List::new(items).block(Block::default().title("Actions").borders(Borders::ALL)),
        layout[1],
    );
}

/// Render the selection editor (theme/keymap picker) popup.
pub fn render_selection_editor(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    editor: &SelectionEditor,
    theme: &Theme,
) {
    let popup = centered_rect(60, 60, area);
    frame.render_widget(Clear, popup);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(popup);

    frame.render_widget(
        Paragraph::new(format!(
            "Current: {} | Up/Down select | Enter apply | Esc close",
            editor.active
        ))
        .block(Block::default().title(title).borders(Borders::ALL)),
        layout[0],
    );

    let items = editor
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            let mut label = option.clone();
            if option == &editor.active {
                label.push_str(" (active)");
            }

            if index == editor.selected {
                ListItem::new(Line::from(Span::styled(
                    label,
                    Style::default()
                        .bg(parse_color(&theme.palette.accent))
                        .fg(parse_color(&theme.palette.background))
                        .add_modifier(Modifier::BOLD),
                )))
            } else {
                ListItem::new(label)
            }
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        List::new(items).block(Block::default().title("Available").borders(Borders::ALL)),
        layout[1],
    );

    frame.render_widget(
        Paragraph::new("Changes apply immediately and are persisted to config.toml"),
        layout[2],
    );
}

/// Render the close buffer confirmation popup.
pub fn render_close_confirmation(frame: &mut Frame, area: Rect, no_open_buffers: bool) {
    let popup = centered_rect(46, 20, area);
    frame.render_widget(Clear, popup);

    let (title, message) = if no_open_buffers {
        (
            "No Open Buffers",
            "There are no open buffers. Press Enter or Esc to continue.",
        )
    } else {
        (
            "Confirm Close",
            "Close the currently focused buffer? Press y/Enter to confirm, n/Esc to cancel.",
        )
    };

    frame.render_widget(
        Paragraph::new(message)
            .block(Block::default().title(title).borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        popup,
    );
}

/// Parse a color string (hex or named) into a ratatui Color.
pub fn parse_color(value: &str) -> Color {
    if let Some(hex) = value.strip_prefix('#')
        && hex.len() == 6
        && let Ok(raw) = u32::from_str_radix(hex, 16)
    {
        let r = ((raw >> 16) & 0xff) as u8;
        let g = ((raw >> 8) & 0xff) as u8;
        let b = (raw & 0xff) as u8;
        return Color::Rgb(r, g, b);
    }

    match value.to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" => Color::Gray,
        "darkgray" => Color::DarkGray,
        "white" => Color::White,
        _ => Color::Reset,
    }
}

/// Get border style based on focus state.
pub fn border_style(theme: &Theme, focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(parse_color(&theme.palette.accent))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

/// Get highlight style for focused list items.
pub fn focus_style(theme: &Theme, focused: bool) -> Style {
    if focused {
        Style::default()
            .bg(parse_color(&theme.palette.accent))
            .fg(parse_color(&theme.palette.background))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

/// Calculate a centered rectangle within an area.
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Convert a key event to a canonical string for keymap lookup.
pub fn canonical_key(key: KeyEvent) -> String {
    let mut parts = Vec::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt");
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("shift");
    }

    let code = match key.code {
        KeyCode::Char(c) => c.to_ascii_lowercase().to_string(),
        KeyCode::Enter => "enter".to_owned(),
        KeyCode::Esc => "esc".to_owned(),
        KeyCode::Backspace => "backspace".to_owned(),
        KeyCode::Delete => "delete".to_owned(),
        KeyCode::Left => "left".to_owned(),
        KeyCode::Right => "right".to_owned(),
        KeyCode::Up => "up".to_owned(),
        KeyCode::Down => "down".to_owned(),
        KeyCode::PageUp => "pageup".to_owned(),
        KeyCode::PageDown => "pagedown".to_owned(),
        KeyCode::Home => "home".to_owned(),
        KeyCode::End => "end".to_owned(),
        _ => "unknown".to_owned(),
    };

    if parts.is_empty() {
        code
    } else {
        format!("{}-{code}", parts.join("-"))
    }
}
