//! 桌面卡片窗口、快速面板及 Windows 任务栏时钟集成。

use crate::presentation::*;
use crate::*;
mod operations;
mod taskbar;
pub(crate) use operations::*;
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

    pub(crate) fn set_card_locked(&self, instance_key: &str, locked: bool) -> bool {
        match instance_key.split(':').next().unwrap_or(instance_key) {
            "calendar" => self.calendar.set_locked(locked),
            "events" => self.events.set_locked(locked),
            "countdown" => self.countdown.set_locked(locked),
            "clock" => self.clock.set_locked(locked),
            "weather" => self.weather.set_locked(locked),
            "focus" => self.focus.set_locked(locked),
            "todo" => self.todo.set_locked(locked),
            key if key.starts_with("note_") => {
                let Some(id) = key.trim_start_matches("note_").parse::<i32>().ok() else {
                    return false;
                };
                let notes = self.notes.borrow();
                let Some(note) = notes.iter().find(|note| note.get_note_id() == id) else {
                    return false;
                };
                note.set_locked(locked);
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn card_pinned(&self, instance_key: &str) -> bool {
        match instance_key.split(':').next().unwrap_or(instance_key) {
            "calendar" => self.calendar.get_pinned(),
            "events" => self.events.get_pinned(),
            "countdown" => self.countdown.get_pinned(),
            "clock" => self.clock.get_pinned(),
            "weather" => self.weather.get_pinned(),
            "focus" => self.focus.get_pinned(),
            "todo" => self.todo.get_pinned(),
            key if key.starts_with("note_") => key
                .trim_start_matches("note_")
                .parse::<i32>()
                .ok()
                .and_then(|id| {
                    self.notes
                        .borrow()
                        .iter()
                        .find(|note| note.get_note_id() == id)
                        .map(|note| note.get_pinned())
                })
                .unwrap_or(false),
            _ => false,
        }
    }

    pub(crate) fn set_card_pinned(&self, instance_key: &str, pinned: bool) -> bool {
        match instance_key.split(':').next().unwrap_or(instance_key) {
            "calendar" => self.calendar.set_pinned(pinned),
            "events" => self.events.set_pinned(pinned),
            "countdown" => self.countdown.set_pinned(pinned),
            "clock" => self.clock.set_pinned(pinned),
            "weather" => self.weather.set_pinned(pinned),
            "focus" => self.focus.set_pinned(pinned),
            "todo" => self.todo.set_pinned(pinned),
            key if key.starts_with("note_") => {
                let Some(id) = key.trim_start_matches("note_").parse::<i32>().ok() else {
                    return false;
                };
                let notes = self.notes.borrow();
                let Some(note) = notes.iter().find(|note| note.get_note_id() == id) else {
                    return false;
                };
                note.set_pinned(pinned);
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn reset_card_size(&self, instance_key: &str) -> bool {
        let (width, height) = match instance_key.split(':').next().unwrap_or(instance_key) {
            "calendar" => (304.0, 280.0),
            "events" => (304.0, 280.0),
            "countdown" => (304.0, 280.0),
            "clock" => (304.0, 190.0),
            "weather" => (304.0, 244.0),
            "focus" => (304.0, 218.0),
            "todo" => (304.0, 226.0),
            key if key.starts_with("note_") => (304.0, 280.0),
            _ => return false,
        };
        let size = slint::LogicalSize::new(width, height);
        match instance_key.split(':').next().unwrap_or(instance_key) {
            "calendar" => self.calendar.window().set_size(size),
            "events" => self.events.window().set_size(size),
            "countdown" => self.countdown.window().set_size(size),
            "clock" => self.clock.window().set_size(size),
            "weather" => self.weather.window().set_size(size),
            "focus" => self.focus.window().set_size(size),
            "todo" => self.todo.window().set_size(size),
            key if key.starts_with("note_") => {
                let Some(id) = key.trim_start_matches("note_").parse::<i32>().ok() else {
                    return false;
                };
                let notes = self.notes.borrow();
                let Some(note) = notes.iter().find(|note| note.get_note_id() == id) else {
                    return false;
                };
                note.window().set_size(size);
            }
            _ => return false,
        }
        true
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
        self.weather
            .set_provider_text(source.get_weather_provider());
        self.weather.set_refreshing(source.get_weather_refreshing());
        self.weather
            .set_refresh_error(source.get_weather_refresh_error());
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

    /// Update only time-sensitive fields, and only on cards that are actually
    /// visible. Static models continue to use `sync_from` after data changes.
    pub(crate) fn sync_realtime_from(&self, source: &WidgetWindow) {
        if self.events.window().is_visible() {
            self.events
                .set_current_minutes(source.get_current_minutes());
        }
        if self.clock.window().is_visible() {
            self.clock
                .set_current_time_main(source.get_current_time_main());
            self.clock.set_current_seconds(source.get_current_seconds());
            self.clock.set_day_progress(source.get_day_progress());
            self.clock
                .set_day_progress_text(source.get_day_progress_text());
            self.clock
                .set_day_remaining_text(source.get_day_remaining_text());
        }
        if self.focus.window().is_visible() {
            self.focus.set_focus_time_text(source.get_focus_time_text());
            self.focus.set_focus_running(source.get_focus_running());
            self.focus.set_focus_progress(source.get_focus_progress());
            self.focus.set_focus_round(source.get_focus_round());
            self.focus
                .set_focus_total_minutes(source.get_focus_total_minutes());
        }
    }

    pub(crate) fn sync_weather_from(&self, source: &WidgetWindow) {
        if !self.weather.window().is_visible() {
            return;
        }
        self.weather.set_city(source.get_weather_city());
        self.weather
            .set_temperature(source.get_weather_temperature());
        self.weather
            .set_description(source.get_weather_description());
        self.weather.set_icon_kind(source.get_weather_icon_kind());
        self.weather.set_updated_text(source.get_weather_updated());
        self.weather
            .set_provider_text(source.get_weather_provider());
        self.weather.set_refreshing(source.get_weather_refreshing());
        self.weather
            .set_refresh_error(source.get_weather_refresh_error());
        self.weather.set_forecast(source.get_weather_days());
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
                restore_widget_window_size(&window, &conn, &key);
                let pinned_key = format!("widget_{key}_pinned");
                let pinned = db::get_setting(&conn, &pinned_key, "0").unwrap_or_default() == "1";
                window.set_pinned(pinned);
                window.set_locked(
                    db::get_setting(&conn, &format!("widget_instance_{key}_locked"), "0")
                        .unwrap_or_default()
                        == "1",
                );
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
                use slint::winit_030::winit::window::ResizeDirection;
                let window_weak = window.as_weak();
                window.on_begin_window_resize(move || {
                    if let Some(window) = window_weak.upgrade() {
                        if window.get_locked() {
                            return;
                        }
                        let _ = window.window().with_winit_window(|native| {
                            let _ = native.drag_resize_window(ResizeDirection::SouthEast);
                        });
                    }
                });
            }
            {
                let window_weak = window.as_weak();
                let key = key.clone();
                window.on_end_window_resize(move || {
                    if let Some(window) = window_weak.upgrade() {
                        if let Ok(conn) = db::open() {
                            if let Err(error) =
                                save_widget_window_size_to_conn(&window, &conn, &key)
                            {
                                error_reporter::report("保存便签卡片逻辑尺寸失败", &error);
                            }
                        }
                    }
                });
            }

            {
                let window_weak = window.as_weak();
                window.on_begin_window_drag(move || {
                    if let Some(window) = window_weak.upgrade() {
                        if window.get_locked() {
                            return;
                        }
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
                            if let Err(error) =
                                save_widget_window_size_to_conn(&window, &conn, &key)
                            {
                                error_reporter::report("保存便签卡片跨屏尺寸失败", &error);
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
