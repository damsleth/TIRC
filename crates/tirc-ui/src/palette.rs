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

        let mut scored = self
            .items
            .iter()
            .filter_map(|item| fuzzy_score(&self.query, item).map(|score| (score, item)))
            .collect::<Vec<_>>();

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        self.results = scored.into_iter().map(|(_, item)| item.clone()).collect();

        if self.selected >= self.results.len() {
            self.selected = self.results.len().saturating_sub(1);
        }
    }
}

fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    let query = query.to_ascii_lowercase();
    let candidate = candidate.to_ascii_lowercase();

    let mut score = 0_i32;
    let mut pos = 0_usize;

    for q in query.chars() {
        let slice = &candidate[pos..];
        let found = slice.find(q)?;
        pos += found + 1;
        score += 10 - found as i32;
    }

    let compact_bonus = (candidate.len() as i32 - query.len() as i32).max(0);
    Some(score - compact_bonus)
}
