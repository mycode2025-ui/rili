//! 桌面卡片窗口、快速面板及 Windows 任务栏时钟集成。

use crate::presentation::*;
use crate::*;
mod taskbar;
pub(crate) use taskbar::*;

/// 桌面挂件是彼此独立的系统窗口；`WidgetWindow` 仅继续承担现有数据/回调中转，
/// 不再作为用户可见的“挂件集合页”。
#[derive(Clone, Copy)]
pub(crate) struct DesktopWidgetVisibility {
    pub(crate) calendar: bool,
    pub(crate) events: bool,
    pub(crate) countdown: bool,
    pub(crate) clock: bool,
    pub(crate) weather: bool,
    pub(crate) focus: bool,
    pub(crate) todo: bool,
    pub(crate) notes: bool,
}

impl DesktopWidgetVisibility {
    pub(crate) fn any(self) -> bool {
        self.calendar
            || self.events
            || self.countdown
            || self.clock
            || self.weather
            || self.focus
            || self.todo
            || self.notes
    }

    pub(crate) fn set(&mut self, kind: &str, visible: bool) {
        match kind {
            "calendar" => self.calendar = visible,
            "events" => self.events = visible,
            "countdown" => self.countdown = visible,
            "clock" => self.clock = visible,
            "weather" => self.weather = visible,
            "focus" => self.focus = visible,
            "todo" => self.todo = visible,
            "notes" => self.notes = visible,
            _ => {}
        }
    }
}

pub(crate) struct DesktopWidgetWindows {
    pub(crate) calendar: CalendarWidgetWindow,
    pub(crate) events: EventsWidgetWindow,
    pub(crate) event_editor: RefCell<Option<NewEventWindow>>,
    pub(crate) appearance_editor: RefCell<Option<WidgetAppearanceWindow>>,
    pub(crate) countdown: CountdownWidgetWindow,
    pub(crate) clock: ClockWidgetWindow,
    pub(crate) weather: WeatherWidgetWindow,
    pub(crate) focus: FocusWidgetWindow,
    pub(crate) todo: TodoWidgetWindow,
    pub(crate) notes: RefCell<Vec<NotesWidgetWindow>>,
}

impl DesktopWidgetWindows {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            calendar: CalendarWidgetWindow::new()?,
            events: EventsWidgetWindow::new()?,
            event_editor: RefCell::new(None),
            appearance_editor: RefCell::new(None),
            countdown: CountdownWidgetWindow::new()?,
            clock: ClockWidgetWindow::new()?,
            weather: WeatherWidgetWindow::new()?,
            focus: FocusWidgetWindow::new()?,
            todo: TodoWidgetWindow::new()?,
            notes: RefCell::new(Vec::new()),
        })
    }

    pub(crate) fn sync_from(&self, source: &WidgetWindow) {
        self.calendar.set_month_title(source.get_month_title());
        self.calendar.set_days(source.get_days());
        self.calendar.set_selected_day(source.get_selected_day());

        self.events.set_today_events(source.get_today_events());
        self.events
            .set_upcoming_events(source.get_upcoming_events());
        self.events
            .set_current_minutes(source.get_current_minutes());

        self.countdown.set_countdowns(source.get_countdowns());

        self.clock
            .set_current_time_main(source.get_current_time_main());
        self.clock.set_current_seconds(source.get_current_seconds());
        self.clock.set_today_date_text(source.get_today_date_text());
        self.clock
            .set_today_lunar_text(source.get_today_lunar_text());
        self.clock.set_day_progress(source.get_day_progress());
        self.clock
            .set_day_progress_text(source.get_day_progress_text());
        self.clock
            .set_day_remaining_text(source.get_day_remaining_text());

        self.weather.set_city(source.get_weather_city());
        self.weather
            .set_temperature(source.get_weather_temperature());
        self.weather
            .set_description(source.get_weather_description());
        self.weather.set_icon_kind(source.get_weather_icon_kind());
        self.weather.set_updated_text(source.get_weather_updated());
        self.weather.set_forecast(source.get_weather_days());

        self.focus.set_focus_time_text(source.get_focus_time_text());
        self.focus.set_focus_task(source.get_focus_task());
        self.focus.set_focus_running(source.get_focus_running());
        self.focus.set_focus_progress(source.get_focus_progress());
        self.focus.set_focus_round(source.get_focus_round());
        self.focus
            .set_focus_total_minutes(source.get_focus_total_minutes());

        self.todo.set_today_todos(source.get_today_todos());
        self.todo
            .set_todo_completed_count(source.get_todo_completed_count());
        self.sync_note_windows(source);
    }

    pub(crate) fn sync_note_windows(&self, source: &WidgetWindow) {
        let model = source.get_recent_notes();
        let notes: Vec<NoteItem> = (0..model.row_count())
            .filter_map(|index| model.row_data(index))
            .collect();
        let note_ids: HashSet<i32> = notes.iter().map(|note| note.id).collect();
        let mut windows = self.notes.borrow_mut();
        let mut newly_created = Vec::new();

        windows.retain(|window| {
            if note_ids.contains(&window.get_note_id()) {
                true
            } else {
                let _ = window.hide();
                false
            }
        });

        for note in &notes {
            if let Some(window) = windows
                .iter()
                .find(|window| window.get_note_id() == note.id)
            {
                if !window.get_editing_dirty() {
                    window.set_note_title(note.title.clone());
                    window.set_note_content(note.content.clone());
                    window.set_editing_dirty(false);
                }
                continue;
            }

            let Ok(window) = NotesWidgetWindow::new() else {
                error_reporter::report("创建便签桌面卡片失败", &"窗口初始化失败");
                continue;
            };
            window.set_note_id(note.id);
            window.set_note_title(note.title.clone());
            window.set_note_content(note.content.clone());
            window.set_editing_dirty(false);

            // Notes are created dynamically after the initial theme pass. Copy the
            // complete live theme state before showing the card so it never flashes
            // light or remains disconnected from the main window's dark mode.
            {
                let source_theme = source.global::<Theme>();
                let theme_mode = source_theme.get_theme_mode();
                let system_dark = source_theme.get_system_dark();
                let accent = source_theme.get_accent();
                let today_bg = source_theme.get_today_bg();
                let font_delta = source_theme.get_font_delta();
                let density_mode = source_theme.get_density_mode();
                let reduce_motion = source_theme.get_reduce_motion();
                let font_family = source_theme.get_font_family();
                let target_theme = window.global::<Theme>();
                target_theme.set_theme_mode(theme_mode);
                target_theme.set_system_dark(system_dark);
                target_theme.set_accent(accent);
                target_theme.set_today_bg(today_bg);
                target_theme.set_font_delta(font_delta);
                target_theme.set_density_mode(density_mode);
                target_theme.set_reduce_motion(reduce_motion);
                target_theme.set_font_family(font_family);
            }

            let key = format!("note_{}", note.id);
            if let Ok(conn) = db::open() {
                let offset = windows.len() as i32 * 34;
                restore_widget_window(&window, &conn, &key, 1672 + offset, 502 + offset);
                let pinned_key = format!("widget_{key}_pinned");
                let pinned = db::get_setting(&conn, &pinned_key, "0").unwrap_or_default() == "1";
                window.set_pinned(pinned);
                let (opacity, card_theme, card_accent) = desktop_widget_style(&conn, &key);
                let app_theme = db::get_setting(&conn, "visual_theme", "1")
                    .ok()
                    .and_then(|value| value.parse::<i32>().ok())
                    .unwrap_or(1);
                let app_accent = db::get_setting(&conn, "theme", "0")
                    .ok()
                    .and_then(|value| value.parse::<i32>().ok())
                    .unwrap_or(0);
                let target = window.global::<Theme>();
                target.set_widget_opacity(opacity);
                target.set_widget_theme_override(card_theme);
                target.set_widget_accent_override(card_accent);
                target.set_theme_mode(if card_theme == 0 {
                    app_theme
                } else {
                    card_theme
                });
                set_theme_accent(
                    target,
                    if card_accent < 0 {
                        app_accent
                    } else {
                        card_accent
                    },
                );
            }

            {
                let window_weak = window.as_weak();
                window.on_begin_window_drag(move || {
                    if let Some(window) = window_weak.upgrade() {
                        let _ = window
                            .window()
                            .with_winit_window(|native| native.drag_window());
                    }
                });
            }
            {
                let window_weak = window.as_weak();
                let key = key.clone();
                window.on_end_window_drag(move || {
                    if let Some(window) = window_weak.upgrade() {
                        let position = window.window().position();
                        if let Ok(conn) = db::open() {
                            if let Err(error) = db::set_setting(
                                &conn,
                                &format!("widget_{key}_x"),
                                &position.x.to_string(),
                            ) {
                                error_reporter::report("保存便签卡片横向位置失败", &error);
                            }
                            if let Err(error) = db::set_setting(
                                &conn,
                                &format!("widget_{key}_y"),
                                &position.y.to_string(),
                            ) {
                                error_reporter::report("保存便签卡片纵向位置失败", &error);
                            }
                        }
                    }
                });
            }
            {
                let key = key.clone();
                window.on_pin_changed(move |pinned| {
                    if let Ok(conn) = db::open() {
                        if let Err(error) = db::set_setting(
                            &conn,
                            &format!("widget_{key}_pinned"),
                            if pinned { "1" } else { "0" },
                        ) {
                            error_reporter::report("保存便签卡片置顶状态失败", &error);
                        }
                    }
                });
            }
            {
                let source_weak = source.as_weak();
                window.on_create_note_card(move || {
                    if let Some(source) = source_weak.upgrade() {
                        source.invoke_create_widget_note();
                    }
                });
            }
            {
                let source_weak = source.as_weak();
                window.on_save_note(move |id, title, content| {
                    if let Some(source) = source_weak.upgrade() {
                        source.invoke_save_widget_note(id, title, content);
                    }
                });
            }
            {
                let source_weak = source.as_weak();
                window.on_delete_note(move |id| {
                    if let Some(source) = source_weak.upgrade() {
                        source.invoke_delete_widget_note(id);
                    }
                });
            }
            {
                let source_weak = source.as_weak();
                let key = key.clone();
                window.on_open_widget_settings(move |_kind| {
                    if let Some(source) = source_weak.upgrade() {
                        source.invoke_open_widget_settings(key.clone().into());
                    }
                });
            }

            let (visible, click_through) = if let Ok(conn) = db::open() {
                (
                    db::get_setting(&conn, "desktop_widgets_visible", "1").unwrap_or_default()
                        != "0"
                        && db::get_setting(&conn, "widget_notes_visible", "1").unwrap_or_default()
                            != "0",
                    db::get_setting(&conn, "desktop_widgets_click_through", "0")
                        .unwrap_or_default()
                        == "1",
                )
            } else {
                (true, false)
            };
            // 先登记再显示。show() 可能触发一次嵌套的窗口同步；如果此时尚未
            // 登记，同一条便签会被误判为缺失并重复创建。
            let window_weak = window.as_weak();
            windows.push(window);
            if visible {
                newly_created.push((window_weak, click_through));
            }
        }
        drop(windows);

        for (window_weak, click_through) in newly_created {
            if let Some(window) = window_weak.upgrade() {
                let _ = window.show();
            }
            let first = window_weak.clone();
            slint::Timer::single_shot(Duration::from_millis(250), move || {
                if let Some(window) = first.upgrade() {
                    remove_widget_from_taskbar(&window);
                    set_widget_click_through(&window, click_through);
                }
            });
            slint::Timer::single_shot(Duration::from_millis(1000), move || {
                if let Some(window) = window_weak.upgrade() {
                    remove_widget_from_taskbar(&window);
                    set_widget_click_through(&window, click_through);
                }
            });
        }
    }

    pub(crate) fn show_configured(&self, visible: DesktopWidgetVisibility, click_through: bool) {
        macro_rules! apply_visibility {
            ($window:expr, $visible:expr) => {
                if $visible {
                    if !$window.window().is_visible() {
                        let _ = $window.show();
                    }
                } else if $window.window().is_visible() {
                    let _ = $window.hide();
                }
            };
        }
        apply_visibility!(self.calendar, visible.calendar);
        apply_visibility!(self.events, visible.events);
        apply_visibility!(self.countdown, visible.countdown);
        apply_visibility!(self.clock, visible.clock);
        apply_visibility!(self.weather, visible.weather);
        apply_visibility!(self.focus, visible.focus);
        apply_visibility!(self.todo, visible.todo);
        if visible.notes {
            for window in self.notes.borrow().iter() {
                if !window.window().is_visible() {
                    let _ = window.show();
                }
            }
        } else {
            for window in self.notes.borrow().iter() {
                let _ = window.hide();
            }
        }

        // Slint creates the native HWND asynchronously when the event loop
        // begins. Apply TOOLWINDOW on the first event-loop turn, not directly
        // after show(), where with_winit_window() can still return None.
        macro_rules! schedule_taskbar_style {
            ($delay_ms:expr) => {{
                let calendar = self.calendar.as_weak();
                let events = self.events.as_weak();
                let countdown = self.countdown.as_weak();
                let clock = self.clock.as_weak();
                let weather = self.weather.as_weak();
                let focus = self.focus.as_weak();
                let todo = self.todo.as_weak();
                let notes: Vec<_> = self
                    .notes
                    .borrow()
                    .iter()
                    .map(|window| window.as_weak())
                    .collect();
                slint::Timer::single_shot(Duration::from_millis($delay_ms), move || {
                    if visible.calendar {
                        if let Some(window) = calendar.upgrade() {
                            remove_widget_from_taskbar(&window);
                            set_widget_click_through(&window, click_through);
                        }
                    }
                    if visible.events {
                        if let Some(window) = events.upgrade() {
                            remove_widget_from_taskbar(&window);
                            set_widget_click_through(&window, click_through);
                        }
                    }
                    if visible.countdown {
                        if let Some(window) = countdown.upgrade() {
                            remove_widget_from_taskbar(&window);
                            set_widget_click_through(&window, click_through);
                        }
                    }
                    if visible.clock {
                        if let Some(window) = clock.upgrade() {
                            remove_widget_from_taskbar(&window);
                            set_widget_click_through(&window, click_through);
                        }
                    }
                    if visible.weather {
                        if let Some(window) = weather.upgrade() {
                            remove_widget_from_taskbar(&window);
                            set_widget_click_through(&window, click_through);
                        }
                    }
                    if visible.focus {
                        if let Some(window) = focus.upgrade() {
                            remove_widget_from_taskbar(&window);
                            set_widget_click_through(&window, click_through);
                        }
                    }
                    if visible.todo {
                        if let Some(window) = todo.upgrade() {
                            remove_widget_from_taskbar(&window);
                            set_widget_click_through(&window, click_through);
                        }
                    }
                    if visible.notes {
                        for note in &notes {
                            if let Some(window) = note.upgrade() {
                                remove_widget_from_taskbar(&window);
                                set_widget_click_through(&window, click_through);
                            }
                        }
                    }
                });
            }};
        }
        schedule_taskbar_style!(250);
        schedule_taskbar_style!(1000);
    }

    pub(crate) fn hide_all(&self) {
        let _ = self.calendar.hide();
        let _ = self.events.hide();
        if let Some(editor) = self.event_editor.borrow().as_ref() {
            let _ = editor.hide();
        }
        if let Some(editor) = self.appearance_editor.borrow().as_ref() {
            let _ = editor.hide();
        }
        let _ = self.countdown.hide();
        let _ = self.clock.hide();
        let _ = self.weather.hide();
        let _ = self.focus.hide();
        let _ = self.todo.hide();
        for window in self.notes.borrow().iter() {
            let _ = window.hide();
        }
    }

    pub(crate) fn apply_click_through(&self, visible: DesktopWidgetVisibility, enabled: bool) {
        if visible.calendar {
            set_widget_click_through(&self.calendar, enabled);
        }
        if visible.events {
            set_widget_click_through(&self.events, enabled);
        }
        if visible.countdown {
            set_widget_click_through(&self.countdown, enabled);
        }
        if visible.clock {
            set_widget_click_through(&self.clock, enabled);
        }
        if visible.weather {
            set_widget_click_through(&self.weather, enabled);
        }
        if visible.focus {
            set_widget_click_through(&self.focus, enabled);
        }
        if visible.todo {
            set_widget_click_through(&self.todo, enabled);
        }
        if visible.notes {
            for window in self.notes.borrow().iter() {
                set_widget_click_through(window, enabled);
            }
        }
    }
}

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
    let size = component.window().size();
    let state = state.borrow();
    if let Err(error) = db::set_setting(
        &state.conn,
        &format!("widget_{kind}_width"),
        &size.width.to_string(),
    ) {
        error_reporter::report(&format!("保存 {kind} 挂件宽度失败"), &error);
    }
    if let Err(error) = db::set_setting(
        &state.conn,
        &format!("widget_{kind}_height"),
        &size.height.to_string(),
    ) {
        error_reporter::report(&format!("保存 {kind} 挂件高度失败"), &error);
    }
}
