//! Mouse event handling.
//!
//! This module provides extracted mouse handling logic for the UI engine.
//! It's prepared for future refactoring of UiEngine::handle_mouse.

#![allow(dead_code)]

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use tirc_core::UiEvent;

use crate::engine::{FocusPane, UiIntent};

/// Layout regions for mouse hit testing.
#[derive(Debug, Clone, Default)]
pub(crate) struct LayoutRegions {
    pub servers: Option<Rect>,
    pub buffers: Option<Rect>,
    pub chat: Option<Rect>,
    pub nicklist: Option<Rect>,
}

impl LayoutRegions {
    /// Check if a position is within a region.
    pub fn contains(&self, region: Option<Rect>, col: u16, row: u16) -> bool {
        region.map(|r| {
            col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
        }).unwrap_or(false)
    }
}

/// Mouse handler context holding mutable references to UI state.
pub(crate) struct MouseContext<'a> {
    pub regions: &'a LayoutRegions,
    pub focus_pane: &'a mut FocusPane,
    pub server_selected: &'a mut usize,
    pub buffer_selected: &'a mut usize,
    pub nick_selected: &'a mut usize,
    pub server_names: &'a [String],
    pub buffer_refs: &'a [(String, String)],
    pub nick_refs: &'a [String],
    pub chat_scroll: &'a mut u16,
    pub chat_max_scroll: u16,
    pub chat_follow_tail: &'a mut bool,
}

impl<'a> MouseContext<'a> {
    /// Scroll chat up by specified lines.
    fn scroll_up(&mut self, lines: u16) {
        *self.chat_follow_tail = false;
        *self.chat_scroll = self.chat_scroll.saturating_sub(lines);
    }

    /// Scroll chat down by specified lines.
    fn scroll_down(&mut self, lines: u16) {
        *self.chat_scroll = (*self.chat_scroll + lines).min(self.chat_max_scroll);
        if *self.chat_scroll >= self.chat_max_scroll {
            *self.chat_follow_tail = true;
        }
    }

    /// Handle a mouse event and return UI intents.
    pub fn handle(&mut self, mouse: MouseEvent) -> Vec<UiIntent> {
        let col = mouse.column;
        let row = mouse.row;

        match mouse.kind {
            MouseEventKind::ScrollUp => self.handle_scroll_up(col, row),
            MouseEventKind::ScrollDown => self.handle_scroll_down(col, row),
            MouseEventKind::Down(MouseButton::Left) => self.handle_left_click(col, row),
            _ => Vec::new(),
        }
    }

    fn handle_scroll_up(&mut self, col: u16, row: u16) -> Vec<UiIntent> {
        if self.regions.contains(self.regions.nicklist, col, row) {
            if *self.nick_selected > 0 {
                *self.nick_selected = self.nick_selected.saturating_sub(3);
            }
            vec![UiIntent::Event(UiEvent::Tick)]
        } else if self.regions.contains(self.regions.chat, col, row) {
            self.scroll_up(3);
            vec![UiIntent::Event(UiEvent::Tick)]
        } else if self.regions.contains(self.regions.servers, col, row) {
            if *self.server_selected > 0 {
                *self.server_selected = self.server_selected.saturating_sub(1);
            }
            vec![UiIntent::Event(UiEvent::Tick)]
        } else if self.regions.contains(self.regions.buffers, col, row) {
            if *self.buffer_selected > 0 {
                *self.buffer_selected = self.buffer_selected.saturating_sub(1);
            }
            vec![UiIntent::Event(UiEvent::Tick)]
        } else {
            self.scroll_up(3);
            vec![UiIntent::Event(UiEvent::Tick)]
        }
    }

    fn handle_scroll_down(&mut self, col: u16, row: u16) -> Vec<UiIntent> {
        if self.regions.contains(self.regions.nicklist, col, row) {
            if !self.nick_refs.is_empty() {
                *self.nick_selected = (*self.nick_selected + 3).min(self.nick_refs.len().saturating_sub(1));
            }
            vec![UiIntent::Event(UiEvent::Tick)]
        } else if self.regions.contains(self.regions.chat, col, row) {
            self.scroll_down(3);
            vec![UiIntent::Event(UiEvent::Tick)]
        } else if self.regions.contains(self.regions.servers, col, row) {
            if !self.server_names.is_empty() {
                *self.server_selected = (*self.server_selected + 1).min(self.server_names.len().saturating_sub(1));
            }
            vec![UiIntent::Event(UiEvent::Tick)]
        } else if self.regions.contains(self.regions.buffers, col, row) {
            if !self.buffer_refs.is_empty() {
                *self.buffer_selected = (*self.buffer_selected + 1).min(self.buffer_refs.len().saturating_sub(1));
            }
            vec![UiIntent::Event(UiEvent::Tick)]
        } else {
            self.scroll_down(3);
            vec![UiIntent::Event(UiEvent::Tick)]
        }
    }

    fn handle_left_click(&mut self, col: u16, row: u16) -> Vec<UiIntent> {
        if self.regions.contains(self.regions.servers, col, row) {
            if let Some(rect) = self.regions.servers {
                let rel_y = (row - rect.y).saturating_sub(1) as usize;
                if rel_y < self.server_names.len() {
                    *self.server_selected = rel_y;
                    *self.focus_pane = FocusPane::Servers;
                }
            }
            vec![UiIntent::Event(UiEvent::Tick)]
        } else if self.regions.contains(self.regions.buffers, col, row) {
            if let Some(rect) = self.regions.buffers {
                let rel_y = (row - rect.y).saturating_sub(1) as usize;
                if rel_y < self.buffer_refs.len() {
                    *self.buffer_selected = rel_y;
                    *self.focus_pane = FocusPane::Buffers;
                    if let Some((network, name)) = self.buffer_refs.get(rel_y).cloned() {
                        return vec![
                            UiIntent::Event(UiEvent::Tick),
                            UiIntent::SubmitLine(format!("/buffer {}/{}", network, name)),
                        ];
                    }
                }
            }
            vec![UiIntent::Event(UiEvent::Tick)]
        } else if self.regions.contains(self.regions.nicklist, col, row) {
            if let Some(rect) = self.regions.nicklist {
                let rel_y = (row - rect.y).saturating_sub(1) as usize;
                if rel_y < self.nick_refs.len() {
                    *self.nick_selected = rel_y;
                    *self.focus_pane = FocusPane::Nicklist;
                }
            }
            vec![UiIntent::Event(UiEvent::Tick)]
        } else if self.regions.contains(self.regions.chat, col, row) {
            *self.focus_pane = FocusPane::Chat;
            vec![UiIntent::Event(UiEvent::Tick)]
        } else {
            Vec::new()
        }
    }
}
