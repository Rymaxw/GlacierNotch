use std::io;
use std::path::PathBuf;

const DESKTOP_FILE: &str = "glaciernotch.desktop";

pub fn entry_path() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    config.join("autostart").join(DESKTOP_FILE)
}

fn desktop_entry() -> &'static str {
    include_str!("../packaging/glaciernotch.desktop")
}

pub fn status() -> bool {
    entry_path().is_file()
}

pub fn enable() -> io::Result<PathBuf> {
    let path = entry_path();
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("autostart path has no parent directory"))?;
    std::fs::create_dir_all(parent)?;
    std::fs::write(&path, desktop_entry())?;
    Ok(path)
}

pub fn disable() -> io::Result<PathBuf> {
    let path = entry_path();
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_starts_the_single_service_without_a_terminal() {
        let entry = desktop_entry();
        assert!(entry.contains("Name=GlacierNotch\n"));
        assert!(entry.contains("Exec=systemctl --user start glaciernotch.service\n"));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true\n"));
        assert!(entry.contains("Terminal=false\n"));
        assert!(!entry.contains("--demo"));
    }

    #[test]
    fn autostart_path_uses_new_app_name() {
        assert!(entry_path().ends_with("autostart/glaciernotch.desktop"));
    }
}
