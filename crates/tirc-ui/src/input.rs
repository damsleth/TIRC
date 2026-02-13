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
