use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};

/// Palette matcher state.
#[derive(Debug, Clone, Default)]
pub struct PaletteEngine {
    items: Vec<String>,
    pub query: String,
    pub selected: usize,
    pub results: Vec<String>,
}

impl PaletteEngine {
    pub fn new(items: Vec<String>) -> Self {
        let mut this = Self {
            items,
            query: String::new(),
            selected: 0,
            results: Vec::new(),
        };
        this.refresh();
        this
    }

    pub fn set_items(&mut self, items: Vec<String>) {
        self.items = items;
        self.refresh();
    }

    pub fn clear_query(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.refresh();
    }

    pub fn query_push(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
        self.refresh();
    }

    pub fn query_backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
        self.refresh();
    }

    pub fn move_selection_up(&mut self) {
        if self.results.is_empty() {
            self.selected = 0;
            return;
        }

        if self.selected == 0 {
            self.selected = self.results.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    pub fn move_selection_down(&mut self) {
        if self.results.is_empty() {
            self.selected = 0;
            return;
        }

        self.selected = (self.selected + 1) % self.results.len();
    }

    pub fn selected_item(&self) -> Option<&str> {
        self.results.get(self.selected).map(String::as_str)
    }

    pub fn refresh(&mut self) {
        if self.query.trim().is_empty() {
            self.results = self.items.clone();
            return;
        }

        let pattern = Pattern::new(
            &self.query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
        let mut buf = Vec::new();

        let mut scored: Vec<(u32, &String)> = self
            .items
            .iter()
            .filter_map(|item| {
                let haystack = Utf32Str::new(item, &mut buf);
                let score = pattern.score(haystack, &mut matcher)?;
                Some((score, item))
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        self.results = scored.into_iter().map(|(_, item)| item.clone()).collect();

        if self.selected >= self.results.len() {
            self.selected = self.results.len().saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_match_basic() {
        let engine = PaletteEngine::new(vec![
            "connect".to_owned(),
            "config".to_owned(),
            "quit".to_owned(),
        ]);
        assert_eq!(engine.results.len(), 3);
    }

    #[test]
    fn fuzzy_match_filters() {
        let mut engine = PaletteEngine::new(vec![
            "connect".to_owned(),
            "config".to_owned(),
            "quit".to_owned(),
        ]);
        engine.query_push('q');
        assert_eq!(engine.results, vec!["quit"]);
    }

    #[test]
    fn fuzzy_match_partial() {
        let mut engine = PaletteEngine::new(vec![
            "connect".to_owned(),
            "config".to_owned(),
            "quit".to_owned(),
        ]);
        engine.query_push('c');
        engine.query_push('o');
        // Both "connect" and "config" should match "co"
        assert!(engine.results.contains(&"connect".to_owned()));
        assert!(engine.results.contains(&"config".to_owned()));
        assert!(!engine.results.contains(&"quit".to_owned()));
    }
}
