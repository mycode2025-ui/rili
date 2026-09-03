use super::*;

pub(crate) fn sync_desktop_visibility_to_ui(ui: &AppWindow, visible: DesktopWidgetVisibility) {
    ui.set_desktop_calendar_visible(visible.calendar);
    ui.set_desktop_events_visible(visible.events);
    ui.set_desktop_countdown_visible(visible.countdown);
    ui.set_desktop_clock_visible(visible.clock);
    ui.set_desktop_weather_visible(visible.weather);
    ui.set_desktop_focus_visible(visible.focus);
    ui.set_desktop_todo_visible(visible.todo);
    ui.set_desktop_notes_visible(visible.notes);
}

thread_local! {
    pub(crate) static DESKTOP_WIDGET_WINDOWS: RefCell<Option<Rc<DesktopWidgetWindows>>> = const { RefCell::new(None) };
}

/// Return the physical rectangle of the exact card that opened a companion
/// window. The main window may be hidden/minimized while desktop cards remain
/// visible, so it must never be used as the positioning anchor here.
pub(crate) fn desktop_widget_rect(instance_key: &str) -> Option<(i32, i32, i32, i32)> {
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        let windows = slot.borrow();
        let windows = windows.as_ref()?;
        let rect = |window: &slint::Window| {
            let position = window.position();
            let size = window.size();
            (
                position.x,
                position.y,
                size.width as i32,
                size.height as i32,
            )
        };
        match instance_key.split(':').next().unwrap_or(instance_key) {
            "calendar" => Some(rect(windows.calendar.window())),
            "events" => Some(rect(windows.events.window())),
            "countdown" => Some(rect(windows.countdown.window())),
            "clock" => Some(rect(windows.clock.window())),
            "weather" => Some(rect(windows.weather.window())),
            "focus" => Some(rect(windows.focus.window())),
            "todo" => Some(rect(windows.todo.window())),
            note_key if note_key.starts_with("note_") => {
                let id = note_key.trim_start_matches("note_").parse::<i32>().ok()?;
                windows
                    .notes
                    .borrow()
                    .iter()
                    .find(|note| note.get_note_id() == id)
                    .map(|note| rect(note.window()))
            }
            _ => None,
        }
    })
}

pub(crate) fn sync_desktop_widgets(source: &WidgetWindow) {
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        if let Some(windows) = slot.borrow().as_ref() {
            windows.sync_from(source);
        }
    });
}

pub(crate) fn open_full_event_editor(ui: &AppWindow, date: NaiveDate) {
    ui.set_editing_event_id(0);
    ui.set_editor_title("".into());
    ui.set_editor_date(date.to_string().into());
    ui.set_editor_time("09:00".into());
    ui.set_editor_reminder(ui.get_default_event_reminder());
    ui.set_editor_repeat("none".into());
    ui.set_editor_note("".into());
    ui.set_editor_calendar_id(1);
    ui.set_editor_calendar_name("默认".into());
    ui.set_editor_duration(60);
    ui.set_editor_error("".into());
    ui.set_editor_occurrence_date(date.to_string().into());
    ui.set_editor_can_delete(false);
    ui.set_editor_is_repeating(false);
    ui.set_new_event_open(true);
    show_and_focus_main_window(ui);
}

/// Open the complete event editor as an independent desktop-card window.
/// A fresh component is created for each use so reminder/repeat/calendar state
/// cannot leak from the previous draft.
pub(crate) fn show_desktop_event_editor(
    windows: &Rc<DesktopWidgetWindows>,
    ui: &AppWindow,
    widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
    date: NaiveDate,
    anchor_x: i32,
    anchor_y: i32,
) {
    if let Some(previous) = windows.event_editor.borrow_mut().take() {
        let _ = previous.hide();
    }

    let Ok(editor) = NewEventWindow::new() else {
        error_reporter::report("创建独立日程编辑窗口失败", &"窗口初始化失败");
        return;
    };
    editor.set_draft_title("".into());
    editor.set_draft_date(date.to_string().into());
    editor.set_draft_time("09:00".into());
    editor.set_draft_reminder(state.borrow().default_event_reminder.clone().into());
    editor.set_draft_repeat("none".into());
    editor.set_draft_note("".into());
    editor.set_draft_calendar_id(1);
    editor.set_draft_calendar_name("默认".into());
    editor.set_draft_duration(60);
    editor.set_validation_message("".into());
    editor.set_calendars(ui.get_calendars());
    editor.set_picker_year(date.year());
    editor.set_picker_month(date.month() as i32);
    editor.set_picker_day(date.day() as i32);
    editor.set_picker_hour(9);
    editor.set_picker_minute(0);

    // Copy the live theme/accessibility state before the new top-level window
    // is shown, avoiding a light flash and keeping it visually identical.
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
        editor.on_begin_window_drag(move || {
            if let Some(editor) = editor_weak.upgrade() {
                let _ = editor
                    .window()
                    .with_winit_window(|native| native.drag_window());
            }
        });
    }
    {
        let editor_weak = editor.as_weak();
        editor.on_close_requested(move || {
            if let Some(editor) = editor_weak.upgrade() {
                let _ = editor.hide();
            }
        });
    }
    {
        let editor_weak = editor.as_weak();
        editor.on_prepare_date_picker(move |value| {
            let date = parse_ui_date(value.as_str()).unwrap_or(date);
            if let Some(editor) = editor_weak.upgrade() {
                editor.set_picker_year(date.year());
                editor.set_picker_month(date.month() as i32);
                editor.set_picker_day(date.day() as i32);
            }
        });
    }
    {
        let editor_weak = editor.as_weak();
        editor.on_prepare_time_picker(move |value| {
            let time = NaiveTime::parse_from_str(value.trim(), "%H:%M")
                .unwrap_or_else(|_| NaiveTime::from_hms_opt(9, 0, 0).unwrap_or(NaiveTime::MIN));
            if let Some(editor) = editor_weak.upgrade() {
                editor.set_picker_hour(time.hour() as i32);
                editor.set_picker_minute(time.minute() as i32);
            }
        });
    }
    {
        let editor_weak = editor.as_weak();
        let state = state.clone();
        editor.on_parse(move |text| {
            if let Some(editor) = editor_weak.upgrade() {
                match natural::parse(text.trim(), Local::now().date_naive()) {
                    Ok(draft) => {
                        let reminder = if draft.reminder.is_empty() {
                            state.borrow().default_event_reminder.clone()
                        } else {
                            draft.reminder
                        };
                        editor.set_draft_title(draft.title.into());
                        editor.set_draft_date(draft.date.into());
                        editor.set_draft_time(draft.time.into());
                        editor.set_draft_reminder(reminder.into());
                        editor.set_validation_message("".into());
                    }
                    Err(error) => {
                        editor.set_validation_message(format!("解析失败：{error}").into())
                    }
                }
            }
        });
    }
    {
        let editor_weak = editor.as_weak();
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        editor.on_save(
            move |title, date, time, reminder, repeat, note, calendar_id, duration| {
                let result = (|| -> Result<()> {
                    let title = title.trim();
                    if title.is_empty() {
                        anyhow::bail!("日程标题不能为空");
                    }
                    let date = parse_ui_date(date.as_str()).map_err(anyhow::Error::msg)?;
                    let time = if time.trim().is_empty() {
                        None
                    } else {
                        Some(parse_widget_time(time.as_str())?)
                    };
                    let repeat = match repeat.as_str() {
                        "daily" | "weekly" | "monthly" | "yearly" => repeat.as_str(),
                        _ => "none",
                    };
                    let s = state.borrow();
                    let calendar_id = db::list_calendars(&s.conn)?
                        .into_iter()
                        .find(|calendar| calendar.id == calendar_id as i64)
                        .map(|calendar| calendar.id)
                        .unwrap_or(1);
                    db::create_event(
                        &s.conn,
                        db::NewEvent {
                            title,
                            date,
                            time: time.as_deref(),
                            note: note.trim(),
                            repeat_rule: repeat,
                            reminder_offsets: reminder.trim(),
                            category: "event",
                            calendar_id,
                        },
                    )?;
                    let created_id = s.conn.last_insert_rowid();
                    if duration != 60 {
                        db::update_event(
                            &s.conn,
                            created_id,
                            db::EventUpdate {
                                duration_minutes: Some(duration.max(1) as i64),
                                ..db::EventUpdate::default()
                            },
                        )?;
                    }
                    Ok(())
                })();

                if let Some(editor) = editor_weak.upgrade() {
                    match &result {
                        Ok(()) => {
                            editor.set_validation_message("".into());
                            let _ = editor.hide();
                        }
                        Err(error) => {
                            editor.set_validation_message(format!("保存失败：{error}").into())
                        }
                    }
                }
                if result.is_ok() {
                    if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                        ui.set_action_message("日程已创建".into());
                        refresh_all(&ui, &widget, &state);
                    }
                }
            },
        );
    }

    editor.window().set_position(slint::PhysicalPosition::new(
        (anchor_x - 88).max(0),
        (anchor_y - 108).max(0),
    ));
    if let Err(error) = editor.show() {
        error_reporter::report("显示独立日程编辑窗口失败", &error);
        return;
    }
    let _ = editor
        .window()
        .with_winit_window(|native| native.focus_window());
    *windows.event_editor.borrow_mut() = Some(editor);
}

pub(crate) fn setting_i32(conn: &Connection, key: &str, default: i32) -> i32 {
    db::get_setting(conn, key, &default.to_string())
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

pub(crate) fn restore_widget_window<C: ComponentHandle>(
    component: &C,
    conn: &Connection,
    kind: &str,
    default_x: i32,
    default_y: i32,
) {
    let x = setting_i32(conn, &format!("widget_{kind}_x"), default_x);
    let y = setting_i32(conn, &format!("widget_{kind}_y"), default_y);
    component
        .window()
        .set_position(slint::PhysicalPosition::new(x, y));
}

pub(crate) fn restore_widget_window_size<C: ComponentHandle>(
    component: &C,
    conn: &Connection,
    kind: &str,
) {
    let logical_width = db::get_setting(conn, &format!("widget_{kind}_logical_width"), "")
        .ok()
        .and_then(|value| value.parse::<f32>().ok());
    let logical_height = db::get_setting(conn, &format!("widget_{kind}_logical_height"), "")
        .ok()
        .and_then(|value| value.parse::<f32>().ok());
    if let (Some(width), Some(height)) = (logical_width, logical_height) {
        component
            .window()
            .set_size(slint::LogicalSize::new(width, height));
        return;
    }

    // 兼容旧版本保存的物理像素；首次拖动或缩放后会迁移到逻辑尺寸键。
    let width = db::get_setting(conn, &format!("widget_{kind}_width"), "")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let height = db::get_setting(conn, &format!("widget_{kind}_height"), "")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    if let (Some(width), Some(height)) = (width, height) {
        component
            .window()
            .set_size(slint::PhysicalSize::new(width, height));
    }
}

pub(crate) fn save_widget_window_size_to_conn<C: ComponentHandle>(
    component: &C,
    conn: &Connection,
    kind: &str,
) -> Result<()> {
    let size = component.window().size();
    let (logical_width, logical_height) = rili::window_policy::physical_to_logical_size(
        size.width,
        size.height,
        component.window().scale_factor(),
    );
    db::set_setting(
        conn,
        &format!("widget_{kind}_logical_width"),
        &format!("{logical_width:.2}"),
    )?;
    db::set_setting(
        conn,
        &format!("widget_{kind}_logical_height"),
        &format!("{logical_height:.2}"),
    )?;
    Ok(())
}

pub(crate) fn save_widget_window_position<C: ComponentHandle>(
    component: &C,
    state: &Rc<RefCell<AppState>>,
    kind: &str,
) {
    let position = component.window().position();
    let state = state.borrow();
    if let Err(error) = db::set_setting(
        &state.conn,
        &format!("widget_{kind}_x"),
        &position.x.to_string(),
    ) {
        error_reporter::report(&format!("保存 {kind} 挂件横坐标失败"), &error);
    }
    if let Err(error) = db::set_setting(
        &state.conn,
        &format!("widget_{kind}_y"),
        &position.y.to_string(),
    ) {
        error_reporter::report(&format!("保存 {kind} 挂件纵坐标失败"), &error);
    }
}

pub(crate) fn save_widget_window_size<C: ComponentHandle>(
    component: &C,
    state: &Rc<RefCell<AppState>>,
    kind: &str,
) {
    let state = state.borrow();
    if let Err(error) = save_widget_window_size_to_conn(component, &state.conn, kind) {
        error_reporter::report(&format!("保存 {kind} 挂件逻辑尺寸失败"), &error);
    }
}
