use std::io;
use std::path::Path;

use super::Tab;

pub struct TabManager {
    pub tabs: Vec<Tab>,
    pub active_index: usize,
    untitled_counter: usize,
}

impl TabManager {
    pub fn new() -> Self {
        let first = Tab::new_untitled("Untitled 1".into());
        Self {
            tabs: vec![first],
            active_index: 0,
            untitled_counter: 1,
        }
    }

    pub fn new_tab(&mut self) -> usize {
        self.untitled_counter += 1;
        let name = format!("Untitled {}", self.untitled_counter);
        self.tabs.push(Tab::new_untitled(name));
        self.active_index = self.tabs.len() - 1;
        self.active_index
    }

    pub fn open_file(&mut self, path: &Path) -> io::Result<usize> {
        let tab = Tab::from_file(path)?;
        self.tabs.push(tab);
        self.active_index = self.tabs.len() - 1;
        Ok(self.active_index)
    }

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_index]
    }

    pub fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_index]
    }

    pub fn set_active(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_index = index;
        }
    }

    pub fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.untitled_counter += 1;
            let name = format!("Untitled {}", self.untitled_counter);
            self.tabs.push(Tab::new_untitled(name));
            self.active_index = 0;
        } else if self.active_index >= self.tabs.len() {
            self.active_index = self.tabs.len() - 1;
        } else if self.active_index > index {
            self.active_index -= 1;
        } else if self.active_index == index && self.active_index >= self.tabs.len() {
            self.active_index = self.tabs.len() - 1;
        }
    }

    #[allow(dead_code)]
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }
}
