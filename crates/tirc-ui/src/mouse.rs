//! Mouse event handling.
//!
//! This module provides extracted mouse handling logic for the UI engine.
//! It's prepared for future refactoring of UiEngine::handle_mouse.
//!
//! NOTE: Currently unused - mouse handling is inline in engine.rs.
//! This module exists as a placeholder for future extraction.

#![allow(dead_code)]

use ratatui::layout::Rect;

use crate::engine::FocusPane;

/// Layout regions for mouse hit testing.
#[derive(Debug, Clone, Default)]
pub(crate) struct LayoutRegions {
    pub sidebar: Option<Rect>,
    pub chat: Option<Rect>,
    pub nicklist: Option<Rect>,
}

impl LayoutRegions {
    /// Check if a position is within a region.
    pub fn contains(&self, region: Option<Rect>, col: u16, row: u16) -> bool {
        region
            .map(|r| col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height)
            .unwrap_or(false)
    }
}

/// Mouse handler context holding mutable references to UI state.
pub(crate) struct MouseContext<'a> {
    pub regions: &'a LayoutRegions,
    pub focus_pane: &'a mut FocusPane,
    pub sidebar_selected: &'a mut usize,
    pub nick_selected: &'a mut usize,
    pub sidebar_items_len: usize,
    pub nick_refs_len: usize,
    pub chat_scroll: &'a mut u16,
    pub chat_max_scroll: u16,
    pub chat_follow_tail: &'a mut bool,
}
