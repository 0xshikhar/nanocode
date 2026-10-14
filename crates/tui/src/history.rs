#[derive(Clone, Debug, Default)]
pub struct History {
    entries: Vec<String>,
    position: Option<usize>,
    draft: String,
}

impl History {
    #[must_use]
    pub const fn new(entries: Vec<String>) -> Self {
        Self {
            entries,
            position: None,
            draft: String::new(),
        }
    }

    #[must_use]
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn push(&mut self, entry: impl Into<String>) -> bool {
        let entry = entry.into();
        self.position = None;
        self.draft.clear();
        if entry.trim().is_empty() || self.entries.last() == Some(&entry) {
            return false;
        }
        self.entries.push(entry);
        true
    }

    pub fn older(&mut self, current: &str) -> Option<&str> {
        let index = match self.position {
            None => {
                let last = self.entries.len().checked_sub(1)?;
                self.draft = current.to_owned();
                last
            }
            Some(0) => return None,
            Some(index) => index - 1,
        };
        self.position = Some(index);
        self.entries.get(index).map(String::as_str)
    }

    pub fn newer(&mut self) -> Option<&str> {
        let index = self.position?;
        if index + 1 < self.entries.len() {
            self.position = Some(index + 1);
            return self.entries.get(index + 1).map(String::as_str);
        }
        self.position = None;
        Some(self.draft.as_str())
    }

    #[must_use]
    pub const fn is_browsing(&self) -> bool {
        self.position.is_some()
    }

    pub fn reset(&mut self) {
        self.position = None;
        self.draft.clear();
    }
}
