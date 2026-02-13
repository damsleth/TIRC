/// Line editor with Emacs-like primitives.
#[derive(Debug, Clone, Default)]
pub struct InputEditor {
    text: String,
    cursor: usize,
}

impl InputEditor {
    pub fn from_text(text: String) -> Self {
        let cursor = text.chars().count();
        Self { text, cursor }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
        self.cursor = self.text.chars().count();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.text.chars().count());
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.text.chars().count();
    }

    pub fn insert_char(&mut self, c: char) {
        let mut chars = self.text.chars().collect::<Vec<_>>();
        chars.insert(self.cursor, c);
        self.cursor += 1;
        self.text = chars.into_iter().collect();
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let mut chars = self.text.chars().collect::<Vec<_>>();
        chars.remove(self.cursor - 1);
        self.cursor -= 1;
        self.text = chars.into_iter().collect();
    }

    pub fn delete(&mut self) {
        let len = self.text.chars().count();
        if self.cursor >= len {
            return;
        }

        let mut chars = self.text.chars().collect::<Vec<_>>();
        chars.remove(self.cursor);
        self.text = chars.into_iter().collect();
    }

    pub fn kill_to_end(&mut self) {
        let chars = self.text.chars().collect::<Vec<_>>();
        self.text = chars[..self.cursor].iter().collect();
    }

    pub fn kill_to_start(&mut self) {
        let chars = self.text.chars().collect::<Vec<_>>();
        self.text = chars[self.cursor..].iter().collect();
        self.cursor = 0;
    }

    pub fn kill_word(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let mut chars = self.text.chars().collect::<Vec<_>>();
        let mut idx = self.cursor;

        while idx > 0 && chars[idx - 1].is_whitespace() {
            idx -= 1;
        }
        while idx > 0 && !chars[idx - 1].is_whitespace() {
            idx -= 1;
        }

        chars.drain(idx..self.cursor);
        self.cursor = idx;
        self.text = chars.into_iter().collect();
    }
}

/// Safe wrapper for input history navigation.
/// 
/// Encapsulates history entries, current navigation index, and draft text
/// to prevent out-of-bounds access and ensure consistent state.
#[derive(Debug, Clone, Default)]
pub struct InputHistory {
    entries: Vec<String>,
    /// Current position in history. None = at draft (newest).
    index: Option<usize>,
    /// Draft text saved when navigating into history.
    draft: String,
    /// Maximum number of entries to keep.
    max_entries: usize,
}

impl InputHistory {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            index: None,
            draft: String::new(),
            max_entries,
        }
    }

    /// Push a new entry to history. Resets navigation position.
    pub fn push(&mut self, entry: String) {
        if entry.is_empty() {
            return;
        }
        // Deduplicate: don't add if same as last entry
        if self.entries.last().map(|s| s == &entry).unwrap_or(false) {
            self.reset();
            return;
        }
        self.entries.push(entry);
        // Enforce max entries
        while self.entries.len() > self.max_entries {
            self.entries.remove(0);
        }
        self.reset();
    }

    /// Reset navigation position to newest (draft).
    pub fn reset(&mut self) {
        self.index = None;
        self.draft.clear();
    }

    /// Navigate to previous (older) entry. Returns the text to display, if any.
    pub fn prev(&mut self, current_text: &str) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }

        match self.index {
            None => {
                // Save current text as draft and go to newest history entry
                self.draft = current_text.to_owned();
                self.index = Some(self.entries.len() - 1);
            }
            Some(0) => {
                // Already at oldest, don't move
                return self.entries.first().map(String::as_str);
            }
            Some(idx) => {
                self.index = Some(idx - 1);
            }
        }

        self.index.and_then(|idx| self.entries.get(idx)).map(String::as_str)
    }

    /// Navigate to next (newer) entry. Returns the text to display, if any.
    pub fn next(&mut self) -> Option<&str> {
        let Some(idx) = self.index else {
            return None;
        };

        if idx + 1 < self.entries.len() {
            self.index = Some(idx + 1);
            return self.entries.get(idx + 1).map(String::as_str);
        }

        // Return to draft
        self.index = None;
        Some(&self.draft)
    }

    /// Get current draft text (for restoration).
    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// Check if currently navigating history.
    pub fn is_navigating(&self) -> bool {
        self.index.is_some()
    }
}
