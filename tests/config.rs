use std::os::unix::fs::PermissionsExt;

use glacier::config::{Config, ConfigWatcher, FeaturesConfig, IdleMode, MascotForm, TabId};

#[test]
fn config_round_trips_atomically_with_private_permissions() {
    let root = std::env::temp_dir().join(format!("glacier-config-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let path = root.join("config.json");

    let mut cfg = Config::default();
    cfg.features.music = false;
    cfg.features.brightness = false;
    cfg.features.default_tab = TabId::Focus;
    cfg.appearance.hover_open_delay_ms = 120;
    cfg.appearance.escape_quits = false;
    cfg.mascot.idle_mode = IdleMode::Clock;
    cfg.music.online_lrclib = false;
    cfg.focus.focus_minutes = 40;
    cfg.controls.volume_max = 1.5;
    cfg.system.tray_icon = false;

    cfg.save_to(&path).unwrap();

    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );

    let loaded = Config::load_from(&path);
    assert_eq!(loaded, cfg);

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn config_watcher_detects_atomic_updates_live() {
    let root = std::env::temp_dir().join(format!("glacier-watch-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let path = root.join("config.json");

    let mut watcher = ConfigWatcher::new(path.clone());
    assert!(watcher.poll_now().is_none());

    let mut first = Config::default();
    first.features.shelf = false;
    first.save_to(&path).unwrap();

    let reloaded = watcher.poll_now().expect("watcher should detect creation");
    assert!(!reloaded.features.shelf);
    assert!(watcher.poll_now().is_none());

    let mut second = reloaded;
    second.features.shelf = true;
    second.focus.break_minutes = 10;
    second.save_to(&path).unwrap();

    let updated = watcher
        .poll_now()
        .expect("watcher should detect atomic rename");
    assert!(updated.features.shelf);
    assert_eq!(updated.focus.break_minutes, 10);

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn invalid_json_falls_back_cleanly_to_defaults() {
    let root = std::env::temp_dir().join(format!("glacier-corrupt-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("config.json");
    std::fs::write(&path, b"{ not valid json").unwrap();

    let loaded = Config::load_from(&path);
    assert_eq!(loaded, Config::default());

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn sub_feature_toggles_gate_control_and_clips_tabs() {
    let mut features = FeaturesConfig::default();
    assert!(features.volume_enabled());
    assert!(features.brightness_enabled());
    assert!(features.is_tab_enabled(TabId::Control));

    features.volume = false;
    assert!(features.is_tab_enabled(TabId::Control));
    features.brightness = false;
    assert!(!features.is_tab_enabled(TabId::Control));

    features.clipboard = false;
    assert!(features.is_tab_enabled(TabId::Clips));
    features.notes = false;
    assert!(!features.is_tab_enabled(TabId::Clips));
}

#[test]
fn app_menu_desktop_entry_opens_settings_separately_from_autostart() {
    let app_menu = include_str!("../packaging/glaciernotch.desktop");
    let autostart = include_str!("../packaging/glaciernotch-autostart.desktop");
    assert!(app_menu.contains("Name=GlacierNotch\n"));
    assert!(app_menu.contains("--settings"));
    assert!(!app_menu.contains("X-GNOME-Autostart-enabled=true"));
    assert!(autostart.contains("Exec=systemctl --user start glaciernotch.service\n"));
    assert!(autostart.contains("X-GNOME-Autostart-enabled=true\n"));
}

#[test]
fn mascot_form_defaults_to_kurama_and_roundtrips_all_modes() {
    let root = std::env::temp_dir().join(format!("glacier-mascot-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let path = root.join("config.json");

    let defaults = Config::default();
    assert_eq!(defaults.mascot.form, MascotForm::Kurama);

    // Backwards compatibility: existing config without `form` loads as Kurama.
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        &path,
        r#"{ "mascot": { "idle_mode": "mascot", "show_aura": false } }"#,
    )
    .unwrap();
    let legacy = Config::load_from(&path);
    assert_eq!(legacy.mascot.form, MascotForm::Kurama);
    assert!(!legacy.mascot.show_aura);

    for form in MascotForm::ALL {
        let mut cfg = Config::default();
        cfg.mascot.form = form;
        cfg.save_to(&path).unwrap();
        let loaded = Config::load_from(&path);
        assert_eq!(loaded.mascot.form, form);
    }

    std::fs::remove_dir_all(&root).unwrap();
}
