use super::*;

struct Target {
    id: String,
    rect: Rect,
    action: Action,
    arrows: Option<[Action; 2]>,
}

fn target(id: impl Into<String>, rect: Rect, action: Action) -> Target {
    Target {
        id: id.into(),
        rect,
        action,
        arrows: None,
    }
}

fn targets(view: &View<'_>, state: &Interactions) -> Vec<Target> {
    let w = view.w;
    let mut list = Vec::new();
    let enabled_tabs = view.config.features.enabled_tabs();
    if !enabled_tabs.is_empty() {
        let strip_w = tab_strip_width(w, view.config);
        let slot = strip_w / enabled_tabs.len() as f32;
        for (index, tab) in enabled_tabs.iter().enumerate() {
            list.push(target(
                format!("tab-{index}"),
                Rect::from_min_size(
                    Pos2::new(slot * index as f32, 0.0),
                    Vec2::new(slot, TAB_H - 4.0),
                ),
                Action::Tab(*tab),
            ));
        }
    }
    if view.config.appearance.show_settings_button {
        list.push(target("settings-gear", gear_rect(w), Action::OpenSettings));
    }
    if enabled_tabs.is_empty() {
        list.push(target(
            "open-settings-empty",
            Rect::from_center_size(Pos2::new(w / 2.0, 152.0), Vec2::new(124.0, 28.0)),
            Action::OpenSettings,
        ));
        return list;
    }
    match view.tab {
        Tab::Music => {
            let track = view.track;
            let seek_step = view.config.music.seek_step_secs as f64;
            let seekable = track.can_seek && track.length.is_some_and(|length| length > 0.0);
            if seekable {
                let mut progress = target(
                    "progress",
                    Rect::from_min_max(Pos2::new(16.0, 128.0), Pos2::new(w - 16.0, 148.0)),
                    Action::Seek(track.position),
                );
                progress.arrows = Some([
                    Action::Transport(Cmd::SeekBy(-seek_step)),
                    Action::Transport(Cmd::SeekBy(seek_step)),
                ]);
                list.push(progress);
                list.push(target(
                    "elapsed",
                    Rect::from_min_max(Pos2::new(16.0, 150.0), Pos2::new(70.0, 166.0)),
                    Action::Transport(Cmd::SeekBy(-seek_step)),
                ));
                list.push(target(
                    "remaining",
                    Rect::from_min_max(Pos2::new(w - 70.0, 150.0), Pos2::new(w - 16.0, 166.0)),
                    Action::Transport(Cmd::SeekBy(seek_step)),
                ));
                if view.config.music.show_lyrics {
                    for (index, row) in lyric_rows(view).into_iter().enumerate() {
                        if let Some(at) = row.at {
                            list.push(target(
                                format!("lyric-{index}"),
                                Rect::from_min_max(
                                    Pos2::new(16.0, 168.0 + index as f32 * 22.0),
                                    Pos2::new(w - 16.0, 190.0 + index as f32 * 20.0),
                                ),
                                Action::Seek(at),
                            ));
                        }
                    }
                }
            }
            let cx = w / 2.0 - 30.0;
            for (index, delta, size, enabled, cmd) in [
                (0, -44.0, 30.0, track.can_prev, Cmd::Previous),
                (1, 0.0, 38.0, !track.title.is_empty(), Cmd::PlayPause),
                (2, 44.0, 30.0, track.can_next, Cmd::Next),
            ] {
                if enabled {
                    list.push(target(
                        format!("transport-{index}"),
                        Rect::from_center_size(Pos2::new(cx + delta, 246.0), Vec2::splat(size)),
                        Action::Transport(cmd),
                    ));
                }
            }
            if track.can_control {
                list.push(target(
                    "rate",
                    Rect::from_center_size(Pos2::new(w - 46.0, 246.0), Vec2::new(48.0, 22.0)),
                    Action::Rate(next_rate(track.rate)),
                ));
            }
        }
        Tab::Control => {
            let mut rows = Vec::new();
            if view.config.features.volume_enabled() {
                rows.push((0u8, view.controls.volume, view.controls.has_volume()));
            }
            if view.config.features.brightness_enabled() {
                rows.push((
                    1u8,
                    view.controls.brightness,
                    view.controls.has_brightness(),
                ));
            }
            for (slot, (index, value, enabled)) in rows.into_iter().enumerate() {
                if enabled {
                    let top = 92.0 + slot as f32 * 72.0;
                    let mut slider = target(
                        format!("slider-{index}"),
                        Rect::from_min_max(
                            Pos2::new(16.0, top + 4.0),
                            Pos2::new(w - 16.0, top + 30.0),
                        ),
                        slider_action(index, value, true),
                    );
                    let max = if index == 0 {
                        view.controls.volume_max()
                    } else {
                        1.0
                    };
                    slider.arrows = Some([
                        slider_action(index, (value - 0.03).clamp(0.0, max), true),
                        slider_action(index, (value + 0.03).clamp(0.0, max), true),
                    ]);
                    list.push(slider);
                }
            }
        }
        Tab::Focus => {
            for (id, dx, y, width, action) in [
                (
                    "focus-mode",
                    -62.0,
                    174.0,
                    116.0,
                    focus::Action::Set(focus::Mode::Focus),
                ),
                (
                    "break-mode",
                    62.0,
                    174.0,
                    108.0,
                    focus::Action::Set(focus::Mode::Break),
                ),
                ("timer-toggle", -62.0, 212.0, 116.0, focus::Action::Toggle),
                ("timer-reset", 62.0, 212.0, 108.0, focus::Action::Reset),
            ] {
                list.push(target(
                    id,
                    Rect::from_center_size(Pos2::new(w / 2.0 + dx, y), Vec2::new(width, 26.0)),
                    Action::Timer(action),
                ));
            }
        }
        Tab::Shelf => {
            for (index, _) in view
                .shelf
                .items
                .iter()
                .enumerate()
                .skip(state.page * PAGE_SIZE)
                .take(PAGE_SIZE)
            {
                let top = 46.0 + (index % PAGE_SIZE) as f32 * 34.0;
                list.push(target(
                    format!("shelf-open-{index}"),
                    Rect::from_min_size(Pos2::new(w - 108.0, top + 5.0), Vec2::new(44.0, 20.0)),
                    Action::Shelf(shelf::Action::Open(index)),
                ));
                list.push(target(
                    format!("shelf-copy-{index}"),
                    Rect::from_min_size(Pos2::new(w - 58.0, top + 5.0), Vec2::new(44.0, 20.0)),
                    Action::Shelf(shelf::Action::Copy(index)),
                ));
            }
            page_targets(&mut list, state.page, view.shelf.items.len());
            if !view.shelf.items.is_empty() {
                list.push(target(
                    "shelf-clear",
                    Rect::from_min_size(Pos2::new(w - 84.0, 250.0), Vec2::new(70.0, 20.0)),
                    Action::Shelf(shelf::Action::Clear),
                ));
            }
        }
        Tab::Clips => {
            let clips_on = view.config.features.clipboard;
            let notes_on = view.config.features.notes;
            if clips_on {
                list.push(target(
                    "history-view",
                    Rect::from_min_size(Pos2::new(16.0, 34.0), Vec2::new(73.0, 24.0)),
                    Action::Ui(UiAction::Notes(false)),
                ));
            }
            if notes_on {
                let x = if clips_on { 97.0 } else { 16.0 };
                list.push(target(
                    "notes-view",
                    Rect::from_min_size(Pos2::new(x, 34.0), Vec2::new(59.0, 24.0)),
                    Action::Ui(UiAction::Notes(true)),
                ));
            }
            if notes_on && (state.notes || !clips_on) {
                list.push(target(
                    "new-note",
                    Rect::from_min_size(Pos2::new(w - 96.0, 34.0), Vec2::new(80.0, 24.0)),
                    Action::Ui(UiAction::EditNote(None)),
                ));
                for (index, note) in view
                    .notes
                    .items
                    .iter()
                    .enumerate()
                    .skip(state.page * PAGE_SIZE)
                    .take(PAGE_SIZE)
                {
                    let top = 68.0 + (index % PAGE_SIZE) as f32 * 34.0;
                    list.push(target(
                        format!("note-copy-{}", note.id),
                        Rect::from_min_size(Pos2::new(w - 132.0, top + 5.0), Vec2::new(46.0, 20.0)),
                        Action::Note(notes::Action::Copy(note.id)),
                    ));
                    list.push(target(
                        format!("note-edit-{}", note.id),
                        Rect::from_min_size(Pos2::new(w - 82.0, top + 5.0), Vec2::new(46.0, 20.0)),
                        Action::Ui(UiAction::EditNote(Some(note.id))),
                    ));
                    list.push(target(
                        format!("note-remove-{}", note.id),
                        Rect::from_min_size(Pos2::new(w - 32.0, top + 5.0), Vec2::new(20.0, 20.0)),
                        Action::Note(notes::Action::Remove(note.id)),
                    ));
                }
                page_targets(&mut list, state.page, view.notes.items.len());
            } else {
                for (index, clip) in view
                    .clips
                    .ordered()
                    .iter()
                    .enumerate()
                    .skip(state.page * PAGE_SIZE)
                    .take(PAGE_SIZE)
                {
                    let top = 68.0 + (index % PAGE_SIZE) as f32 * 34.0;
                    list.push(target(
                        format!("clip-copy-{index}"),
                        Rect::from_min_size(Pos2::new(w - 132.0, top + 5.0), Vec2::new(46.0, 20.0)),
                        Action::Clips(crate::clipboard::Action::Copy(clip.text.clone())),
                    ));
                    list.push(target(
                        format!("clip-pin-{index}"),
                        Rect::from_min_size(Pos2::new(w - 82.0, top + 5.0), Vec2::new(46.0, 20.0)),
                        Action::Clips(crate::clipboard::Action::TogglePin(clip.text.clone())),
                    ));
                    list.push(target(
                        format!("clip-remove-{index}"),
                        Rect::from_min_size(Pos2::new(w - 32.0, top + 5.0), Vec2::new(20.0, 20.0)),
                        Action::Clips(crate::clipboard::Action::Remove(clip.text.clone())),
                    ));
                }
                page_targets(&mut list, state.page, view.clips.items.len());
                if !view.clips.items.is_empty() {
                    list.push(target(
                        "clips-clear",
                        Rect::from_min_size(Pos2::new(w - 84.0, 250.0), Vec2::new(70.0, 20.0)),
                        Action::Clips(crate::clipboard::Action::Clear),
                    ));
                }
            }
        }
    }
    list
}

fn page_targets(list: &mut Vec<Target>, page: usize, count: usize) {
    if page > 0 {
        list.push(target(
            "previous-page",
            Rect::from_min_size(Pos2::new(16.0, 250.0), Vec2::new(46.0, 20.0)),
            Action::Ui(UiAction::Page(-1)),
        ));
    }
    if (page + 1) * PAGE_SIZE < count {
        list.push(target(
            "next-page",
            Rect::from_min_size(Pos2::new(154.0, 250.0), Vec2::new(46.0, 20.0)),
            Action::Ui(UiAction::Page(1)),
        ));
    }
}

pub(super) fn draw(
    ctx: &egui::Context,
    state: &mut Interactions,
    view: &View<'_>,
    offset: Vec2,
) -> Option<Action> {
    egui::Area::new(egui::Id::unique("glacier-native-controls"))
        .order(egui::Order::Tooltip)
        .fixed_pos(Pos2::ZERO)
        .movable(false)
        .show(ctx, |ui| {
            ui.set_min_size(Vec2::new(W_EXPANDED, H_EXPANDED));
            ui.set_max_size(Vec2::new(W_EXPANDED, H_EXPANDED));
            ui.set_clip_rect(ctx.viewport_rect());
            if state.editor.is_some() {
                return editor(ui, state, view.notes);
            }
            let mut action = None;
            for target in targets(view, state) {
                let rect = target.rect.translate(offset);
                let response = ui.interact(
                    rect,
                    egui::Id::unique(("glacier-control", &target.id)),
                    egui::Sense::click(),
                );
                if state.focus_first {
                    response.request_focus();
                    state.focus_first = false;
                }
                if response.has_focus() {
                    let focus_color = accent_color(view.config);
                    ui.painter().rect_stroke(
                        rect.shrink(1.0),
                        CornerRadius::same(4),
                        Stroke::new(1.5, focus_color),
                        StrokeKind::Inside,
                    );
                    if let Some(arrows) = target.arrows {
                        if ctx.input(|i| !i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) {
                            action = Some(arrows[0].clone());
                        } else if ctx
                            .input(|i| !i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight))
                        {
                            action = Some(arrows[1].clone());
                        }
                    }
                }
                if response.clicked() {
                    action = Some(target.action);
                }
            }
            action
        })
        .inner
}

fn editor(ui: &mut egui::Ui, state: &mut Interactions, notes: &Notes) -> Option<Action> {
    let draft = state.editor.as_mut()?;
    let w = W_EXPANDED;
    let rect = Rect::from_min_max(Pos2::new(16.0, 70.0), Pos2::new(w - 16.0, 238.0));
    ui.painter().rect_filled(rect, CornerRadius::same(8), CHIP);
    ui.painter().text(
        Pos2::new(26.0, 82.0),
        Align2::LEFT_CENTER,
        if draft.id.is_some() {
            "Edit note"
        } else {
            "New note"
        },
        sub_font(),
        TEXT,
    );
    ui.visuals_mut().override_text_color = Some(TEXT);
    ui.visuals_mut().extreme_bg_color = BG;
    ui.visuals_mut().selection.stroke = Stroke::new(1.0, Color32::from_rgb(122, 211, 255));
    let response = ui.put(
        Rect::from_min_max(Pos2::new(26.0, 94.0), Pos2::new(w - 26.0, 190.0)),
        egui::TextEdit::multiline(&mut draft.text)
            .id_salt("quick-note-editor")
            .font(sub_font())
            .desired_rows(5)
            .char_limit(notes::MAX_CHARS)
            .hint_text("Write a quick note"),
    );
    if state.focus_first {
        response.request_focus();
        state.focus_first = false;
    }
    let save = Rect::from_min_size(Pos2::new(w - 172.0, 202.0), Vec2::new(70.0, 24.0));
    let cancel = Rect::from_min_size(Pos2::new(w - 94.0, 202.0), Vec2::new(70.0, 24.0));
    let mut save_clicked = ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Enter));
    let mut cancel_clicked = false;
    for (id, rect, label) in [
        ("save-note", save, "Save"),
        ("cancel-note", cancel, "Cancel"),
    ] {
        let response = ui.interact(rect, egui::Id::unique(id), egui::Sense::click());
        small_button(ui.painter(), rect, label, response.hovered());
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(6),
                Stroke::new(1.5, Color32::from_rgb(122, 211, 255)),
                StrokeKind::Inside,
            );
        }
        if response.clicked() {
            if id == "save-note" {
                save_clicked = true;
            } else {
                cancel_clicked = true;
            }
        }
    }
    if let Some(error) = &notes.error {
        ui.painter().text(
            Pos2::new(26.0, 231.0),
            Align2::LEFT_CENTER,
            clip(ui.painter(), error, &tiny(), w - 52.0),
            tiny(),
            TEXT,
        );
    }
    if save_clicked {
        Some(Action::Note(match draft.id {
            Some(id) => notes::Action::Edit {
                id,
                text: draft.text.clone(),
            },
            None => notes::Action::Add(draft.text.clone()),
        }))
    } else if cancel_clicked {
        Some(Action::Ui(UiAction::CancelNote))
    } else {
        None
    }
}
