use crate::*;

fn resolve_widget_preview(global: (i32, i32, i32), overrides: (i32, i32, i32)) -> (i32, i32, i32) {
    (
        if overrides.0 < 0 {
            global.0
        } else {
            overrides.0
        },
        if overrides.1 < 0 {
            global.1
        } else {
            overrides.1
        },
        if overrides.2 < 0 {
            global.2
        } else {
            overrides.2
        },
    )
}

pub(crate) fn register_widget_bridge_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    desktop_widgets: &Rc<DesktopWidgetWindows>,
    state: &Rc<RefCell<AppState>>,
    widget_shown: &Rc<Cell<bool>>,
) {
    // -------- 挂件数据/行为中转：与主窗口保持同步 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_prev_month(move || {
            shift_month(&state, -1);
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_next_month(move || {
            shift_month(&state, 1);
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_goto_today(move || {
            goto_today(&state);
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_select_day(move |day| {
            state.borrow_mut().selected_day = day.max(1) as u32;
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let widget_shown = widget_shown.clone();
        widget.on_close_widget(move || {
            widget_shown.set(false);
            if let Some(widget) = widget_weak.upgrade() {
                let _ = widget.hide();
            }
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_widget_visible(false);
            }
        });
    }
    {
        let state = state.clone();
        widget.on_pin_changed(move |pinned| {
            let s = state.borrow();
            if let Err(error) =
                db::set_setting(&s.conn, "widget_pinned", if pinned { "1" } else { "0" })
            {
                eprintln!("保存挂件置顶状态失败: {error}");
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let desktop_widgets = desktop_widgets.clone();
        let state = state.clone();
        widget.on_open_widget_settings(move |instance_key| {
            if let Some(ui) = ui_weak.upgrade() {
                let instance_key = instance_key.to_string();
                let base_kind = instance_key.split(':').next().unwrap_or(&instance_key);
                let name = match base_kind {
                    "calendar" => "月历".to_string(),
                    "events" => "今日日程".to_string(),
                    "countdown" => "倒数日".to_string(),
                    "clock" => "时钟".to_string(),
                    "weather" => "天气".to_string(),
                    "focus" => "专注计时".to_string(),
                    "todo" => "今日待办".to_string(),
                    value if value.starts_with("note_") => {
                        format!("便签 #{}", value.trim_start_matches("note_"))
                    }
                    _ => "桌面卡片".to_string(),
                };
                if let Some(previous) = desktop_widgets.appearance_editor.borrow_mut().take() {
                    previous.invoke_close_requested();
                }
                let Ok(editor) = WidgetAppearanceWindow::new() else {
                    ui.set_action_message("无法打开卡片外观设置".into());
                    return;
                };
                let global_style = desktop_widget_global_style(&state.borrow().conn);
                let original_style = desktop_widget_style(&state.borrow().conn, &instance_key);
                let (opacity_override, theme_override, accent_override) =
                    desktop_widget_instance_overrides(&state.borrow().conn, &instance_key);
                editor.set_instance_key(instance_key.clone().into());
                editor.set_card_name(name.into());
                editor.set_global_opacity(global_style.0);
                editor.set_opacity_override(opacity_override);
                editor.set_theme_override(theme_override);
                editor.set_accent_override(accent_override);
                let preview_committed = Rc::new(Cell::new(false));
                {
                    let source = ui.global::<Theme>();
                    let target = editor.global::<Theme>();
                    target.set_theme_mode(source.get_theme_mode());
                    target.set_system_dark(source.get_system_dark());
                    target.set_accent(source.get_accent());
                    target.set_today_bg(source.get_today_bg());
                    target.set_font_delta(source.get_font_delta());
                    target.set_density_mode(source.get_density_mode());
                    target.set_reduce_motion(source.get_reduce_motion());
                    target.set_font_family(source.get_font_family());
                }
                {
                    let editor_weak = editor.as_weak();
                    let ui_weak = ui.as_weak();
                    let instance_key = instance_key.clone();
                    let preview_committed = preview_committed.clone();
                    editor.on_close_requested(move || {
                        if !preview_committed.get() {
                            if let Some(ui) = ui_weak.upgrade() {
                                apply_desktop_widget_style(
                                    &instance_key,
                                    original_style.0,
                                    original_style.1,
                                    original_style.2,
                                    ui.get_visual_theme(),
                                    ui.get_theme_index(),
                                );
                            }
                        }
                        if let Some(editor) = editor_weak.upgrade() {
                            let _ = editor.hide();
                        }
                    });
                }
                {
                    let ui_weak = ui.as_weak();
                    let state = state.clone();
                    let instance_key = instance_key.clone();
                    editor.on_preview(move |opacity, theme, accent| {
                        if let Some(ui) = ui_weak.upgrade() {
                            let global = desktop_widget_global_style(&state.borrow().conn);
                            let effective =
                                resolve_widget_preview(global, (opacity, theme, accent));
                            apply_desktop_widget_style(
                                &instance_key,
                                effective.0,
                                effective.1,
                                effective.2,
                                ui.get_visual_theme(),
                                ui.get_theme_index(),
                            );
                        }
                    });
                }
                {
                    let editor_weak = editor.as_weak();
                    let ui_weak = ui.as_weak();
                    let state = state.clone();
                    let instance_key = instance_key.clone();
                    let preview_committed = preview_committed.clone();
                    editor.on_save(move |opacity, theme, accent| {
                        let prefix = format!("widget_instance_{instance_key}");
                        let result = db::set_settings(
                            &mut state.borrow_mut().conn,
                            &[
                                (&format!("{prefix}_opacity"), &opacity.to_string()),
                                (&format!("{prefix}_theme"), &theme.to_string()),
                                (&format!("{prefix}_accent"), &accent.to_string()),
                            ],
                        );
                        if let Some(ui) = ui_weak.upgrade() {
                            match result {
                                Ok(()) => {
                                    preview_committed.set(true);
                                    let (effective_opacity, effective_theme, effective_accent) =
                                        desktop_widget_style(&state.borrow().conn, &instance_key);
                                    apply_desktop_widget_style(
                                        &instance_key,
                                        effective_opacity,
                                        effective_theme,
                                        effective_accent,
                                        ui.get_visual_theme(),
                                        ui.get_theme_index(),
                                    );
                                    ui.set_action_message("已保存这张卡片的独立外观".into());
                                    if let Some(editor) = editor_weak.upgrade() {
                                        let _ = editor.hide();
                                    }
                                }
                                Err(error) => ui.set_action_message(
                                    format!("保存卡片外观失败：{error}").into(),
                                ),
                            }
                        }
                    });
                }
                let (anchor_x, anchor_y, anchor_width, anchor_height) =
                    desktop_widget_rect(&instance_key).unwrap_or_else(|| {
                        let position = ui.window().position();
                        let size = ui.window().size();
                        (
                            position.x,
                            position.y,
                            size.width as i32,
                            size.height as i32,
                        )
                    });
                editor.window().set_position(slint::PhysicalPosition::new(
                    (anchor_x + (anchor_width - 320) / 2).max(8),
                    (anchor_y + (anchor_height - 228) / 2).max(8),
                ));
                let _ = editor.show();
                *desktop_widgets.appearance_editor.borrow_mut() = Some(editor);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let desktop_widgets = desktop_widgets.clone();
        desktop_widgets.weather.on_set_city(move |city| {
            let city = city.trim();
            if city.is_empty() {
                return;
            }
            if let Some(ui) = ui_weak.upgrade() {
                ui.invoke_set_weather_city(city.into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_open_main_view(move |kind, id, occurrence_date| {
            let mode = match kind.as_str() {
                "calendar" => 0,
                "todo" => 4,
                "countdown" => 8,
                "focus" => 6,
                _ => 2,
            };
            state.borrow_mut().view_mode = mode;
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                set_main_view_mode(&ui, mode);
                refresh_all(&ui, &widget, &state);
                show_and_focus_main_window(&ui);
                if kind.as_str() == "event" && id > 0 {
                    ui.set_event_open_date_hint(occurrence_date);
                    ui.invoke_open_event(id);
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_focus_toggle(move || {
            {
                let mut s = state.borrow_mut();
                let now = Instant::now();
                if s.pomodoro_end_at.is_some() {
                    s.pomodoro_remaining_secs = pomodoro_seconds(&s, now);
                    s.pomodoro_end_at = None;
                } else {
                    if s.pomodoro_remaining_secs == 0 {
                        s.pomodoro_remaining_secs = s.pomodoro_total_secs;
                    }
                    s.pomodoro_end_at = Some(now + Duration::from_secs(s.pomodoro_remaining_secs));
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                update_tool_status(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_focus_start(move || {
            {
                let mut s = state.borrow_mut();
                if s.pomodoro_remaining_secs == 0 {
                    s.pomodoro_remaining_secs = s.pomodoro_total_secs;
                }
                if s.pomodoro_end_at.is_none() {
                    s.pomodoro_end_at =
                        Some(Instant::now() + Duration::from_secs(s.pomodoro_remaining_secs));
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                update_tool_status(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_focus_complete(move || {
            {
                let mut s = state.borrow_mut();
                s.pomodoro_remaining_secs = 0;
                s.pomodoro_end_at = None;
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                update_tool_status(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        widget.on_add_widget_todo(move || {
            state.borrow_mut().view_mode = 4;
            if let Some(ui) = ui_weak.upgrade() {
                set_main_view_mode(&ui, 4);
                ui.set_todo_add_target("normal".into());
                ui.set_todo_draft_title("".into());
                ui.set_todo_add_open(true);
                show_and_focus_main_window(&ui);
            }
        });
    }
}
