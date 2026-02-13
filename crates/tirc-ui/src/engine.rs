use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use tirc_config::{Keymap, Theme};
use tirc_core::{AppViewModel, UiEvent};

use crate::{input::InputEditor, palette::PaletteEngine};

/// Intent emitted by the UI layer into the app reducer.
#[derive(Debug, Clone)]
pub enum UiIntent {
    Event(UiEvent),
    SubmitLine(String),
    ApplyTheme(String),
    ApplyKeymap(String),
}

#[derive(Debug, Clone, Default)]
struct SelectionEditor {
    open: bool,
    options: Vec<String>,
    selected: usize,
    active: String,
}

impl SelectionEditor {
    fn with_options(options: Vec<String>, active: String) -> Self {
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

    fn set_options(&mut self, options: Vec<String>, active: String) {
        self.options = options;
        self.active = active;
        self.selected = self
            .options
            .iter()
            .position(|option| option == &self.active)
            .unwrap_or(0);
    }

    fn move_up(&mut self) {
        if self.options.is_empty() {
            self.selected = 0;
            return;
        }

        if self.selected == 0 {
            self.selected = self.options.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    fn move_down(&mut self) {
        if self.options.is_empty() {
            self.selected = 0;
            return;
        }

        self.selected = (self.selected + 1) % self.options.len();
    }

    fn selected_value(&self) -> Option<String> {
        self.options.get(self.selected).cloned()
    }
}

enum EditorKind {
    Theme,
    Keymap,
}

/// Stateful ratatui engine for rendering and keyboard handling.
pub struct UiEngine {
    theme: Theme,
    keymap_actions: HashMap<String, String>,
    input: InputEditor,
    pub palette_open: bool,
    palette: PaletteEngine,
    theme_editor: SelectionEditor,
    keymap_editor: SelectionEditor,
    help_open: bool,
    last_draw: Instant,
    idle_hz: u8,
}

impl UiEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        theme: Theme,
        keymap: Keymap,
        palette_items: Vec<String>,
        idle_hz: u8,
        theme_options: Vec<String>,
        active_theme: String,
        keymap_options: Vec<String>,
        active_keymap: String,
    ) -> Self {
        let keymap_actions = flatten_keymap(&keymap);

        Self {
            theme,
            keymap_actions,
            input: InputEditor::default(),
            palette_open: false,
            palette: PaletteEngine::new(palette_items),
            theme_editor: SelectionEditor::with_options(theme_options, active_theme),
            keymap_editor: SelectionEditor::with_options(keymap_options, active_keymap),
            help_open: false,
            last_draw: Instant::now(),
            idle_hz,
        }
    }

    pub fn set_theme(&mut self, theme: Theme, name: String) {
        self.theme = theme;
        self.theme_editor.active = name.clone();
        if let Some(pos) = self
            .theme_editor
            .options
            .iter()
            .position(|option| option == &name)
        {
            self.theme_editor.selected = pos;
        }
    }

    pub fn set_keymap(&mut self, keymap: &Keymap, name: String) {
        self.keymap_actions = flatten_keymap(keymap);
        self.keymap_editor.active = name.clone();
        if let Some(pos) = self
            .keymap_editor
            .options
            .iter()
            .position(|option| option == &name)
        {
            self.keymap_editor.selected = pos;
        }
    }

    pub fn set_theme_options(&mut self, options: Vec<String>, active: String) {
        self.theme_editor.set_options(options, active);
    }

    pub fn set_keymap_options(&mut self, options: Vec<String>, active: String) {
        self.keymap_editor.set_options(options, active);
    }

    pub fn set_palette_items(&mut self, items: Vec<String>) {
        self.palette.set_items(items);
    }

    pub fn sync_input(&mut self, value: &str) {
        if self.input.text() != value {
            self.input.set_text(value.to_owned());
        }
    }

    pub fn max_idle_interval(&self) -> Duration {
        Duration::from_millis((1000 / u64::from(self.idle_hz.max(1))).max(16))
    }

    pub fn should_draw_heartbeat(&self) -> bool {
        self.last_draw.elapsed() >= self.max_idle_interval()
    }

    pub fn draw(&mut self, frame: &mut Frame, vm: &AppViewModel) {
        self.last_draw = Instant::now();
        self.help_open = vm.help_open;

        let area = frame.area();
        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(4),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .split(area);

        let top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(28),
                Constraint::Min(30),
                Constraint::Length(24),
            ])
            .split(root[0]);

        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(7), Constraint::Min(4)])
            .split(top[0]);

        let network_items = vm
            .networks
            .iter()
            .map(|network| {
                let icon = if network.connected { "*" } else { "o" };
                ListItem::new(format!("{icon} {}", network.name))
            })
            .collect::<Vec<_>>();

        frame.render_widget(
            List::new(network_items).block(Block::default().title("Servers").borders(Borders::ALL)),
            left[0],
        );

        let buffer_items = vm
            .buffers
            .iter()
            .map(|buffer| {
                let unread = if buffer.unread > 0 {
                    format!(" ({})", buffer.unread)
                } else {
                    String::new()
                };
                ListItem::new(format!(
                    "{}/{} [{}]{}",
                    buffer.id.network, buffer.id.name, buffer.nick_count, unread
                ))
            })
            .collect::<Vec<_>>();

        frame.render_widget(
            List::new(buffer_items).block(Block::default().title("Buffers").borders(Borders::ALL)),
            left[1],
        );

        let chat_title = vm
            .active_buffer
            .as_ref()
            .map(|buffer| format!("{} / {}", buffer.id.network, buffer.id.name))
            .unwrap_or_else(|| "No active buffer".to_owned());

        let chat_lines = vm
            .active_buffer
            .as_ref()
            .map(|buffer| {
                buffer
                    .lines
                    .iter()
                    .map(|line| {
                        let ts = line.ts.format("%H:%M:%S").to_string();
                        let sender = line.sender.clone().unwrap_or_else(|| "*".to_owned());
                        let style = if line.mention {
                            Style::default().fg(parse_color(&self.theme.palette.mention))
                        } else {
                            Style::default().fg(parse_color(&self.theme.palette.foreground))
                        };
                        Line::from(vec![
                            Span::styled(format!("[{ts}] "), Style::default().fg(Color::DarkGray)),
                            Span::styled(format!("<{sender}> "), Style::default().fg(Color::Cyan)),
                            Span::styled(line.body.clone(), style),
                        ])
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        frame.render_widget(
            Paragraph::new(chat_lines)
                .block(Block::default().title(chat_title).borders(Borders::ALL))
                .wrap(Wrap { trim: false }),
            top[1],
        );

        let nick_items = vm
            .active_buffer
            .as_ref()
            .map(|buffer| {
                buffer
                    .nicklist
                    .iter()
                    .map(|nick| ListItem::new(nick.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        frame.render_widget(
            List::new(nick_items).block(Block::default().title("Nicklist").borders(Borders::ALL)),
            top[2],
        );

        let status_style = Style::default()
            .fg(parse_color(&self.theme.palette.background))
            .bg(parse_color(&self.theme.palette.accent))
            .add_modifier(Modifier::BOLD);
        frame.render_widget(
            Paragraph::new(vm.status.clone()).style(status_style),
            root[1],
        );

        let input_block = Block::default().title("Input").borders(Borders::ALL);
        frame.render_widget(
            Paragraph::new(self.input.text().to_owned())
                .style(Style::default().fg(parse_color(&self.theme.palette.foreground)))
                .block(input_block),
            root[2],
        );

        if vm.help_open {
            self.render_help_overlay(frame, area);
        }

        if self.palette_open {
            self.render_palette(frame, area);
        }

        if self.theme_editor.open {
            self.render_selection_editor(frame, area, "Theme Editor", &self.theme_editor);
        }

        if self.keymap_editor.open {
            self.render_selection_editor(frame, area, "Keymap Editor", &self.keymap_editor);
        }
    }

    pub fn handle_terminal_event(&mut self, event: Event) -> Vec<UiIntent> {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Resize(width, height) => {
                vec![UiIntent::Event(UiEvent::Resize { width, height })]
            }
            _ => Vec::new(),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Vec<UiIntent> {
        if key.kind != KeyEventKind::Press {
            return Vec::new();
        }

        if self.help_open {
            match key.code {
                KeyCode::Esc => {
                    return vec![UiIntent::Event(UiEvent::Action("help.close".to_owned()))];
                }
                KeyCode::Char('q') if key.modifiers == KeyModifiers::NONE => {
                    return vec![UiIntent::Event(UiEvent::Action("help.close".to_owned()))];
                }
                _ => {
                    if let Some(action) = self.lookup_action(key)
                        && action == "help.toggle"
                    {
                        return self.handle_action(&action);
                    }
                    return Vec::new();
                }
            }
        }

        if self.palette_open {
            return self.handle_palette_key(key);
        }

        if self.theme_editor.open {
            return self.handle_selection_editor_key(EditorKind::Theme, key);
        }

        if self.keymap_editor.open {
            return self.handle_selection_editor_key(EditorKind::Keymap, key);
        }

        if let Some(action) = self.lookup_action(key) {
            return self.handle_action(&action);
        }

        match key.code {
            KeyCode::Enter => {
                let line = self.input.text().to_owned();
                self.input.clear();
                vec![
                    UiIntent::Event(UiEvent::InputChanged(String::new())),
                    UiIntent::SubmitLine(line),
                ]
            }
            KeyCode::Char(c) if key.modifiers == KeyModifiers::NONE => {
                self.input.insert_char(c);
                vec![UiIntent::Event(UiEvent::InputChanged(
                    self.input.text().to_owned(),
                ))]
            }
            KeyCode::Backspace => {
                self.input.backspace();
                vec![UiIntent::Event(UiEvent::InputChanged(
                    self.input.text().to_owned(),
                ))]
            }
            KeyCode::Delete => {
                self.input.delete();
                vec![UiIntent::Event(UiEvent::InputChanged(
                    self.input.text().to_owned(),
                ))]
            }
            KeyCode::Left => {
                self.input.move_left();
                Vec::new()
            }
            KeyCode::Right => {
                self.input.move_right();
                Vec::new()
            }
            KeyCode::Home => {
                self.input.move_home();
                Vec::new()
            }
            KeyCode::End => {
                self.input.move_end();
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn handle_palette_key(&mut self, key: KeyEvent) -> Vec<UiIntent> {
        match key.code {
            KeyCode::Esc => {
                self.palette_open = false;
                self.palette.clear_query();
                vec![UiIntent::Event(UiEvent::Action(
                    "palette.toggle".to_owned(),
                ))]
            }
            KeyCode::Up => {
                self.palette.move_selection_up();
                Vec::new()
            }
            KeyCode::Down => {
                self.palette.move_selection_down();
                Vec::new()
            }
            KeyCode::Enter => {
                if let Some(selected) = self.palette.selected_item() {
                    self.palette_open = false;
                    vec![UiIntent::Event(UiEvent::PaletteExecute(
                        selected.to_owned(),
                    ))]
                } else {
                    Vec::new()
                }
            }
            KeyCode::Backspace => {
                self.palette.query_backspace();
                vec![UiIntent::Event(UiEvent::PaletteQuery(
                    self.palette.query.clone(),
                ))]
            }
            KeyCode::Char(c) if key.modifiers == KeyModifiers::NONE => {
                self.palette.query_push(c);
                vec![UiIntent::Event(UiEvent::PaletteQuery(
                    self.palette.query.clone(),
                ))]
            }
            _ => Vec::new(),
        }
    }

    fn handle_selection_editor_key(&mut self, kind: EditorKind, key: KeyEvent) -> Vec<UiIntent> {
        let editor = match kind {
            EditorKind::Theme => &mut self.theme_editor,
            EditorKind::Keymap => &mut self.keymap_editor,
        };

        match key.code {
            KeyCode::Esc => {
                editor.open = false;
                Vec::new()
            }
            KeyCode::Up => {
                editor.move_up();
                Vec::new()
            }
            KeyCode::Down => {
                editor.move_down();
                Vec::new()
            }
            KeyCode::Enter => {
                editor.open = false;
                if let Some(selected) = editor.selected_value() {
                    match kind {
                        EditorKind::Theme => vec![UiIntent::ApplyTheme(selected)],
                        EditorKind::Keymap => vec![UiIntent::ApplyKeymap(selected)],
                    }
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    fn handle_action(&mut self, action: &str) -> Vec<UiIntent> {
        match action {
            "palette.toggle" => {
                self.palette_open = !self.palette_open;
                if self.palette_open {
                    self.palette.clear_query();
                }
                self.theme_editor.open = false;
                self.keymap_editor.open = false;
                vec![UiIntent::Event(UiEvent::Action(action.to_owned()))]
            }
            "help.toggle" => vec![UiIntent::Event(UiEvent::Action(action.to_owned()))],
            "theme.editor.toggle" => {
                self.theme_editor.open = !self.theme_editor.open;
                self.keymap_editor.open = false;
                self.palette_open = false;
                vec![UiIntent::Event(UiEvent::Action(action.to_owned()))]
            }
            "keymap.editor.toggle" => {
                self.keymap_editor.open = !self.keymap_editor.open;
                self.theme_editor.open = false;
                self.palette_open = false;
                vec![UiIntent::Event(UiEvent::Action(action.to_owned()))]
            }
            "input.home" => {
                self.input.move_home();
                Vec::new()
            }
            "input.end" => {
                self.input.move_end();
                Vec::new()
            }
            "input.kill_to_end" => {
                self.input.kill_to_end();
                vec![UiIntent::Event(UiEvent::InputChanged(
                    self.input.text().to_owned(),
                ))]
            }
            "input.kill_to_start" => {
                self.input.kill_to_start();
                vec![UiIntent::Event(UiEvent::InputChanged(
                    self.input.text().to_owned(),
                ))]
            }
            "input.kill_word" => {
                self.input.kill_word();
                vec![UiIntent::Event(UiEvent::InputChanged(
                    self.input.text().to_owned(),
                ))]
            }
            _ => vec![UiIntent::Event(UiEvent::Action(action.to_owned()))],
        }
    }

    fn lookup_action(&self, key: KeyEvent) -> Option<String> {
        let canonical = canonical_key(key);
        self.keymap_actions.get(&canonical).cloned()
    }

    fn render_help_overlay(&self, frame: &mut Frame, area: Rect) {
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
            Line::from("Alt-Left / Alt-Right: Cycle buffers"),
            Line::from("Ctrl-A / Ctrl-E: Start/end of input"),
            Line::from("Ctrl-K / Ctrl-U / Ctrl-W: Kill operations"),
            Line::from("/join /part /msg /connect /disconnect /theme /keymap /quit"),
        ];

        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::default().title("Help").borders(Borders::ALL))
                .wrap(Wrap { trim: true }),
            popup,
        );
    }

    fn render_palette(&self, frame: &mut Frame, area: Rect) {
        let popup = centered_rect(70, 55, area);
        frame.render_widget(Clear, popup);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(popup);

        frame.render_widget(
            Paragraph::new(self.palette.query.clone()).block(
                Block::default()
                    .title("Command Palette")
                    .borders(Borders::ALL),
            ),
            layout[0],
        );

        let items = self
            .palette
            .results
            .iter()
            .enumerate()
            .take(12)
            .map(|(index, item)| {
                if index == self.palette.selected {
                    ListItem::new(Line::from(Span::styled(
                        item.clone(),
                        Style::default()
                            .bg(parse_color(&self.theme.palette.accent))
                            .fg(parse_color(&self.theme.palette.background))
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

    fn render_selection_editor(
        &self,
        frame: &mut Frame,
        area: Rect,
        title: &str,
        editor: &SelectionEditor,
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
                            .bg(parse_color(&self.theme.palette.accent))
                            .fg(parse_color(&self.theme.palette.background))
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
}

fn flatten_keymap(keymap: &Keymap) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for binding in keymap
        .global
        .iter()
        .chain(keymap.input.iter())
        .chain(keymap.buffer.iter())
    {
        map.insert(binding.key.to_ascii_lowercase(), binding.action.clone());
    }
    map
}

fn parse_color(value: &str) -> Color {
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

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

fn canonical_key(key: KeyEvent) -> String {
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
