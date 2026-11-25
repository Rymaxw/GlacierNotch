//! Transient file shelf: whatever is dropped on the notch stays reachable from
//! the Shelf tab until it is cleared or Glacier exits.

use std::path::PathBuf;
use std::process::{Command, Stdio};

pub const MAX_ITEMS: usize = 8;

#[derive(Clone, Debug)]
pub struct Item {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub dir: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open(usize),
    Copy(usize),
    Clear,
}

#[derive(Default)]
pub struct Shelf {
    pub items: Vec<Item>,
}

impl Shelf {
    pub fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            let Some(item) = describe(path) else {
                continue;
            };
            if let Some(slot) = self.items.iter().position(|old| old.path == item.path) {
                self.items.remove(slot);
            }
            self.items.insert(0, item);
        }
        self.items.truncate(MAX_ITEMS);
    }

    /// Returns a path string when the action was `Copy`, so the caller can hand
    /// it to the clipboard owner.
    pub fn act(&mut self, action: Action) -> Option<String> {
        match action {
            Action::Open(index) => {
                if let Some(item) = self.items.get(index) {
                    let _ = Command::new("xdg-open")
                        .arg(&item.path)
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
                None
            }
            Action::Copy(index) => self
                .items
                .get(index)
                .map(|item| item.path.display().to_string()),
            Action::Clear => {
                self.items.clear();
                None
            }
        }
    }
}

fn describe(path: PathBuf) -> Option<Item> {
    let meta = std::fs::metadata(&path).ok()?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    Some(Item {
        path,
        name,
        size: meta.len(),
        dir: meta.is_dir(),
    })
}

/// `1.4 MB` style sizes; directories report a dash because their byte count is
/// meaningless without walking them.
pub fn human_size(bytes: u64, dir: bool) -> String {
    if dir {
        return "folder".to_owned();
    }
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_like_a_file_manager() {
        assert_eq!(human_size(0, false), "0 B");
        assert_eq!(human_size(999, false), "999 B");
        assert_eq!(human_size(1536, false), "1.5 KB");
        assert_eq!(human_size(1_468_006, false), "1.4 MB");
        assert_eq!(human_size(42, true), "folder");
    }

    #[test]
    fn newest_drop_lands_first_and_duplicates_moved_up() {
        let mut shelf = Shelf::default();
        let temp = std::env::temp_dir().join("glacier-shelf-test");
        std::fs::create_dir_all(&temp).unwrap();
        let a = temp.join("a.txt");
        let b = temp.join("b.txt");
        std::fs::write(&a, b"aaaa").unwrap();
        std::fs::write(&b, b"bb").unwrap();

        shelf.add(vec![a.clone(), b.clone()]);
        assert_eq!(shelf.items.len(), 2);
        assert_eq!(shelf.items[0].name, "b.txt");

        shelf.add(vec![a.clone()]);
        assert_eq!(shelf.items.len(), 2);
        assert_eq!(shelf.items[0].name, "a.txt");
        assert_eq!(shelf.items[0].size, 4);

        let copied = shelf.act(Action::Copy(0));
        assert_eq!(copied.as_deref(), Some(a.to_str().unwrap()));

        shelf.act(Action::Clear);
        assert!(shelf.items.is_empty());
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn missing_paths_are_ignored_and_list_is_capped() {
        let mut shelf = Shelf::default();
        shelf.add(vec![PathBuf::from("/definitely/not/here/xyz")]);
        assert!(shelf.items.is_empty());

        let temp = std::env::temp_dir().join("glacier-shelf-cap");
        std::fs::create_dir_all(&temp).unwrap();
        let paths: Vec<PathBuf> = (0..MAX_ITEMS + 3)
            .map(|i| {
                let path = temp.join(format!("f{i}.txt"));
                std::fs::write(&path, b"x").unwrap();
                path
            })
            .collect();
        shelf.add(paths);
        assert_eq!(shelf.items.len(), MAX_ITEMS);
        let _ = std::fs::remove_dir_all(&temp);
    }
}
