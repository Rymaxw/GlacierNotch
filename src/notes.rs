use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::storage;

pub const MAX_NOTES: usize = 40;
pub const MAX_CHARS: usize = 6000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Note {
    pub id: u64,
    pub text: String,
}

#[derive(Clone, Debug)]
pub enum Action {
    Add(String),
    Edit { id: u64, text: String },
    Copy(u64),
    Remove(u64),
}

pub struct Notes {
    pub items: Vec<Note>,
    pub error: Option<String>,
    path: PathBuf,
}

impl Default for Notes {
    fn default() -> Self {
        Self::from_path(storage::data_path("notes.json"))
    }
}

impl Notes {
    pub fn from_path(path: PathBuf) -> Self {
        let (items, error) = match storage::read_at(&path) {
            Ok(items) => (items, None),
            Err(error) => (Vec::new(), Some(format!("Cannot read notes: {error}"))),
        };
        Self { items, error, path }
    }

    pub fn act(&mut self, action: Action) -> Option<String> {
        self.error = None;
        let mut items = self.items.clone();
        match action {
            Action::Copy(id) => {
                return self
                    .items
                    .iter()
                    .find(|note| note.id == id)
                    .map(|note| note.text.clone());
            }
            Action::Add(text) => {
                if items.len() >= MAX_NOTES {
                    self.error = Some("Note limit reached. Delete a note first.".into());
                    return None;
                }
                if !self.validate(&text) {
                    return None;
                }
                let Some(id) = items
                    .iter()
                    .map(|note| note.id)
                    .max()
                    .unwrap_or(0)
                    .checked_add(1)
                else {
                    self.error = Some("Cannot allocate another note ID.".into());
                    return None;
                };
                items.insert(0, Note { id, text });
            }
            Action::Edit { id, text } => {
                if !self.validate(&text) {
                    return None;
                }
                let Some(note) = items.iter_mut().find(|note| note.id == id) else {
                    self.error = Some("This note no longer exists.".into());
                    return None;
                };
                note.text = text;
            }
            Action::Remove(id) => items.retain(|note| note.id != id),
        }
        match storage::write_at(&self.path, &items) {
            Ok(()) => self.items = items,
            Err(error) => self.error = Some(format!("Cannot save notes: {error}")),
        }
        None
    }

    fn validate(&mut self, text: &str) -> bool {
        if text.trim().is_empty() {
            self.error = Some("Write a note before saving.".into());
            false
        } else if text.chars().count() > MAX_CHARS {
            self.error = Some("Keep each note within 6000 characters.".into());
            false
        } else {
            true
        }
    }
}
