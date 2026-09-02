#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! GUI 入口：装配 Slint 主窗口 + 桌面挂件窗口、把 SQLite 数据渲染到日历网格 / 侧边栏，
//! 并把 UI 回调接回数据库读写；同时负责启动后台提醒线程和系统托盘图标。
//! 命令行子命令入口见 `cli.rs`。

mod almanac;
mod app_paths;
mod cli;
mod date_calc;
mod db;
mod holidays;
mod ics;
mod integrations;
mod lunar;
mod natural;
mod recurrence;
mod reminders;
mod share;
mod sync;
mod weather;

slint::include_modules!();

use anyhow::{Context, Result};
use chrono::{Datelike, Local, NaiveDate, NaiveTime, Timelike};
use clap::Parser;
use rusqlite::Connection;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// 新建分类日历时依次挑选的设计规范强调色循环。
const CALENDAR_COLOR_CYCLE: [&str; 8] = [
    "#2e6be6", "#0e9f6e", "#c77700", "#d93a49", "#7c4dff", "#0891b2", "#db2777", "#65a30d",
];

/// 把 "#RRGGBB" 解析成 Slint 颜色；解析失败时回退成中性灰，保证不会因为脏数据崩溃。
fn parse_hex_color(hex: &str) -> slint::Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&hex[0..2], 16),
            u8::from_str_radix(&hex[2..4], 16),
            u8::from_str_radix(&hex[4..6], 16),
        ) {
            return slint::Color::from_rgb_u8(r, g, b);
        }
    }
    slint::Color::from_rgb_u8(0x9a, 0x9a, 0x9a)
}

fn main() -> Result<()> {
    let gui_launch = std::env::args_os().nth(1).is_none();
    // 保持 CONSOLE 子系统以保留 CLI 输出；只有无参数桌面启动才立即脱离控制台。
    // 从资源管理器/快捷方式启动时，Windows 创建的附带黑窗会随之关闭；从终端启动
    // GUI 时只分离当前进程，不会隐藏或关闭调用方终端。
    #[cfg(target_os = "windows")]
    if gui_launch {
        detach_console_for_gui();
    }
    let cli = cli::Cli::parse();
    if cli.command.is_some() {
        return cli::run(cli);
    }
    run_gui()
}

#[cfg(target_os = "windows")]
fn detach_console_for_gui() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn FreeConsole() -> i32;
    }
    unsafe {
        FreeConsole();
    }
}

struct AppState {
    conn: Connection,
    year: i32,
    month: u32, // 1..=12
    selected_day: u32,
    week_starts_sunday: bool,
    show_week_numbers: bool,
    /// 0=月视图 1=周视图 2=日视图 3=三日视图 4=待办 5=年视图 6=工具 7=搜索 8=记录 9=排班 10=设置 11=便签 12=课程表
    view_mode: i32,
    /// 0=看板 1=四象限（仅在 view_mode==4 时使用）
    todo_board_mode: i32,
    calculator_start: String,
    calculator_end: String,
    calculator_offset: String,
    calculator_result: String,
    stopwatch_elapsed_secs: u64,
    stopwatch_started_at: Option<Instant>,
    pomodoro_remaining_secs: u64,
    pomodoro_total_secs: u64,
    pomodoro_end_at: Option<Instant>,
    focus_task: String,
    focus_round: i32,
    search_query: String,
    shift_start_date: String,
    shift_end_date: String,
    shift_sequence: String,
    shift_result: String,
    ai_input: String,
    ai_draft: String,
    ai_draft_title: String,
    ai_draft_date: String,
    ai_draft_time: String,
    ai_draft_reminder: String,
    default_event_reminder: String,
    subscription_name: String,
    subscription_url: String,
    /// 周视图当前显示的一周里的任意一天（用于计算这一周的起止日期）；与 (year, month, selected_day)
    /// 分开维护，因为一周可能跨两个月，导航时两者需要保持同步但含义不同。
    week_anchor: NaiveDate,
    /// 日/三日视图当前显示的第一天。
    timeline_anchor: NaiveDate,
    /// 课程表学期第一周的周一，以及当前正在浏览的周次。
    course_term_start: NaiveDate,
    course_week: i32,
}

/// 桌面挂件是彼此独立的系统窗口；`WidgetWindow` 仅继续承担现有数据/回调中转，
/// 不再作为用户可见的“挂件集合页”。
#[derive(Clone, Copy)]
struct DesktopWidgetVisibility {
    calendar: bool,
    events: bool,
    countdown: bool,
    clock: bool,
    focus: bool,
    todo: bool,
    notes: bool,
}

impl DesktopWidgetVisibility {
    fn any(self) -> bool {
        self.calendar
            || self.events
            || self.countdown
            || self.clock
            || self.focus
            || self.todo
            || self.notes
    }

    fn set(&mut self, kind: &str, visible: bool) {
        match kind {
            "calendar" => self.calendar = visible,
            "events" => self.events = visible,
            "countdown" => self.countdown = visible,
            "clock" => self.clock = visible,
            "focus" => self.focus = visible,
            "todo" => self.todo = visible,
            "notes" => self.notes = visible,
            _ => {}
        }
    }
}

struct DesktopWidgetWindows {
    calendar: CalendarWidgetWindow,
    events: EventsWidgetWindow,
    event_editor: RefCell<Option<NewEventWindow>>,
    countdown: CountdownWidgetWindow,
    clock: ClockWidgetWindow,
    focus: FocusWidgetWindow,
    todo: TodoWidgetWindow,
    notes: RefCell<Vec<NotesWidgetWindow>>,
}

impl DesktopWidgetWindows {
    fn new() -> Result<Self> {
        Ok(Self {
            calendar: CalendarWidgetWindow::new()?,
            events: EventsWidgetWindow::new()?,
            event_editor: RefCell::new(None),
            countdown: CountdownWidgetWindow::new()?,
            clock: ClockWidgetWindow::new()?,
            focus: FocusWidgetWindow::new()?,
            todo: TodoWidgetWindow::new()?,
            notes: RefCell::new(Vec::new()),
        })
    }

    fn sync_from(&self, source: &WidgetWindow) {
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

    fn sync_note_windows(&self, source: &WidgetWindow) {
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
                eprintln!("创建便签桌面卡片失败");
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
                let target_theme = window.global::<Theme>();
                target_theme.set_theme_mode(theme_mode);
                target_theme.set_system_dark(system_dark);
                target_theme.set_accent(accent);
                target_theme.set_today_bg(today_bg);
                target_theme.set_font_delta(font_delta);
                target_theme.set_density_mode(density_mode);
                target_theme.set_reduce_motion(reduce_motion);
            }

            let key = format!("note_{}", note.id);
            if let Ok(conn) = db::open() {
                let offset = windows.len() as i32 * 34;
                restore_widget_window(&window, &conn, &key, 1672 + offset, 502 + offset);
                let pinned_key = format!("widget_{key}_pinned");
                let pinned = db::get_setting(&conn, &pinned_key, "0").unwrap_or_default() == "1";
                window.set_pinned(pinned);
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
                            let _ = db::set_setting(
                                &conn,
                                &format!("widget_{key}_x"),
                                &position.x.to_string(),
                            );
                            let _ = db::set_setting(
                                &conn,
                                &format!("widget_{key}_y"),
                                &position.y.to_string(),
                            );
                        }
                    }
                });
            }
            {
                let key = key.clone();
                window.on_pin_changed(move |pinned| {
                    if let Ok(conn) = db::open() {
                        let _ = db::set_setting(
                            &conn,
                            &format!("widget_{key}_pinned"),
                            if pinned { "1" } else { "0" },
                        );
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
                window.on_open_widget_settings(move |kind| {
                    if let Some(source) = source_weak.upgrade() {
                        source.invoke_open_widget_settings(kind);
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

    fn show_configured(&self, visible: DesktopWidgetVisibility, click_through: bool) {
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

    fn hide_all(&self) {
        let _ = self.calendar.hide();
        let _ = self.events.hide();
        if let Some(editor) = self.event_editor.borrow().as_ref() {
            let _ = editor.hide();
        }
        let _ = self.countdown.hide();
        let _ = self.clock.hide();
        let _ = self.focus.hide();
        let _ = self.todo.hide();
        for window in self.notes.borrow().iter() {
            let _ = window.hide();
        }
    }

    fn apply_click_through(&self, visible: DesktopWidgetVisibility, enabled: bool) {
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

fn sync_desktop_visibility_to_ui(ui: &AppWindow, visible: DesktopWidgetVisibility) {
    ui.set_desktop_calendar_visible(visible.calendar);
    ui.set_desktop_events_visible(visible.events);
    ui.set_desktop_countdown_visible(visible.countdown);
    ui.set_desktop_clock_visible(visible.clock);
    ui.set_desktop_focus_visible(visible.focus);
    ui.set_desktop_todo_visible(visible.todo);
    ui.set_desktop_notes_visible(visible.notes);
}

thread_local! {
    static DESKTOP_WIDGET_WINDOWS: RefCell<Option<Rc<DesktopWidgetWindows>>> = const { RefCell::new(None) };
}

fn sync_desktop_widgets(source: &WidgetWindow) {
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        if let Some(windows) = slot.borrow().as_ref() {
            windows.sync_from(source);
        }
    });
}

fn restore_main_window_maximized(ui: &AppWindow, was_maximized: bool) {
    if was_maximized {
        ui.window().set_maximized(true);
    }
}

fn set_main_view_mode(ui: &AppWindow, mode: i32) {
    let was_maximized = ui.window().is_maximized();
    ui.set_view_mode(mode);
    restore_main_window_maximized(ui, was_maximized);
}

fn show_and_focus_main_window(ui: &AppWindow) {
    // Showing the main window must not silently turn a maximized window back
    // into its preferred 1280x820 size. On Windows, calling
    // `set_minimized(false)` unconditionally can perform a restore operation
    // even when the window was not minimized (for example when opening Today
    // from the quick panel or a desktop widget).
    let keep_maximized = ui.window().is_maximized();
    let was_minimized = ui.window().is_minimized();
    let _ = ui.show();
    if was_minimized {
        ui.window().set_minimized(false);
    }
    restore_main_window_maximized(ui, keep_maximized);
    let _ = ui
        .window()
        .with_winit_window(|native| native.focus_window());
}

fn open_full_event_editor(ui: &AppWindow, date: NaiveDate) {
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
    ui.set_new_event_open(true);
    show_and_focus_main_window(ui);
}

/// Open the complete event editor as an independent desktop-card window.
/// A fresh component is created for each use so reminder/repeat/calendar state
/// cannot leak from the previous draft.
fn show_desktop_event_editor(
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
        eprintln!("创建独立日程编辑窗口失败");
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
                .unwrap_or_else(|_| NaiveTime::from_hms_opt(9, 0, 0).unwrap());
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
                        title,
                        date,
                        time.as_deref(),
                        note.trim(),
                        repeat,
                        reminder.trim(),
                        "event",
                        calendar_id,
                    )?;
                    let created_id = s.conn.last_insert_rowid();
                    if duration != 60 {
                        db::update_event(
                            &s.conn,
                            created_id,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            Some(duration.max(1) as i64),
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
        eprintln!("显示独立日程编辑窗口失败：{error}");
        return;
    }
    let _ = editor
        .window()
        .with_winit_window(|native| native.focus_window());
    *windows.event_editor.borrow_mut() = Some(editor);
}

/// Desktop widgets are interactive top-level windows, but they are auxiliary
/// TimeHub surfaces rather than six independent taskbar applications. On
/// Windows, TOOLWINDOW is the native contract for this exact behavior.
#[cfg(target_os = "windows")]
fn remove_widget_from_taskbar<C: ComponentHandle>(component: &C) {
    use slint::winit_030::winit::platform::windows::WindowExtWindows;
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::c_void;

    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
    const WS_EX_APPWINDOW: isize = 0x0004_0000;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_FRAMECHANGED: u32 = 0x0020;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetWindowLongPtrW(hwnd: *mut c_void, index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: *mut c_void, index: i32, value: isize) -> isize;
        fn SetWindowPos(
            hwnd: *mut c_void,
            insert_after: *mut c_void,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
    }

    let applied = component.window().with_winit_window(|native| {
        native.set_skip_taskbar(true);
        let Ok(window_handle) = native.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(window_handle) = window_handle.as_raw() else {
            return;
        };
        let hwnd = window_handle.hwnd.get() as *mut c_void;
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd,
                GWL_EXSTYLE,
                (style | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW,
            );
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    });
    if applied.is_none() {
        eprintln!("桌面挂件 HWND 尚未就绪，等待下一次样式重试");
    }
}

#[cfg(not(target_os = "windows"))]
fn remove_widget_from_taskbar<C: ComponentHandle>(_component: &C) {}

/// Show the quick panel as a real foreground popup. `show()` alone is not
/// sufficient for a non-taskbar, non-pinned tool window: Windows may keep it
/// behind the currently active application after the taskbar clock is clicked.
fn show_and_focus_quick_panel(quick: &QuickPanelWindow) {
    show_and_focus_quick_panel_at(quick, None);
}

fn show_and_focus_quick_panel_at(quick: &QuickPanelWindow, anchor: Option<TaskbarRect>) {
    if let Some(taskbar) = anchor {
        position_quick_panel_at_rect(quick, taskbar);
    } else {
        position_quick_panel_at_taskbar(quick);
    }
    quick.window().set_minimized(false);
    let _ = quick.show();
    remove_widget_from_taskbar(quick);
    let _ = quick
        .window()
        .with_winit_window(|native| native.focus_window());
}

/// 关闭命中测试后，鼠标消息会直接交给挂件下方的窗口。
/// 该状态只能从主窗口的“桌面卡片”管理器恢复，避免卡片本身拦截点击。
fn set_widget_click_through<C: ComponentHandle>(component: &C, enabled: bool) {
    let _ = component.window().with_winit_window(|native| {
        if let Err(error) = native.set_cursor_hittest(!enabled) {
            eprintln!("设置桌面挂件鼠标穿透失败: {error}");
        }
    });
}

#[derive(Clone, Copy)]
struct TaskbarRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    edge: u32,
    scale: f32,
}

#[cfg(target_os = "windows")]
fn windows_taskbar_rect() -> Option<TaskbarRect> {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct AppBarData {
        cb_size: u32,
        hwnd: *mut c_void,
        callback_message: u32,
        edge: u32,
        rect: Rect,
        l_param: isize,
    }

    const ABM_GETTASKBARPOS: u32 = 0x0000_0005;
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHAppBarMessage(message: u32, data: *mut AppBarData) -> usize;
    }

    let mut data = AppBarData {
        cb_size: std::mem::size_of::<AppBarData>() as u32,
        hwnd: std::ptr::null_mut(),
        callback_message: 0,
        edge: 0,
        rect: Rect::default(),
        l_param: 0,
    };
    if unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut data) } == 0 {
        return None;
    }
    Some(TaskbarRect {
        left: data.rect.left,
        top: data.rect.top,
        right: data.rect.right,
        bottom: data.rect.bottom,
        edge: data.edge,
        scale: windows_primary_scale(),
    })
}

#[cfg(not(target_os = "windows"))]
fn windows_taskbar_rect() -> Option<TaskbarRect> {
    None
}

/// Windows 11 在副屏上使用 `Shell_SecondaryTrayWnd`，且时钟通常是 XAML
/// 元素而不是传统 `TrayClockWClass`。枚举所有任务栏顶层窗口，才能让右下角
/// 时间入口在任意显示器上都可用。
#[cfg(target_os = "windows")]
fn windows_taskbar_rects() -> Vec<TaskbarRect> {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    struct MonitorInfo {
        cb_size: u32,
        monitor: Rect,
        work: Rect,
        flags: u32,
    }
    type EnumWindowsProc = Option<unsafe extern "system" fn(*mut c_void, isize) -> i32>;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(callback: EnumWindowsProc, l_param: isize) -> i32;
        fn GetClassNameW(hwnd: *mut c_void, name: *mut u16, count: i32) -> i32;
        fn GetWindowRect(hwnd: *mut c_void, rect: *mut Rect) -> i32;
        fn IsWindowVisible(hwnd: *mut c_void) -> i32;
        fn MonitorFromWindow(hwnd: *mut c_void, flags: u32) -> *mut c_void;
        fn GetMonitorInfoW(monitor: *mut c_void, info: *mut MonitorInfo) -> i32;
        fn GetDpiForWindow(hwnd: *mut c_void) -> u32;
    }

    unsafe extern "system" fn collect(hwnd: *mut c_void, l_param: isize) -> i32 {
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }
        let mut class_name = [0u16; 64];
        let length = unsafe { GetClassNameW(hwnd, class_name.as_mut_ptr(), 64) };
        if length <= 0 {
            return 1;
        }
        let class_name = String::from_utf16_lossy(&class_name[..length as usize]);
        if class_name != "Shell_TrayWnd" && class_name != "Shell_SecondaryTrayWnd" {
            return 1;
        }

        let mut rect = Rect::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
            return 1;
        }
        let monitor = unsafe { MonitorFromWindow(hwnd, 2) }; // MONITOR_DEFAULTTONEAREST
        let mut monitor_info = MonitorInfo {
            cb_size: std::mem::size_of::<MonitorInfo>() as u32,
            monitor: Rect::default(),
            work: Rect::default(),
            flags: 0,
        };
        let has_monitor =
            !monitor.is_null() && unsafe { GetMonitorInfoW(monitor, &mut monitor_info) } != 0;
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let edge = if width >= height {
            if has_monitor
                && (rect.top - monitor_info.monitor.top).abs()
                    <= (monitor_info.monitor.bottom - rect.bottom).abs()
            {
                1 // top
            } else {
                3 // bottom
            }
        } else if has_monitor
            && (rect.left - monitor_info.monitor.left).abs()
                <= (monitor_info.monitor.right - rect.right).abs()
        {
            0 // left
        } else {
            2 // right
        };
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        let taskbars = unsafe { &mut *(l_param as *mut Vec<TaskbarRect>) };
        taskbars.push(TaskbarRect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
            edge,
            scale: dpi as f32 / 96.0,
        });
        1
    }

    let mut taskbars = Vec::new();
    unsafe {
        EnumWindows(
            Some(collect),
            &mut taskbars as *mut Vec<TaskbarRect> as isize,
        );
    }
    taskbars
}

#[cfg(not(target_os = "windows"))]
fn windows_taskbar_rects() -> Vec<TaskbarRect> {
    Vec::new()
}

#[cfg(target_os = "windows")]
fn windows_primary_scale() -> f32 {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDpiForSystem() -> u32;
    }
    (unsafe { GetDpiForSystem() }.max(96) as f32) / 96.0
}

#[cfg(not(target_os = "windows"))]
fn windows_primary_scale() -> f32 {
    1.0
}

fn position_quick_panel_at_taskbar(quick: &QuickPanelWindow) {
    let Some(taskbar) = windows_taskbar_rect() else {
        return;
    };
    position_quick_panel_at_rect(quick, taskbar);
}

fn position_quick_panel_at_rect(quick: &QuickPanelWindow, taskbar: TaskbarRect) {
    let scale = taskbar.scale;
    let panel_width = (460.0 * scale).round() as i32;
    let panel_height = (640.0 * scale).round() as i32;
    let gap = (8.0 * scale).round() as i32;
    let (x, y) = match taskbar.edge {
        0 => (taskbar.right + gap, taskbar.bottom - panel_height - gap),
        1 => (taskbar.right - panel_width - gap, taskbar.bottom + gap),
        2 => (
            taskbar.left - panel_width - gap,
            taskbar.bottom - panel_height - gap,
        ),
        _ => (
            taskbar.right - panel_width - gap,
            taskbar.top - panel_height - gap,
        ),
    };
    quick
        .window()
        .set_position(slint::PhysicalPosition::new(x, y));
}

static TASKBAR_CLOCK_HOOK_ENABLED: AtomicBool = AtomicBool::new(false);
static TASKBAR_CLOCK_CLICKED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "windows")]
const MAX_TASKBAR_CLOCKS: usize = 8;
#[cfg(target_os = "windows")]
struct AtomicTaskbarClockRect {
    left: AtomicI32,
    top: AtomicI32,
    right: AtomicI32,
    bottom: AtomicI32,
    edge: AtomicI32,
    scale_milli: AtomicI32,
}
#[cfg(target_os = "windows")]
impl AtomicTaskbarClockRect {
    const fn new() -> Self {
        Self {
            left: AtomicI32::new(0),
            top: AtomicI32::new(0),
            right: AtomicI32::new(0),
            bottom: AtomicI32::new(0),
            edge: AtomicI32::new(3),
            scale_milli: AtomicI32::new(1000),
        }
    }
}
#[cfg(target_os = "windows")]
static TASKBAR_CLOCK_RECTS: [AtomicTaskbarClockRect; MAX_TASKBAR_CLOCKS] =
    [const { AtomicTaskbarClockRect::new() }; MAX_TASKBAR_CLOCKS];
#[cfg(target_os = "windows")]
static TASKBAR_CLOCK_RECT_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "windows")]
static TASKBAR_CLOCK_CLICKED_INDEX: AtomicUsize = AtomicUsize::new(0);

#[cfg(target_os = "windows")]
fn last_clicked_taskbar_anchor() -> Option<TaskbarRect> {
    let index = TASKBAR_CLOCK_CLICKED_INDEX.load(Ordering::Acquire);
    if index >= TASKBAR_CLOCK_RECT_COUNT.load(Ordering::Acquire) {
        return None;
    }
    let anchor = &TASKBAR_CLOCK_RECTS[index];
    Some(TaskbarRect {
        left: anchor.left.load(Ordering::Relaxed),
        top: anchor.top.load(Ordering::Relaxed),
        right: anchor.right.load(Ordering::Relaxed),
        bottom: anchor.bottom.load(Ordering::Relaxed),
        edge: anchor.edge.load(Ordering::Relaxed).max(0) as u32,
        scale: anchor.scale_milli.load(Ordering::Relaxed).max(1) as f32 / 1000.0,
    })
}

#[cfg(not(target_os = "windows"))]
fn last_clicked_taskbar_anchor() -> Option<TaskbarRect> {
    None
}

/// Keeps the Windows clock itself visible and only takes over its left-click entry.
/// The hit area follows the actual taskbar edge and DPI instead of drawing a covering window.
#[cfg(target_os = "windows")]
fn update_taskbar_clock_hit_rect() {
    let mut taskbars = windows_taskbar_rects();
    if taskbars.is_empty() {
        if let Some(taskbar) = windows_taskbar_rect() {
            taskbars.push(taskbar);
        }
    }
    let count = taskbars.len().min(MAX_TASKBAR_CLOCKS);
    for (index, taskbar) in taskbars.into_iter().take(count).enumerate() {
        let clock_width = (112.0 * taskbar.scale).round() as i32;
        let clock_height = (64.0 * taskbar.scale).round() as i32;
        let horizontal = taskbar.edge == 1 || taskbar.edge == 3;
        let (left, top, right, bottom) = if horizontal {
            (
                (taskbar.right - clock_width).max(taskbar.left),
                taskbar.top,
                taskbar.right,
                taskbar.bottom,
            )
        } else {
            (
                taskbar.left,
                (taskbar.bottom - clock_height).max(taskbar.top),
                taskbar.right,
                taskbar.bottom,
            )
        };
        let target = &TASKBAR_CLOCK_RECTS[index];
        target.left.store(left, Ordering::Relaxed);
        target.top.store(top, Ordering::Relaxed);
        target.right.store(right, Ordering::Relaxed);
        target.bottom.store(bottom, Ordering::Relaxed);
        target.edge.store(taskbar.edge as i32, Ordering::Relaxed);
        target
            .scale_milli
            .store((taskbar.scale * 1000.0).round() as i32, Ordering::Relaxed);
    }
    TASKBAR_CLOCK_RECT_COUNT.store(count, Ordering::Release);
}

#[cfg(target_os = "windows")]
fn spawn_taskbar_clock_click_hook() {
    use std::ffi::c_void;

    const WH_MOUSE_LL: i32 = 14;
    const WM_LBUTTONDOWN: usize = 0x0201;
    const WM_LBUTTONUP: usize = 0x0202;

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[repr(C)]
    struct MouseHookData {
        point: Point,
        mouse_data: u32,
        flags: u32,
        time: u32,
        extra_info: usize,
    }
    #[repr(C)]
    struct Message {
        hwnd: *mut c_void,
        message: u32,
        w_param: usize,
        l_param: isize,
        time: u32,
        point: Point,
        private_data: u32,
    }
    type HookProc = Option<unsafe extern "system" fn(i32, usize, isize) -> isize>;

    unsafe extern "system" fn mouse_proc(code: i32, w_param: usize, l_param: isize) -> isize {
        if code >= 0 && TASKBAR_CLOCK_HOOK_ENABLED.load(Ordering::Relaxed) {
            let data = unsafe { &*(l_param as *const MouseHookData) };
            let count = TASKBAR_CLOCK_RECT_COUNT.load(Ordering::Acquire);
            for index in 0..count {
                let rect = &TASKBAR_CLOCK_RECTS[index];
                let inside = data.point.x >= rect.left.load(Ordering::Relaxed)
                    && data.point.x < rect.right.load(Ordering::Relaxed)
                    && data.point.y >= rect.top.load(Ordering::Relaxed)
                    && data.point.y < rect.bottom.load(Ordering::Relaxed);
                if inside && (w_param == WM_LBUTTONDOWN || w_param == WM_LBUTTONUP) {
                    if w_param == WM_LBUTTONUP {
                        TASKBAR_CLOCK_CLICKED_INDEX.store(index, Ordering::Relaxed);
                        TASKBAR_CLOCK_CLICKED.store(true, Ordering::Release);
                    }
                    return 1;
                }
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param) }
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetWindowsHookExW(
            hook_id: i32,
            callback: HookProc,
            module: *mut c_void,
            thread_id: u32,
        ) -> *mut c_void;
        fn CallNextHookEx(hook: *mut c_void, code: i32, w_param: usize, l_param: isize) -> isize;
        fn UnhookWindowsHookEx(hook: *mut c_void) -> i32;
        fn GetMessageW(message: *mut Message, hwnd: *mut c_void, min: u32, max: u32) -> i32;
        fn TranslateMessage(message: *const Message) -> i32;
        fn DispatchMessageW(message: *const Message) -> isize;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    }

    std::thread::spawn(move || unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
        if hook.is_null() {
            eprintln!("Windows 系统时钟点击接管启动失败");
            return;
        }
        let mut message: Message = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        UnhookWindowsHookEx(hook);
    });
}

#[cfg(not(target_os = "windows"))]
fn update_taskbar_clock_hit_rect() {}

#[cfg(not(target_os = "windows"))]
fn spawn_taskbar_clock_click_hook() {}

fn setting_i32(conn: &Connection, key: &str, default: i32) -> i32 {
    db::get_setting(conn, key, &default.to_string())
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn restore_widget_window<C: ComponentHandle>(
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

fn restore_widget_window_size<C: ComponentHandle>(component: &C, conn: &Connection, kind: &str) {
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

fn save_widget_window_position<C: ComponentHandle>(
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
        eprintln!("保存 {kind} 挂件横坐标失败: {error}");
    }
    if let Err(error) = db::set_setting(
        &state.conn,
        &format!("widget_{kind}_y"),
        &position.y.to_string(),
    ) {
        eprintln!("保存 {kind} 挂件纵坐标失败: {error}");
    }
}

fn save_widget_window_size<C: ComponentHandle>(
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
        eprintln!("保存 {kind} 挂件宽度失败: {error}");
    }
    if let Err(error) = db::set_setting(
        &state.conn,
        &format!("widget_{kind}_height"),
        &size.height.to_string(),
    ) {
        eprintln!("保存 {kind} 挂件高度失败: {error}");
    }
}

fn run_gui() -> Result<()> {
    // 后台提醒扫描线程：独立打开自己的数据库连接，随进程退出而结束。
    reminders::spawn();
    // 后台天气刷新线程：独立数据库连接，30 分钟刷新一次，GUI 只读缓存不会卡界面。
    weather::spawn();
    // 后台 ICS 订阅刷新线程：网络请求与 GUI 解耦，订阅错误只记录到源状态。
    integrations::spawn();

    let conn = db::open()?;
    let today = Local::now().date_naive();

    let week_starts_sunday =
        db::get_setting(&conn, "week_start", "mon").unwrap_or_else(|_| "mon".to_string()) == "sun";
    let show_week_numbers =
        db::get_setting(&conn, "show_week_number", "0").unwrap_or_else(|_| "0".to_string()) == "1";
    let theme_index: i32 = db::get_setting(&conn, "theme", "0")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let visual_theme: i32 = db::get_setting(&conn, "visual_theme", "1")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| (0..=2).contains(v))
        .unwrap_or(1);
    let interface_font_size: i32 = db::get_setting(&conn, "interface_font_size", "13")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(|v: i32| v.clamp(12, 16))
        .unwrap_or(13);
    let interface_density: i32 = db::get_setting(&conn, "interface_density", "1")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(|v: i32| v.clamp(0, 2))
        .unwrap_or(1);
    let reduce_motion = db::get_setting(&conn, "reduce_motion", "0").unwrap_or_default() == "1";
    let notifications_enabled =
        db::get_setting(&conn, "notifications_enabled", "1").unwrap_or_default() != "0";
    let default_event_reminder = db::default_event_reminder(&conn)
        .unwrap_or_else(|_| db::DEFAULT_EVENT_REMINDER.to_string());
    let local_only = db::get_setting(&conn, "local_only", "0").unwrap_or_default() == "1";
    let taskbar_clock_enabled =
        db::get_setting(&conn, "taskbar_clock_enabled", "1").unwrap_or_default() != "0";
    let focus_task = db::get_setting(&conn, "focus_task", "深度工作 · TimeHub UI 实现")
        .unwrap_or_else(|_| "深度工作 · TimeHub UI 实现".to_string());
    let focus_minutes = db::get_setting(&conn, "focus_minutes", "25")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (5..=240).contains(value))
        .unwrap_or(25);
    let default_term_start = week_start_of(today, false);
    let course_term_start =
        db::get_setting(&conn, "course_term_start", &default_term_start.to_string())
            .ok()
            .and_then(|value| NaiveDate::parse_from_str(&value, "%Y-%m-%d").ok())
            .unwrap_or(default_term_start);
    let course_week = course_week_for_date(course_term_start, today);

    let state = Rc::new(RefCell::new(AppState {
        conn,
        year: today.year(),
        month: today.month(),
        selected_day: today.day(),
        week_starts_sunday,
        show_week_numbers,
        view_mode: 0,
        todo_board_mode: 1,
        calculator_start: today.to_string(),
        calculator_end: (today + chrono::Duration::days(1)).to_string(),
        calculator_offset: "10".to_string(),
        calculator_result: String::new(),
        stopwatch_elapsed_secs: 0,
        stopwatch_started_at: None,
        pomodoro_remaining_secs: focus_minutes * 60,
        pomodoro_total_secs: focus_minutes * 60,
        pomodoro_end_at: None,
        focus_task,
        focus_round: 1,
        search_query: String::new(),
        shift_start_date: today.to_string(),
        shift_end_date: (today + chrono::Duration::days(30)).to_string(),
        shift_sequence: "白班,白班,夜班,夜班,休息,休息".to_string(),
        shift_result: String::new(),
        ai_input: String::new(),
        ai_draft: String::new(),
        ai_draft_title: String::new(),
        ai_draft_date: String::new(),
        ai_draft_time: String::new(),
        ai_draft_reminder: String::new(),
        default_event_reminder: default_event_reminder.clone(),
        subscription_name: String::new(),
        subscription_url: String::new(),
        week_anchor: today,
        timeline_anchor: today,
        course_term_start,
        course_week,
    }));

    let ui = AppWindow::new()?;
    let widget = WidgetWindow::new()?;
    let desktop_widgets = Rc::new(DesktopWidgetWindows::new()?);
    desktop_widgets
        .countdown
        .set_create_date((today + chrono::Duration::days(1)).to_string().into());
    desktop_widgets.clock.set_create_time(
        (Local::now() + chrono::Duration::hours(1))
            .format("%H:%M")
            .to_string()
            .into(),
    );
    let quick_panel = QuickPanelWindow::new()?;
    let widgets_visible =
        db::get_setting(&state.borrow().conn, "desktop_widgets_visible", "1")? == "1";
    let widget_shown = Rc::new(Cell::new(widgets_visible));
    let desktop_widget_visibility = Rc::new(RefCell::new(DesktopWidgetVisibility {
        calendar: db::get_setting(&state.borrow().conn, "widget_calendar_visible", "1")? == "1",
        events: db::get_setting(&state.borrow().conn, "widget_events_visible", "1")? == "1",
        countdown: db::get_setting(&state.borrow().conn, "widget_countdown_visible", "1")? == "1",
        clock: db::get_setting(&state.borrow().conn, "widget_clock_visible", "1")? == "1",
        focus: db::get_setting(&state.borrow().conn, "widget_focus_visible", "1")? == "1",
        todo: db::get_setting(&state.borrow().conn, "widget_todo_visible", "1")? == "1",
        notes: db::get_setting(&state.borrow().conn, "widget_notes_visible", "1")? == "1",
    }));
    let desktop_click_through = Rc::new(Cell::new(
        db::get_setting(&state.borrow().conn, "desktop_widgets_click_through", "0")? == "1",
    ));
    widget_shown.set(widgets_visible && desktop_widget_visibility.borrow().any());
    let widget_pinned = db::get_setting(&state.borrow().conn, "widget_pinned", "0")
        .unwrap_or_else(|_| "0".to_string())
        == "1";

    {
        let state = state.borrow();
        restore_widget_window(&desktop_widgets.calendar, &state.conn, "calendar", 88, 58);
        restore_widget_window_size(&desktop_widgets.calendar, &state.conn, "calendar");
        restore_widget_window(&desktop_widgets.events, &state.conn, "events", 616, 58);
        restore_widget_window(
            &desktop_widgets.countdown,
            &state.conn,
            "countdown",
            1144,
            58,
        );
        restore_widget_window(&desktop_widgets.clock, &state.conn, "clock", 88, 502);
        restore_widget_window(&desktop_widgets.focus, &state.conn, "focus", 616, 502);
        restore_widget_window(&desktop_widgets.todo, &state.conn, "todo", 1144, 502);
        // The quick panel is a taskbar flyout, not a freely positioned desktop
        // widget. Always start at the taskbar corner and never restore a stale
        // user-dragged position from older builds.
        position_quick_panel_at_taskbar(&quick_panel);
        quick_panel.set_pinned(db::get_setting(&state.conn, "quick_panel_pinned", "0")? == "1");

        desktop_widgets.calendar.set_pinned(
            db::get_setting(
                &state.conn,
                "widget_calendar_pinned",
                if widget_pinned { "1" } else { "0" },
            )? == "1",
        );
        desktop_widgets.events.set_pinned(
            db::get_setting(
                &state.conn,
                "widget_events_pinned",
                if widget_pinned { "1" } else { "0" },
            )? == "1",
        );
        desktop_widgets.countdown.set_pinned(
            db::get_setting(
                &state.conn,
                "widget_countdown_pinned",
                if widget_pinned { "1" } else { "0" },
            )? == "1",
        );
        desktop_widgets.clock.set_pinned(
            db::get_setting(
                &state.conn,
                "widget_clock_pinned",
                if widget_pinned { "1" } else { "0" },
            )? == "1",
        );
        desktop_widgets.focus.set_pinned(
            db::get_setting(
                &state.conn,
                "widget_focus_pinned",
                if widget_pinned { "1" } else { "0" },
            )? == "1",
        );
        desktop_widgets.todo.set_pinned(
            db::get_setting(
                &state.conn,
                "widget_todo_pinned",
                if widget_pinned { "1" } else { "0" },
            )? == "1",
        );
    }
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        *slot.borrow_mut() = Some(desktop_widgets.clone());
    });

    ui.set_week_starts_sunday(week_starts_sunday);
    ui.set_show_week_numbers(show_week_numbers);
    ui.set_theme_index(theme_index);
    ui.set_visual_theme(visual_theme);
    ui.set_interface_font_size(interface_font_size);
    ui.set_interface_density(interface_density);
    ui.set_reduce_motion(reduce_motion);
    ui.set_notifications_enabled(notifications_enabled);
    ui.set_default_event_reminder(default_event_reminder.into());
    ui.set_local_only(local_only);
    ui.set_taskbar_clock_enabled(taskbar_clock_enabled);
    {
        let visible = *desktop_widget_visibility.borrow();
        sync_desktop_visibility_to_ui(&ui, visible);
    }
    ui.set_desktop_click_through(desktop_click_through.get());
    widget.set_pinned(widget_pinned);
    apply_theme(&ui, &widget, &quick_panel, theme_index);
    apply_visual_theme(&ui, &widget, &quick_panel, visual_theme);
    apply_accessibility_preferences(
        &ui,
        &widget,
        &quick_panel,
        interface_font_size,
        interface_density,
        reduce_motion,
    );

    // The subscription list below is the persistent status display. Keep this
    // transient message empty until an add/sync action produces a real result.
    ui.set_integration_status("".into());
    refresh_all(&ui, &widget, &state);
    let debug_view = if cfg!(debug_assertions) {
        std::env::var("TIMEHUB_DEBUG_VIEW").ok()
    } else {
        None
    };
    if matches!(debug_view.as_deref(), Some("tools" | "tools-then-today")) {
        state.borrow_mut().view_mode = 6;
        ui.set_view_mode(6);
    }
    if debug_view.as_deref() == Some("settings-notifications") {
        ui.set_settings_section(2);
        ui.set_settings_open(true);
        let ui_weak = ui.as_weak();
        slint::Timer::single_shot(Duration::from_millis(1200), move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_action_message("示例提醒已发送，请查看 Windows 通知中心".into());
            }
        });
    }
    if debug_view.as_deref() == Some("tools-then-today") {
        let ui_weak = ui.as_weak();
        slint::Timer::single_shot(Duration::from_millis(2500), move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.invoke_set_view_mode(2);
            }
        });
    }
    sync_quick_panel(&quick_panel, &ui, &widget);
    TASKBAR_CLOCK_HOOK_ENABLED.store(taskbar_clock_enabled, Ordering::Release);
    update_taskbar_clock_hit_rect();
    spawn_taskbar_clock_click_hook();
    if widgets_visible {
        desktop_widgets.sync_from(&widget);
        desktop_widgets.show_configured(
            *desktop_widget_visibility.borrow(),
            desktop_click_through.get(),
        );
        ui.set_widget_visible(desktop_widget_visibility.borrow().any());
    }

    // -------- 无边框标题栏：接入系统拖动/最大化行为；关闭按钮隐藏到托盘 --------
    {
        let ui_weak = ui.as_weak();
        ui.on_begin_window_drag(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let _ = ui.window().with_winit_window(|window| window.drag_window());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_minimize_window(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.window().set_minimized(true);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_maximize_window(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let maximized = ui.window().is_maximized();
                ui.window().set_maximized(!maximized);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_close_window(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let _ = ui.hide();
            }
        });
    }

    // -------- 主窗口：月份导航 / 选中日 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_prev_month(move || {
            shift_month(&state, -1);
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_set_visual_theme(move |mode| {
            let mode = mode.clamp(0, 2);
            {
                let s = state.borrow();
                let _ = db::set_setting(&s.conn, "visual_theme", &mode.to_string());
            }
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                ui.set_visual_theme(mode);
                apply_visual_theme(&ui, &widget, &quick, mode);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_set_interface_font_size(move |font_size| {
            let font_size = font_size.clamp(12, 16);
            {
                let s = state.borrow();
                let _ = db::set_setting(&s.conn, "interface_font_size", &font_size.to_string());
            }
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                ui.set_interface_font_size(font_size);
                apply_accessibility_preferences(
                    &ui,
                    &widget,
                    &quick,
                    font_size,
                    ui.get_interface_density(),
                    ui.get_reduce_motion(),
                );
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_set_interface_density(move |density| {
            let density = density.clamp(0, 2);
            {
                let s = state.borrow();
                let _ = db::set_setting(&s.conn, "interface_density", &density.to_string());
            }
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                ui.set_interface_density(density);
                apply_accessibility_preferences(
                    &ui,
                    &widget,
                    &quick,
                    ui.get_interface_font_size(),
                    density,
                    ui.get_reduce_motion(),
                );
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_set_reduce_motion(move |reduce_motion| {
            {
                let s = state.borrow();
                let _ = db::set_setting(
                    &s.conn,
                    "reduce_motion",
                    if reduce_motion { "1" } else { "0" },
                );
            }
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                ui.set_reduce_motion(reduce_motion);
                apply_accessibility_preferences(
                    &ui,
                    &widget,
                    &quick,
                    ui.get_interface_font_size(),
                    ui.get_interface_density(),
                    reduce_motion,
                );
            }
        });
    }
    // -------- 课程表：周次导航、学期起点与课程增删改 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_change_course_week(move |delta| {
            {
                let mut s = state.borrow_mut();
                s.course_week = (s.course_week + delta).clamp(1, 30);
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_goto_course_current_week(move || {
            {
                let mut s = state.borrow_mut();
                s.course_week =
                    course_week_for_date(s.course_term_start, Local::now().date_naive());
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_set_course_term_start(move |value| {
            let result =
                parse_ui_date(value.trim()).map_err(|_| "请输入有效日期（YYYY-MM-DD）".to_string());
            match result {
                Ok(date) => {
                    {
                        let mut s = state.borrow_mut();
                        s.course_term_start = date;
                        s.course_week = course_week_for_date(date, Local::now().date_naive());
                        if let Err(error) =
                            db::set_setting(&s.conn, "course_term_start", &date.to_string())
                        {
                            return format!("保存开学日失败：{error}").into();
                        }
                    }
                    if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                        ui.set_action_message("课程表开学日已更新".into());
                        refresh_all(&ui, &widget, &state);
                    }
                    SharedString::default()
                }
                Err(message) => message.into(),
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_save_course(
            move |id,
                  title,
                  teacher,
                  location,
                  weekday,
                  start_period,
                  period_count,
                  start_week,
                  end_week,
                  color_index| {
                let result = {
                    let s = state.borrow();
                    db::save_course(
                        &s.conn,
                        id as i64,
                        title.as_str(),
                        teacher.as_str(),
                        location.as_str(),
                        weekday as i64,
                        start_period as i64,
                        period_count as i64,
                        start_week as i64,
                        end_week as i64,
                        color_index as i64,
                    )
                };
                match result {
                    Ok(_) => {
                        if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade())
                        {
                            ui.set_action_message(
                                if id > 0 {
                                    "课程已更新"
                                } else {
                                    "课程已创建"
                                }
                                .into(),
                            );
                            refresh_all(&ui, &widget, &state);
                        }
                        SharedString::default()
                    }
                    Err(error) => error.to_string().into(),
                }
            },
        );
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_delete_course(move |id| {
            let result = {
                let s = state.borrow();
                db::delete_course(&s.conn, id as i64)
            };
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                ui.set_action_message(
                    match result {
                        Ok(_) => "课程已删除".to_string(),
                        Err(error) => format!("删除课程失败：{error}"),
                    }
                    .into(),
                );
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_set_notifications_enabled(move |enabled| {
            let s = state.borrow();
            let _ = db::set_setting(
                &s.conn,
                "notifications_enabled",
                if enabled { "1" } else { "0" },
            );
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_notifications_enabled(enabled);
                ui.set_action_message(
                    if enabled {
                        "通知已启用"
                    } else {
                        "通知已停用"
                    }
                    .into(),
                );
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_set_default_event_reminder(move |value| {
            let value = value.trim().to_string();
            let result = {
                let mut s = state.borrow_mut();
                match db::apply_default_event_reminder(&s.conn, &value) {
                    Ok(changed) => {
                        s.default_event_reminder = value.clone();
                        Ok(changed)
                    }
                    Err(error) => Err(error),
                }
            };
            if let Some(ui) = ui_weak.upgrade() {
                match result {
                    Ok(changed) => {
                        ui.set_default_event_reminder(value.clone().into());
                        let label = match value.as_str() {
                            "" => "关闭",
                            "0" => "准时",
                            "10" => "提前10分钟",
                            "60" => "提前1小时",
                            "1440" => "提前1天",
                            _ => "未知",
                        };
                        ui.set_action_message(
                            format!("默认提醒已设为{label}，并更新 {changed} 条现有日程").into(),
                        );
                        if let Some(widget) = widget_weak.upgrade() {
                            refresh_all(&ui, &widget, &state);
                        }
                    }
                    Err(error) => {
                        ui.set_action_message(format!("更新默认提醒失败：{error}").into())
                    }
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_set_taskbar_clock_enabled(move |enabled| {
            {
                let s = state.borrow();
                let _ = db::set_setting(
                    &s.conn,
                    "taskbar_clock_enabled",
                    if enabled { "1" } else { "0" },
                );
            }
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_taskbar_clock_enabled(enabled);
                ui.set_action_message(
                    if enabled {
                        "系统时钟点击接管已开启"
                    } else {
                        "系统时钟点击接管已关闭"
                    }
                    .into(),
                );
            }
            TASKBAR_CLOCK_HOOK_ENABLED.store(enabled, Ordering::Release);
            if enabled {
                update_taskbar_clock_hit_rect();
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_set_local_only(move |enabled| {
            let s = state.borrow();
            let _ = db::set_setting(&s.conn, "local_only", if enabled { "1" } else { "0" });
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_local_only(enabled);
                ui.set_action_message(
                    if enabled {
                        "已切换为仅本地模式"
                    } else {
                        "已允许外部同步"
                    }
                    .into(),
                );
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_settings_action(move |action| {
            let action = action.to_string();
            if let Some(ui) = ui_weak.upgrade() {
                let message = match action.as_str() {
                    "test-notification" => {
                        if !ui.get_notifications_enabled() {
                            "请先开启“系统通知”后再发送示例提醒".to_string()
                        } else {
                            match reminders::send_test_notification() {
                                Ok(()) => "示例提醒已发送，请查看 Windows 通知中心".to_string(),
                                Err(error) => format!("示例提醒发送失败：{error}"),
                            }
                        }
                    }
                    "sync" => {
                        if ui.get_local_only() {
                            "仅本地模式下未执行同步".to_string()
                        } else {
                            ui.invoke_sync_subscriptions();
                            "正在同步外部日历…".to_string()
                        }
                    }
                    "open-integrations" => {
                        state.borrow_mut().view_mode = 6;
                        set_main_view_mode(&ui, 6);
                        "已打开工具与集成配置".to_string()
                    }
                    "open-data-folder" => {
                        let db_path = app_paths::db_path();
                        let directory = db_path.parent().unwrap_or(db_path.as_path());
                        #[cfg(windows)]
                        let result = std::process::Command::new("explorer.exe")
                            .arg(directory)
                            .spawn();
                        #[cfg(not(windows))]
                        let result: std::io::Result<std::process::Child> =
                            Err(std::io::Error::new(
                                std::io::ErrorKind::Unsupported,
                                "当前平台尚未配置文件管理器",
                            ));
                        match result {
                            Ok(_) => "已打开数据目录".to_string(),
                            Err(error) => format!("打开数据目录失败：{error}"),
                        }
                    }
                    "backup" => {
                        let backup_name = format!(
                            "timehub-backup-{}.db",
                            Local::now().format("%Y%m%d-%H%M%S-%3f")
                        );
                        let backup_path = app_paths::db_path().with_file_name(backup_name);
                        let result = state
                            .borrow()
                            .conn
                            .execute("VACUUM INTO ?1", [backup_path.to_string_lossy().as_ref()]);
                        match result {
                            Ok(_) => format!("备份已生成：{}", backup_path.display()),
                            Err(error) => format!("备份失败：{error}"),
                        }
                    }
                    "reload" => {
                        if let Some(widget) = widget_weak.upgrade() {
                            refresh_all(&ui, &widget, &state);
                        }
                        "数据已重新载入".to_string()
                    }
                    "reset-shortcuts" => "快捷键已恢复为默认值".to_string(),
                    "check-update" => "当前为 TimeHub V3；更新服务地址尚未配置".to_string(),
                    "diagnostics" => {
                        let s = state.borrow();
                        let calendars = db::list_calendars(&s.conn).map(|v| v.len()).unwrap_or(0);
                        let events = db::list_all_events(&s.conn).map(|v| v.len()).unwrap_or(0);
                        format!("诊断完成：{calendars} 个日历，{events} 条日程，数据库可读")
                    }
                    _ => "操作已完成".to_string(),
                };
                ui.set_action_message(message.into());
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_toggle_todo(move |id| {
            {
                let s = state.borrow();
                if let Err(e) = db::toggle_todo(&s.conn, id as i64) {
                    eprintln!("挂件更新待办失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_add_note(move |title: SharedString| {
            let title = title.trim().to_string();
            if !title.is_empty() {
                let s = state.borrow();
                if let Err(e) = db::create_note(&s.conn, &title, "") {
                    eprintln!("挂件新建便签失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_create_widget_note(move || {
            {
                let s = state.borrow();
                if let Err(error) = db::create_note(&s.conn, "新便签", "") {
                    eprintln!("新建便签卡片失败: {error}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_save_widget_note(move |id, title, content| {
            let title = if title.trim().is_empty() {
                "新便签"
            } else {
                title.trim()
            };
            {
                let s = state.borrow();
                if let Err(error) = db::update_note(&s.conn, id as i64, title, content.as_str()) {
                    eprintln!("保存便签卡片失败: {error}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        let visibility = desktop_widget_visibility.clone();
        let shown = widget_shown.clone();
        widget.on_delete_widget_note(move |id| {
            let no_notes_left = {
                let s = state.borrow();
                if let Err(error) = db::delete_note(&s.conn, id as i64) {
                    eprintln!("删除便签卡片失败: {error}");
                }
                db::list_notes(&s.conn).unwrap_or_default().is_empty()
            };
            if no_notes_left {
                visibility.borrow_mut().notes = false;
                let configuration = *visibility.borrow();
                shown.set(configuration.any());
                let s = state.borrow();
                let _ = db::set_setting(&s.conn, "widget_notes_visible", "0");
                if let Some(ui) = ui_weak.upgrade() {
                    sync_desktop_visibility_to_ui(&ui, configuration);
                    ui.set_widget_visible(configuration.any());
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_next_month(move || {
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
        ui.on_goto_today(move || {
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
        ui.on_select_day(move |day| {
            state.borrow_mut().selected_day = day.max(1) as u32;
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_open_event(move |id| {
            let (event, calendar_name) = {
                let s = state.borrow();
                let event = db::get_event(&s.conn, id as i64).ok().flatten();
                let calendar_name = event.as_ref().and_then(|event| {
                    db::list_calendars(&s.conn)
                        .ok()?
                        .into_iter()
                        .find(|calendar| calendar.id == event.calendar_id)
                        .map(|calendar| calendar.name)
                });
                (event, calendar_name)
            };
            if let (Some(ui), Some(event)) = (ui_weak.upgrade(), event) {
                ui.set_editing_event_id(id);
                ui.set_editor_title(event.title.into());
                ui.set_editor_date(event.date.into());
                ui.set_editor_time(event.time.unwrap_or_default().into());
                ui.set_editor_reminder(event.reminder_offsets.into());
                ui.set_editor_repeat(event.repeat_rule.into());
                ui.set_editor_note(event.note.into());
                ui.set_editor_calendar_id(event.calendar_id as i32);
                ui.set_editor_calendar_name(
                    calendar_name.unwrap_or_else(|| "默认".to_string()).into(),
                );
                ui.set_editor_duration(event.duration_minutes as i32);
                ui.set_editor_error("".into());
                ui.set_new_event_open(true);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_save_event_dialog(
            move |id, title, date, time, reminder, repeat, note, calendar_id, duration| {
                let result = (|| -> Result<()> {
                    let title = title.trim();
                    if title.is_empty() {
                        anyhow::bail!("日程标题不能为空");
                    }
                    let date = parse_ui_date(date.as_str()).map_err(anyhow::Error::msg)?;
                    let time = (!time.trim().is_empty()).then_some(time.trim().to_string());
                    let repeat = match repeat.as_str() {
                        "daily" | "weekly" | "monthly" | "yearly" => repeat.as_str(),
                        _ => "none",
                    };
                    let calendar_id = {
                        let s = state.borrow();
                        db::list_calendars(&s.conn)?
                            .into_iter()
                            .find(|calendar| calendar.id == calendar_id as i64)
                            .map(|calendar| calendar.id)
                            .unwrap_or(1)
                    };
                    if id > 0 {
                        db::update_event(
                            &state.borrow().conn,
                            id as i64,
                            Some(title),
                            Some(date),
                            Some(time.as_deref()),
                            Some(note.trim()),
                            Some(repeat),
                            Some(reminder.trim()),
                            None,
                            Some(calendar_id),
                            Some(duration.max(1) as i64),
                        )?;
                    } else {
                        db::create_event(
                            &state.borrow().conn,
                            title,
                            date,
                            time.as_deref(),
                            note.trim(),
                            repeat,
                            reminder.trim(),
                            "event",
                            calendar_id,
                        )?;
                        let created_id = state.borrow().conn.last_insert_rowid();
                        if duration != 60 {
                            db::update_event(
                                &state.borrow().conn,
                                created_id,
                                None,
                                None,
                                None,
                                None,
                                None,
                                None,
                                None,
                                None,
                                Some(duration.max(1) as i64),
                            )?;
                        }
                    }
                    Ok(())
                })();
                let succeeded = result.is_ok();
                if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                    match &result {
                        Ok(()) => {
                            ui.set_editor_error("".into());
                            ui.set_action_message(
                                if id > 0 {
                                    "日程已更新"
                                } else {
                                    "日程已创建"
                                }
                                .into(),
                            );
                            refresh_all(&ui, &widget, &state);
                        }
                        Err(error) => {
                            let message = format!("保存失败：{error}");
                            ui.set_editor_error(message.clone().into());
                            ui.set_action_message(message.into());
                        }
                    }
                }
                succeeded
            },
        );
    }

    // -------- 视图切换：月/周/日/三日 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_set_view_mode(move |mode| {
            if mode == 10 {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_settings_open(true);
                }
                return;
            }
            {
                let mut s = state.borrow_mut();
                s.view_mode = mode;
                // 切换周、日、三日视图时，都从当前选中日期开始，避免沿用旧锚点跳到其他年份。
                if let Some(d) = NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day) {
                    if mode == 1 {
                        s.week_anchor = d;
                    }
                    if mode == 2 || mode == 3 {
                        s.timeline_anchor = d;
                    }
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                set_main_view_mode(&ui, mode);
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_prepare_picker_date(move |value| {
            let fallback = {
                let s = state.borrow();
                NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day)
                    .unwrap_or_else(|| Local::now().date_naive())
            };
            let date = parse_ui_date(value.as_str()).unwrap_or(fallback);
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_picker_year(date.year());
                ui.set_picker_month(date.month() as i32);
                ui.set_picker_day(date.day() as i32);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_prepare_picker_time(move |value| {
            let time = NaiveTime::parse_from_str(value.trim(), "%H:%M")
                .unwrap_or_else(|_| NaiveTime::from_hms_opt(9, 0, 0).unwrap());
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_picker_hour(time.hour() as i32);
                ui.set_picker_minute(time.minute() as i32);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_prev_week(move || {
            shift_week(&state, -1);
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_prev_year(move || {
            shift_year(&state, -1);
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_search(move |query| {
            state.borrow_mut().search_query = query.to_string();
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_open_search_result(move |_id, date, kind| {
            let mode = match kind.as_str() {
                "待办" => 4,
                "便签" => 11,
                "课程" => 12,
                _ => 0,
            };
            if mode == 0 {
                if let Ok(selected) = parse_ui_date(date.as_str()) {
                    select_full_date(&state, selected);
                }
            }
            state.borrow_mut().view_mode = mode;
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                set_main_view_mode(&ui, mode);
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_add_record(move |title, date, category| {
            let title = title.trim().to_string();
            let date_text = date.trim().to_string();
            let category = category.to_string();
            if title.is_empty() || date_text.is_empty() {
                return "请填写名称和日期".into();
            }
            let message = match parse_ui_date(&date_text) {
                Ok(date)
                    if matches!(category.as_str(), "birthday" | "anniversary" | "countdown") =>
                {
                    let repeat = if matches!(category.as_str(), "birthday" | "anniversary") {
                        "yearly"
                    } else {
                        "none"
                    };
                    let s = state.borrow();
                    if let Err(e) =
                        db::create_event(&s.conn, &title, date, None, "", repeat, "", &category, 1)
                    {
                        eprintln!("新增长期节点失败: {e}");
                        format!("添加失败：{e}")
                    } else {
                        "已添加".to_string()
                    }
                }
                Ok(_) => "不支持的记录类型".to_string(),
                Err(e) => e,
            };
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
            message.into()
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_delete_record(move |id| {
            {
                let s = state.borrow();
                if let Err(e) = db::delete_event(&s.conn, id as i64) {
                    eprintln!("删除长期节点失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_generate_shift(move |start, end, sequence| {
            let start_text = start.trim().to_string();
            let end_text = end.trim().to_string();
            let sequence_text = sequence.trim().to_string();
            let result = (|| -> Result<usize> {
                let start = parse_ui_date(&start_text).map_err(anyhow::Error::msg)?;
                let end = parse_ui_date(&end_text).map_err(anyhow::Error::msg)?;
                let names: Vec<&str> = sequence_text
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .collect();
                anyhow::ensure!(!names.is_empty(), "请填写班次序列");
                let s = state.borrow();
                let types = db::list_shift_types(&s.conn)?;
                let ids: Vec<i64> = names
                    .iter()
                    .map(|name| {
                        types
                            .iter()
                            .find(|shift| shift.name == *name)
                            .map(|shift| shift.id)
                            .ok_or_else(|| anyhow::anyhow!("找不到班次：{name}"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                db::generate_shift_assignments(&s.conn, start, end, &ids)
            })();
            {
                let mut s = state.borrow_mut();
                s.shift_start_date = start_text;
                s.shift_end_date = end_text;
                s.shift_sequence = sequence_text;
                s.shift_result = match result {
                    Ok(count) => format!("已生成 {count} 天班表"),
                    Err(e) => format!("生成失败：{e}"),
                };
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_delete_shift(move |id| {
            {
                let s = state.borrow();
                if let Err(e) = db::delete_shift_assignment(&s.conn, id as i64) {
                    eprintln!("清空班次失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_next_year(move || {
            shift_year(&state, 1);
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_next_week(move || {
            shift_week(&state, 1);
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_select_date(move |y, m, d| {
            if let Some(date) = NaiveDate::from_ymd_opt(y, m as u32, d as u32) {
                select_full_date(&state, date);
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    // -------- 日/三日视图：上一天(组)/下一天(组)/点空白处新建/拖拽移动/拖拽调整时长 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_prev_timeline(move || {
            shift_timeline(&state, -1);
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_next_timeline(move || {
            shift_timeline(&state, 1);
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_add_event_at(move |column_index, start_minutes| {
            {
                let s = state.borrow();
                let date = s.timeline_anchor + chrono::Duration::days(column_index as i64);
                let hh = start_minutes / 60;
                let mm = start_minutes % 60;
                let time = format!("{hh:02}:{mm:02}");
                if let Err(e) = db::create_event(
                    &s.conn,
                    "新日程",
                    date,
                    Some(&time),
                    "",
                    "none",
                    &s.default_event_reminder,
                    "event",
                    1,
                ) {
                    eprintln!("新建日程失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_event_moved(move |event_id, new_start_minutes| {
            {
                let s = state.borrow();
                if let Ok(Some(event)) = db::get_event(&s.conn, event_id as i64) {
                    let hh = new_start_minutes / 60;
                    let mm = new_start_minutes % 60;
                    let time = format!("{hh:02}:{mm:02}");
                    if let Err(e) = db::update_event(
                        &s.conn,
                        event.id,
                        None,
                        None,
                        Some(Some(&time)),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    ) {
                        eprintln!("拖拽移动日程失败: {e}");
                    }
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_event_resized(move |event_id, new_duration_minutes| {
            {
                let s = state.borrow();
                if let Err(e) = db::update_event(
                    &s.conn,
                    event_id as i64,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(new_duration_minutes as i64),
                ) {
                    eprintln!("拖拽调整时长失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }

    // -------- 待办看板 / 四象限视图 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_set_todo_board_mode(move |mode| {
            state.borrow_mut().todo_board_mode = mode;
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                ui.set_todo_board_mode(mode);
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_move_todo_status(move |id, status| {
            {
                let s = state.borrow();
                if let Err(e) = db::set_todo_status(&s.conn, id as i64, status.as_str()) {
                    eprintln!("移动看板卡片失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_parse_natural(move |text| {
            let input = text.trim().to_string();
            let parsed = natural::parse(&input, Local::now().date_naive());
            {
                let mut s = state.borrow_mut();
                s.ai_input = input;
                match parsed {
                    Ok(draft) => {
                        let reminder = if draft.reminder.is_empty() {
                            s.default_event_reminder.clone()
                        } else {
                            draft.reminder
                        };
                        s.ai_draft = draft.explanation;
                        s.ai_draft_title = draft.title;
                        s.ai_draft_date = draft.date;
                        s.ai_draft_time = draft.time;
                        s.ai_draft_reminder = reminder;
                    }
                    Err(error) => {
                        s.ai_draft = format!("解析失败：{error}");
                        s.ai_draft_title.clear();
                        s.ai_draft_date.clear();
                        s.ai_draft_time.clear();
                        s.ai_draft_reminder.clear();
                    }
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
                let s = state.borrow();
                if !s.ai_draft_title.is_empty() {
                    ui.set_editor_error("".into());
                    ui.set_editor_title(s.ai_draft_title.clone().into());
                    ui.set_editor_date(s.ai_draft_date.clone().into());
                    ui.set_editor_time(s.ai_draft_time.clone().into());
                    ui.set_editor_reminder(s.ai_draft_reminder.clone().into());
                } else if ui.get_new_event_open() {
                    ui.set_editor_error(s.ai_draft.clone().into());
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_create_natural_event(move |title, date, time, reminder| {
            let result = (|| -> Result<()> {
                let date = parse_ui_date(date.as_str()).map_err(anyhow::Error::msg)?;
                let time = if time.trim().is_empty() {
                    None
                } else {
                    Some(parse_widget_time(time.as_str())?)
                };
                db::create_event(
                    &state.borrow().conn,
                    title.trim(),
                    date,
                    time.as_deref(),
                    "",
                    "none",
                    reminder.trim(),
                    "event",
                    1,
                )?;
                Ok(())
            })();
            {
                let mut s = state.borrow_mut();
                match result {
                    Ok(()) => {
                        s.ai_draft = "已创建日程，可在日历中继续编辑".to_string();
                        s.ai_draft_title.clear();
                        s.ai_draft_date.clear();
                        s.ai_draft_time.clear();
                        s.ai_draft_reminder.clear();
                    }
                    Err(error) => s.ai_draft = format!("创建失败：{error}"),
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_add_subscription(move |name, url| {
            let name = name.trim().to_string();
            let url = url.trim().to_string();
            let (already_exists, result) = {
                let s = state.borrow();
                let already_exists = db::get_subscription_by_url(&s.conn, &url)
                    .ok()
                    .flatten()
                    .is_some();
                (
                    already_exists,
                    db::create_subscription(&s.conn, &name, &url, 1),
                )
            };
            let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) else {
                return;
            };
            let subscription = match result {
                Ok(subscription) => subscription,
                Err(error) => {
                    let text = format!("添加失败：{error}");
                    ui.set_integration_status(text.clone().into());
                    ui.set_action_message(text.into());
                    return;
                }
            };

            {
                let mut s = state.borrow_mut();
                s.subscription_name.clear();
                s.subscription_url.clear();
            }
            refresh_all(&ui, &widget, &state);
            let pending = if already_exists {
                format!("“{}”已存在，正在重新同步…", subscription.name)
            } else {
                format!("已添加“{}”，正在首次同步…", subscription.name)
            };
            ui.set_integration_status(pending.clone().into());
            ui.set_action_message(pending.into());

            let ui_weak = ui.as_weak();
            std::thread::spawn(move || {
                let text = match db::open().and_then(|conn| {
                    integrations::sync_ics_subscription_with_result(&conn, &subscription)
                }) {
                    Ok(report) => format!(
                        "“{}”同步成功：导入 {} 条日程，清理 {} 条已取消日程",
                        subscription.name, report.imported_events, report.removed_cancelled_events
                    ),
                    Err(error) => format!("“{}”同步失败：{error}", subscription.name),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_integration_status(text.clone().into());
                        ui.set_action_message(text.into());
                        ui.invoke_refresh_external_data();
                    }
                });
            });
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_sync_subscriptions(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_integration_status("正在同步全部日历订阅…".into());
            }
            let ui_weak = ui_weak.clone();
            std::thread::spawn(move || {
                let text = match db::open().and_then(|conn| integrations::sync_all_ics(&conn)) {
                    Ok(report) if report.errors.is_empty() => format!(
                        "同步成功：{} 个订阅，导入 {} 条日程，清理 {} 条已取消日程",
                        report.subscriptions,
                        report.imported_events,
                        report.removed_cancelled_events
                    ),
                    Ok(report) => format!(
                        "同步完成：{} 个订阅，{} 条日程，清理 {} 条已取消日程；失败：{}",
                        report.subscriptions,
                        report.imported_events,
                        report.removed_cancelled_events,
                        report.errors.join("；")
                    ),
                    Err(error) => format!("同步失败：{error}"),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_integration_status(text.clone().into());
                        ui.set_action_message(text.into());
                        ui.invoke_refresh_external_data();
                    }
                });
            });
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_refresh_external_data(move || {
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }

    // -------- 独立桌面挂件：系统拖动、独立置顶、移除与位置持久化 --------
    macro_rules! wire_widget_chrome {
        ($window:expr, $kind:literal) => {{
            let window_weak = $window.as_weak();
            let state_for_drag = state.clone();
            $window.on_begin_window_drag(move || {
                if let Some(window) = window_weak.upgrade() {
                    let _ = window
                        .window()
                        .with_winit_window(|native| native.drag_window());
                }
            });

            let window_weak = $window.as_weak();
            $window.on_end_window_drag(move || {
                if let Some(window) = window_weak.upgrade() {
                    save_widget_window_position(&window, &state_for_drag, $kind);
                }
            });

            let state_for_pin = state.clone();
            $window.on_pin_changed(move |pinned| {
                let state = state_for_pin.borrow();
                let key = format!("widget_{}_pinned", $kind);
                if let Err(error) =
                    db::set_setting(&state.conn, &key, if pinned { "1" } else { "0" })
                {
                    eprintln!("保存 {} 挂件置顶状态失败: {error}", $kind);
                }
            });

            let window_weak = $window.as_weak();
            let ui_weak = ui.as_weak();
            let visibility = desktop_widget_visibility.clone();
            let shown = widget_shown.clone();
            let state_for_remove = state.clone();
            $window.on_remove_widget(move || {
                if let Some(window) = window_weak.upgrade() {
                    let _ = window.hide();
                }
                visibility.borrow_mut().set($kind, false);
                let configuration = *visibility.borrow();
                let any_visible = configuration.any();
                shown.set(any_visible);
                if let Some(ui) = ui_weak.upgrade() {
                    sync_desktop_visibility_to_ui(&ui, configuration);
                    ui.set_widget_visible(any_visible);
                }
                let state = state_for_remove.borrow();
                let key = format!("widget_{}_visible", $kind);
                if let Err(error) = db::set_setting(&state.conn, &key, "0") {
                    eprintln!("移除 {} 桌面卡片失败: {error}", $kind);
                }
                let _ = db::set_setting(
                    &state.conn,
                    "desktop_widgets_visible",
                    if any_visible { "1" } else { "0" },
                );
            });
        }};
    }

    wire_widget_chrome!(desktop_widgets.calendar, "calendar");
    wire_widget_chrome!(desktop_widgets.events, "events");
    wire_widget_chrome!(desktop_widgets.countdown, "countdown");
    wire_widget_chrome!(desktop_widgets.clock, "clock");
    wire_widget_chrome!(desktop_widgets.focus, "focus");
    wire_widget_chrome!(desktop_widgets.todo, "todo");

    // The calendar card is one responsive widget: compact at its default
    // size and a full month view when enlarged.  The title-bar button toggles
    // between the two useful sizes, while the corner grip supports free
    // southeast resizing.  Persist the physical size alongside its position.
    {
        use slint::winit_030::winit::window::ResizeDirection;

        let calendar_weak = desktop_widgets.calendar.as_weak();
        desktop_widgets.calendar.on_begin_window_resize(move || {
            if let Some(calendar) = calendar_weak.upgrade() {
                let _ = calendar.window().with_winit_window(|native| {
                    let _ = native.drag_resize_window(ResizeDirection::SouthEast);
                });
            }
        });

        let calendar_weak = desktop_widgets.calendar.as_weak();
        let state_for_size = state.clone();
        desktop_widgets.calendar.on_end_window_resize(move || {
            if let Some(calendar) = calendar_weak.upgrade() {
                save_widget_window_size(&calendar, &state_for_size, "calendar");
            }
        });

        let calendar_weak = desktop_widgets.calendar.as_weak();
        let state_for_size = state.clone();
        desktop_widgets.calendar.on_toggle_window_size(move || {
            if let Some(calendar) = calendar_weak.upgrade() {
                let target = if calendar.get_expanded() {
                    slint::LogicalSize::new(304.0, 280.0)
                } else {
                    slint::LogicalSize::new(1100.0, 720.0)
                };
                calendar.window().set_size(target);
                let calendar_weak = calendar.as_weak();
                let state_for_size = state_for_size.clone();
                slint::Timer::single_shot(Duration::from_millis(120), move || {
                    if let Some(calendar) = calendar_weak.upgrade() {
                        save_widget_window_size(&calendar, &state_for_size, "calendar");
                    }
                });
            }
        });
    }

    // The events card has a visible bottom grip.  Native resize keeps the
    // frameless window responsive while limiting the gesture to vertical size.
    {
        use slint::winit_030::winit::window::ResizeDirection;
        let events_weak = desktop_widgets.events.as_weak();
        desktop_widgets.events.on_begin_window_resize(move || {
            if let Some(events) = events_weak.upgrade() {
                let _ = events.window().with_winit_window(|native| {
                    let _ = native.drag_resize_window(ResizeDirection::South);
                });
            }
        });
    }

    {
        let quick_weak = quick_panel.as_weak();
        quick_panel.on_close_requested(move || {
            if let Some(quick) = quick_weak.upgrade() {
                let _ = quick.hide();
            }
        });
    }
    {
        let state_for_pin = state.clone();
        quick_panel.on_pin_changed(move |pinned| {
            let state = state_for_pin.borrow();
            if let Err(error) = db::set_setting(
                &state.conn,
                "quick_panel_pinned",
                if pinned { "1" } else { "0" },
            ) {
                eprintln!("保存快速面板置顶状态失败: {error}");
            }
        });
    }
    macro_rules! forward_widget_settings {
        ($window:expr) => {{
            let widget_weak = widget.as_weak();
            $window.on_open_widget_settings(move |kind| {
                if let Some(widget) = widget_weak.upgrade() {
                    widget.invoke_open_widget_settings(kind);
                }
            });
        }};
    }

    forward_widget_settings!(desktop_widgets.calendar);
    forward_widget_settings!(desktop_widgets.events);
    forward_widget_settings!(desktop_widgets.countdown);
    forward_widget_settings!(desktop_widgets.clock);
    forward_widget_settings!(desktop_widgets.focus);
    forward_widget_settings!(desktop_widgets.todo);

    macro_rules! forward_widget_main_view {
        ($window:expr) => {{
            let widget_weak = widget.as_weak();
            $window.on_open_main_view(move |kind, id| {
                if let Some(widget) = widget_weak.upgrade() {
                    widget.invoke_open_main_view(kind, id);
                }
            });
        }};
    }

    forward_widget_main_view!(desktop_widgets.calendar);
    forward_widget_main_view!(desktop_widgets.events);
    forward_widget_main_view!(desktop_widgets.countdown);
    forward_widget_main_view!(desktop_widgets.clock);

    {
        let widget_weak = widget.as_weak();
        desktop_widgets.calendar.on_select_day(move |day| {
            if let Some(widget) = widget_weak.upgrade() {
                widget.invoke_select_day(day);
            }
        });
    }
    {
        let widget_weak = widget.as_weak();
        desktop_widgets.calendar.on_prev_month(move || {
            if let Some(widget) = widget_weak.upgrade() {
                widget.invoke_prev_month();
            }
        });
    }
    {
        let widget_weak = widget.as_weak();
        desktop_widgets.calendar.on_next_month(move || {
            if let Some(widget) = widget_weak.upgrade() {
                widget.invoke_next_month();
            }
        });
    }
    {
        let widget_weak = widget.as_weak();
        desktop_widgets.calendar.on_goto_today(move || {
            if let Some(widget) = widget_weak.upgrade() {
                widget.invoke_goto_today();
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        let windows_weak = Rc::downgrade(&desktop_widgets);
        let calendar_weak = desktop_widgets.calendar.as_weak();
        desktop_widgets.calendar.on_request_new_event(move || {
            if let (Some(windows), Some(ui), Some(widget), Some(calendar)) = (
                windows_weak.upgrade(),
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                calendar_weak.upgrade(),
            ) {
                let date = {
                    let s = state.borrow();
                    NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day)
                        .unwrap_or_else(|| Local::now().date_naive())
                };
                let position = calendar.window().position();
                show_desktop_event_editor(
                    &windows, &ui, &widget, &state, date, position.x, position.y,
                );
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        let windows_weak = Rc::downgrade(&desktop_widgets);
        let events_weak = desktop_widgets.events.as_weak();
        desktop_widgets.events.on_request_new_event(move || {
            if let (Some(windows), Some(ui), Some(widget), Some(events)) = (
                windows_weak.upgrade(),
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                events_weak.upgrade(),
            ) {
                let position = events.window().position();
                show_desktop_event_editor(
                    &windows,
                    &ui,
                    &widget,
                    &state,
                    Local::now().date_naive(),
                    position.x,
                    position.y,
                );
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let window_weak = desktop_widgets.countdown.as_weak();
        let state = state.clone();
        desktop_widgets
            .countdown
            .on_create_countdown(move |title, date| {
                let result = (|| -> Result<()> {
                    let title = title.trim();
                    if title.is_empty() {
                        anyhow::bail!("请输入倒数日名称");
                    }
                    let date = parse_ui_date(date.as_str()).map_err(anyhow::Error::msg)?;
                    let s = state.borrow();
                    db::create_event(&s.conn, title, date, None, "", "none", "", "countdown", 1)?;
                    Ok(())
                })();
                if let Some(window) = window_weak.upgrade() {
                    match &result {
                        Ok(()) => {
                            window.set_create_status("".into());
                            window.set_create_open(false);
                            window.set_create_title("".into());
                        }
                        Err(error) => window.set_create_status(error.to_string().into()),
                    }
                }
                if result.is_ok() {
                    if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                        refresh_all(&ui, &widget, &state);
                    }
                }
            });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let window_weak = desktop_widgets.clock.as_weak();
        let state = state.clone();
        desktop_widgets
            .clock
            .on_create_reminder(move |title, time| {
                let result = (|| -> Result<()> {
                    let title = title.trim();
                    if title.is_empty() {
                        anyhow::bail!("请输入提醒内容");
                    }
                    let normalized = parse_widget_time(time.as_str())?;
                    let parsed = NaiveTime::parse_from_str(&normalized, "%H:%M")?;
                    let now = Local::now();
                    let mut date = now.date_naive();
                    if parsed <= now.time() {
                        date += chrono::Duration::days(1);
                    }
                    let s = state.borrow();
                    db::create_event(
                        &s.conn,
                        title,
                        date,
                        Some(&normalized),
                        "",
                        "none",
                        "0",
                        "event",
                        1,
                    )?;
                    Ok(())
                })();
                if let Some(window) = window_weak.upgrade() {
                    match &result {
                        Ok(()) => {
                            window.set_create_status("".into());
                            window.set_create_open(false);
                            window.set_create_title("".into());
                        }
                        Err(error) => window.set_create_status(error.to_string().into()),
                    }
                }
                if result.is_ok() {
                    if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                        refresh_all(&ui, &widget, &state);
                    }
                }
            });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let window_weak = desktop_widgets.focus.as_weak();
        let state = state.clone();
        desktop_widgets
            .focus
            .on_create_focus(move |title, minutes| {
                let result = (|| -> Result<()> {
                    let title = title.trim();
                    if title.is_empty() {
                        anyhow::bail!("请输入专注任务名称");
                    }
                    let minutes = minutes.trim().parse::<u64>().context("时长应为分钟数")?;
                    if !(1..=240).contains(&minutes) {
                        anyhow::bail!("时长范围为 1–240 分钟");
                    }
                    let mut s = state.borrow_mut();
                    s.focus_task = title.to_string();
                    s.pomodoro_total_secs = minutes * 60;
                    s.pomodoro_remaining_secs = s.pomodoro_total_secs;
                    s.pomodoro_end_at = None;
                    s.focus_round += 1;
                    db::set_setting(&s.conn, "focus_task", &s.focus_task)?;
                    db::set_setting(&s.conn, "focus_minutes", &minutes.to_string())?;
                    Ok(())
                })();
                if let Some(window) = window_weak.upgrade() {
                    match &result {
                        Ok(()) => {
                            window.set_create_status("".into());
                            window.set_create_open(false);
                            window.set_create_title("".into());
                        }
                        Err(error) => window.set_create_status(error.to_string().into()),
                    }
                }
                if result.is_ok() {
                    if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                        update_tool_status(&ui, &widget, &state);
                    }
                }
            });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let window_weak = desktop_widgets.todo.as_weak();
        let state = state.clone();
        desktop_widgets.todo.on_create_todo(move |title| {
            let result = (|| -> Result<()> {
                let title = title.trim();
                if title.is_empty() {
                    anyhow::bail!("请输入待办标题");
                }
                let s = state.borrow();
                db::create_todo(&s.conn, title, Some(Local::now().date_naive()), 0)?;
                Ok(())
            })();
            if let Some(window) = window_weak.upgrade() {
                match &result {
                    Ok(()) => {
                        window.set_create_status("".into());
                        window.set_create_open(false);
                        window.set_create_title("".into());
                    }
                    Err(error) => window.set_create_status(error.to_string().into()),
                }
            }
            if result.is_ok() {
                if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                    refresh_all(&ui, &widget, &state);
                }
            }
        });
    }
    {
        let widget_weak = widget.as_weak();
        desktop_widgets.todo.on_toggle_todo(move |id| {
            if let Some(widget) = widget_weak.upgrade() {
                widget.invoke_toggle_todo(id);
            }
        });
    }
    {
        let widget_weak = widget.as_weak();
        desktop_widgets.todo.on_add_widget_todo(move || {
            if let Some(widget) = widget_weak.upgrade() {
                widget.invoke_add_widget_todo();
            }
        });
    }
    {
        let widget_weak = widget.as_weak();
        desktop_widgets.focus.on_focus_toggle(move || {
            if let Some(widget) = widget_weak.upgrade() {
                widget.invoke_focus_toggle();
            }
        });
    }
    {
        let widget_weak = widget.as_weak();
        desktop_widgets.focus.on_focus_start(move || {
            if let Some(widget) = widget_weak.upgrade() {
                widget.invoke_focus_start();
            }
        });
    }
    {
        let widget_weak = widget.as_weak();
        desktop_widgets.focus.on_focus_complete(move || {
            if let Some(widget) = widget_weak.upgrade() {
                widget.invoke_focus_complete();
            }
        });
    }

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
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_open_widget_settings(move |kind| {
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                let section = match kind.as_str() {
                    "calendar" | "events" => 1,
                    "countdown" | "focus" => 2,
                    _ => 0,
                };
                ui.set_settings_section(section);
                ui.set_settings_open(true);
                ui.set_action_message(format!("已打开{}挂件设置", kind).into());
                refresh_all(&ui, &widget, &state);
                show_and_focus_main_window(&ui);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_open_main_view(move |kind, id| {
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
        let widget_weak = widget.as_weak();
        let state = state.clone();
        widget.on_add_widget_todo(move || {
            state.borrow_mut().view_mode = 4;
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                set_main_view_mode(&ui, 4);
                ui.set_todo_add_target("normal".into());
                ui.set_todo_draft_title("".into());
                ui.set_todo_add_open(true);
                refresh_all(&ui, &widget, &state);
                show_and_focus_main_window(&ui);
            }
        });
    }

    // -------- 打开/关闭桌面挂件 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let desktop_widgets = desktop_widgets.clone();
        let widget_shown = widget_shown.clone();
        let desktop_widget_visibility = desktop_widget_visibility.clone();
        let desktop_click_through = desktop_click_through.clone();
        let state = state.clone();
        ui.on_toggle_widget(move || {
            let show = !widget_shown.get();
            widget_shown.set(show);
            if let Some(widget) = widget_weak.upgrade() {
                if show {
                    desktop_widgets.sync_from(&widget);
                    desktop_widgets.show_configured(
                        *desktop_widget_visibility.borrow(),
                        desktop_click_through.get(),
                    );
                } else {
                    desktop_widgets.hide_all();
                }
            }
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_widget_visible(show);
            }
            let state = state.borrow();
            if let Err(error) = db::set_setting(
                &state.conn,
                "desktop_widgets_visible",
                if show { "1" } else { "0" },
            ) {
                eprintln!("保存桌面挂件显示状态失败: {error}");
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let desktop_widgets = desktop_widgets.clone();
        let widget_shown = widget_shown.clone();
        let desktop_widget_visibility = desktop_widget_visibility.clone();
        let desktop_click_through = desktop_click_through.clone();
        let state = state.clone();
        ui.on_set_desktop_widget_visible(move |kind, visible| {
            let kind = kind.to_string();
            let refresh_notes = kind == "notes" && visible;
            if refresh_notes {
                let s = state.borrow();
                if db::list_notes(&s.conn).unwrap_or_default().is_empty() {
                    if let Err(error) = db::create_note(&s.conn, "新便签", "") {
                        eprintln!("创建首个便签卡片失败: {error}");
                    }
                }
            }
            {
                desktop_widget_visibility.borrow_mut().set(&kind, visible);
            }
            let configuration = *desktop_widget_visibility.borrow();
            let any_visible = configuration.any();
            widget_shown.set(any_visible);
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                if refresh_notes {
                    refresh_all(&ui, &widget, &state);
                } else {
                    desktop_widgets.sync_from(&widget);
                }
                desktop_widgets.show_configured(configuration, desktop_click_through.get());
            }
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_widget_visible(any_visible);
            }
            let state = state.borrow();
            let key = format!("widget_{kind}_visible");
            if let Err(error) = db::set_setting(&state.conn, &key, if visible { "1" } else { "0" })
            {
                eprintln!("保存桌面卡片状态失败: {error}");
            }
            let _ = db::set_setting(
                &state.conn,
                "desktop_widgets_visible",
                if any_visible { "1" } else { "0" },
            );
        });
    }
    {
        let desktop_widgets = desktop_widgets.clone();
        let desktop_widget_visibility = desktop_widget_visibility.clone();
        let desktop_click_through = desktop_click_through.clone();
        let state = state.clone();
        ui.on_set_desktop_click_through(move |enabled| {
            desktop_click_through.set(enabled);
            desktop_widgets.apply_click_through(*desktop_widget_visibility.borrow(), enabled);
            let state = state.borrow();
            if let Err(error) = db::set_setting(
                &state.conn,
                "desktop_widgets_click_through",
                if enabled { "1" } else { "0" },
            ) {
                eprintln!("保存桌面挂件鼠标穿透状态失败: {error}");
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        ui.on_open_quick_panel(move || {
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                sync_quick_panel(&quick, &ui, &widget);
                show_and_focus_quick_panel(&quick);
            }
        });
    }

    // -------- 托盘快速面板：待办可直接勾选，打开时从主窗口复制最新模型 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        quick_panel.on_toggle_todo(move |id| {
            {
                let s = state.borrow();
                if let Err(e) = db::toggle_todo(&s.conn, id as i64) {
                    eprintln!("快速面板更新待办失败: {e}");
                }
            }
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                refresh_all(&ui, &widget, &state);
                sync_quick_panel(&quick, &ui, &widget);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        quick_panel.on_open_view(move |mode| {
            {
                let mut s = state.borrow_mut();
                s.view_mode = mode;
                if mode == 2 || mode == 3 {
                    if let Some(date) = NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day) {
                        s.timeline_anchor = date;
                    }
                }
            }
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                set_main_view_mode(&ui, mode);
                refresh_all(&ui, &widget, &state);
                show_and_focus_main_window(&ui);
                let _ = quick.hide();
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let quick_weak = quick_panel.as_weak();
        quick_panel.on_open_event(move |id| {
            if let (Some(ui), Some(quick)) = (ui_weak.upgrade(), quick_weak.upgrade()) {
                ui.invoke_open_event(id);
                show_and_focus_main_window(&ui);
                let _ = quick.hide();
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        quick_panel.on_select_day(move |day| {
            {
                let mut s = state.borrow_mut();
                if NaiveDate::from_ymd_opt(s.year, s.month, day as u32).is_some() {
                    s.selected_day = day as u32;
                    s.timeline_anchor =
                        NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day).unwrap();
                    s.view_mode = 2;
                }
            }
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                set_main_view_mode(&ui, 2);
                refresh_all(&ui, &widget, &state);
                show_and_focus_main_window(&ui);
                let _ = quick.hide();
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let quick_weak = quick_panel.as_weak();
        quick_panel.on_new_event(move || {
            if let (Some(ui), Some(quick)) = (ui_weak.upgrade(), quick_weak.upgrade()) {
                open_full_event_editor(&ui, Local::now().date_naive());
                let _ = quick.hide();
            }
        });
    }

    // -------- 个性化设置：一周起始日 / 周数显示 / 主题 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_set_week_starts_sunday(move |sunday| {
            {
                let mut s = state.borrow_mut();
                s.week_starts_sunday = sunday;
                let _ = db::set_setting(&s.conn, "week_start", if sunday { "sun" } else { "mon" });
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                ui.set_week_starts_sunday(sunday);
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_set_show_week_numbers(move |show| {
            {
                let mut s = state.borrow_mut();
                s.show_week_numbers = show;
                let _ = db::set_setting(&s.conn, "show_week_number", if show { "1" } else { "0" });
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                ui.set_show_week_numbers(show);
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_set_theme(move |index| {
            {
                let s = state.borrow();
                let _ = db::set_setting(&s.conn, "theme", &index.to_string());
            }
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                ui.set_theme_index(index);
                apply_theme(&ui, &widget, &quick, index);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_calculate_tool(move |operation, start, end, offset| {
            let result = match operation.as_str() {
                "diff" => match (parse_ui_date(start.as_str()), parse_ui_date(end.as_str())) {
                    (Ok(start), Ok(end)) => {
                        let diff = date_calc::diff(start, end);
                        format!(
                            "{} 到 {}：自然日 {} 天，工作日 {} 天",
                            diff.start, diff.end, diff.calendar_days, diff.workdays
                        )
                    }
                    (Err(e), _) | (_, Err(e)) => e,
                },
                "calendar" | "workday" => {
                    match (parse_ui_date(start.as_str()), offset.trim().parse::<i64>()) {
                        (Ok(start), Ok(n)) => {
                            let date = if operation == "calendar" {
                                date_calc::add_calendar_days(start, n)
                            } else {
                                date_calc::add_workdays(start, n)
                            };
                            format!(
                                "从 {} {} {} 天：{}",
                                start,
                                if operation == "calendar" {
                                    "起算自然日"
                                } else {
                                    "起算工作日"
                                },
                                n,
                                date
                            )
                        }
                        (Err(e), _) => e,
                        (_, Err(_)) => "推算天数必须是整数，例如 10 或 -3".to_string(),
                    }
                }
                _ => "未知的日期计算操作".to_string(),
            };
            {
                let mut s = state.borrow_mut();
                s.calculator_start = start.to_string();
                s.calculator_end = end.to_string();
                s.calculator_offset = offset.to_string();
                s.calculator_result = result;
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_set_weather_city(move |city| {
            {
                let s = state.borrow();
                if let Err(e) = db::set_setting(&s.conn, "weather_city", city.trim()) {
                    eprintln!("保存天气城市失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                ui.invoke_refresh_weather();
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        ui.on_refresh_weather(move || {
            let ui_weak = ui_weak.clone();
            let widget_weak = widget_weak.clone();
            std::thread::spawn(move || {
                let result = db::open().and_then(|conn| weather::refresh_once(&conn));
                let (weather_text, status_text) = match result {
                    Ok(w) => {
                        let (desc, _) = weather::describe_code(w.code);
                        let text = format!("{} {:.0}°C {desc}", w.city, w.temp_c);
                        let source = if w.provider.is_empty() {
                            "天气服务"
                        } else {
                            w.provider.as_str()
                        };
                        (
                            Some(text.clone()),
                            format!("{text} · 已更新 {} · {source}", w.updated_at),
                        )
                    }
                    Err(e) => {
                        eprintln!("天气刷新失败（继续使用缓存）: {e:#}");
                        let cached = db::open().ok().and_then(|conn| weather::cached(&conn));
                        match cached {
                            Some(w) => {
                                let (desc, _) = weather::describe_code(w.code);
                                let text = format!("{} {:.0}°C {desc}", w.city, w.temp_c);
                                (
                                    Some(text.clone()),
                                    format!("{text} · 使用 {} 缓存；刷新失败: {e:#}", w.updated_at),
                                )
                            }
                            None => (None, format!("天气刷新失败: {e:#}")),
                        }
                    }
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_weather_status(status_text.into());
                    }
                    if let Some(weather_text) = weather_text {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.set_weather_summary(weather_text.clone().into());
                        }
                        if let Some(widget) = widget_weak.upgrade() {
                            widget.set_weather_text(weather_text.into());
                        }
                    }
                });
            });
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_toggle_stopwatch(move || {
            {
                let mut s = state.borrow_mut();
                let now = Instant::now();
                if s.stopwatch_started_at.is_some() {
                    s.stopwatch_elapsed_secs = stopwatch_seconds(&s, now);
                    s.stopwatch_started_at = None;
                } else {
                    s.stopwatch_started_at = Some(now);
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_reset_stopwatch(move || {
            {
                let mut s = state.borrow_mut();
                s.stopwatch_elapsed_secs = 0;
                s.stopwatch_started_at = None;
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_toggle_pomodoro(move || {
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
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_reset_pomodoro(move || {
            {
                let mut s = state.borrow_mut();
                // “重置25分”是固定动作，不能沿用桌面专注控件上一次的自定义时长。
                s.pomodoro_total_secs = 25 * 60;
                s.pomodoro_remaining_secs = 25 * 60;
                s.pomodoro_end_at = None;
                if let Err(error) = db::set_setting(&s.conn, "focus_minutes", "25") {
                    eprintln!("保存番茄钟默认时长失败: {error}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }

    // -------- 分类日历（工作/个人/家庭…，显示/隐藏筛选）--------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_toggle_calendar_visible(move |id, visible| {
            {
                let s = state.borrow();
                if let Err(e) = db::set_calendar_visible(&s.conn, id as i64, visible) {
                    eprintln!("切换日历可见性失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_add_calendar(move |name: SharedString| {
            let name = name.trim().to_string();
            if !name.is_empty() {
                let s = state.borrow();
                let existing_count = db::list_calendars(&s.conn).map(|v| v.len()).unwrap_or(0);
                let color = CALENDAR_COLOR_CYCLE[existing_count % CALENDAR_COLOR_CYCLE.len()];
                if let Err(e) = db::create_calendar(&s.conn, &name, color) {
                    eprintln!("新建分类日历失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }

    // -------- 日程 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_add_event(
            move |title: SharedString,
                  repeat_rule: SharedString,
                  reminder_offsets: SharedString,
                  category: SharedString,
                  calendar_id: i32| {
                let title = title.trim().to_string();
                if !title.is_empty() {
                    let s = state.borrow();
                    if let Some(date) = NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day) {
                        // 生日/纪念日语义上是"每年重复"；如果用户没有手动选择重复规则，这里补一个合理默认值。
                        let repeat_rule = if matches!(category.as_str(), "birthday" | "anniversary")
                            && repeat_rule.as_str() == "none"
                        {
                            "yearly"
                        } else {
                            repeat_rule.as_str()
                        };
                        if let Err(e) = db::create_event(
                            &s.conn,
                            &title,
                            date,
                            None,
                            "",
                            repeat_rule,
                            reminder_offsets.as_str(),
                            category.as_str(),
                            calendar_id as i64,
                        ) {
                            eprintln!("新建日程失败: {e}");
                        }
                    }
                }
                if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                    refresh_all(&ui, &widget, &state);
                }
            },
        );
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_delete_event(move |id| {
            {
                let s = state.borrow();
                if let Err(e) = db::delete_event(&s.conn, id as i64) {
                    eprintln!("删除日程失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }

    // -------- 待办 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_add_todo_target(move |title: SharedString, target: SharedString| {
            let title = title.trim().to_string();
            let target = target.as_str();
            if !title.is_empty() {
                let s = state.borrow();
                let today = Local::now().date_naive();
                let (due, priority, important, status) = match target {
                    "urgent-important" => (Some(today), 2, true, "todo"),
                    "important" => (None, 2, true, "todo"),
                    "urgent" => (Some(today), 1, false, "todo"),
                    "doing" => (None, 1, false, "doing"),
                    "done" => (None, 0, false, "done"),
                    _ => (None, 0, false, "todo"),
                };
                match db::create_todo(&s.conn, &title, due, priority) {
                    Ok(todo) => {
                        if important {
                            if let Err(e) = db::set_todo_important(&s.conn, todo.id, true) {
                                eprintln!("设置待办重要状态失败: {e}");
                            }
                        }
                        if status != "todo" {
                            if let Err(e) = db::set_todo_status(&s.conn, todo.id, status) {
                                eprintln!("设置待办看板状态失败: {e}");
                            }
                        }
                    }
                    Err(e) => eprintln!("新建待办失败: {e}"),
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
                ui.set_action_message("待办已添加".into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_add_todo(move |title: SharedString| {
            let title = title.trim().to_string();
            if !title.is_empty() {
                let s = state.borrow();
                let due = NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day);
                if let Err(e) = db::create_todo(&s.conn, &title, due, 0) {
                    eprintln!("新建待办失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_toggle_todo(move |id| {
            {
                let s = state.borrow();
                if let Err(e) = db::toggle_todo(&s.conn, id as i64) {
                    eprintln!("更新待办失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_delete_todo(move |id| {
            {
                let s = state.borrow();
                if let Err(e) = db::delete_todo(&s.conn, id as i64) {
                    eprintln!("删除待办失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }

    // -------- 便签 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_add_note(move |title: SharedString, content: SharedString| {
            let title = title.trim().to_string();
            if !title.is_empty() {
                let s = state.borrow();
                if let Err(e) = db::create_note(&s.conn, &title, content.as_str()) {
                    eprintln!("新建便签失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_update_note(move |id, title, content| {
            let s = state.borrow();
            if let Err(e) = db::update_note(&s.conn, id as i64, title.trim(), content.as_str()) {
                eprintln!("更新便签失败: {e}");
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_delete_note(move |id| {
            {
                let s = state.borrow();
                if let Err(e) = db::delete_note(&s.conn, id as i64) {
                    eprintln!("删除便签失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }

    // -------- 习惯打卡（始终针对"今天"）--------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_add_habit(move |title: SharedString| {
            let title = title.trim().to_string();
            if !title.is_empty() {
                let s = state.borrow();
                if let Err(e) = db::create_habit(&s.conn, &title) {
                    eprintln!("新建习惯失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_toggle_habit(move |id| {
            {
                let s = state.borrow();
                let today = Local::now().date_naive();
                if let Err(e) = db::toggle_habit_log(&s.conn, id as i64, today) {
                    eprintln!("打卡失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_delete_habit(move |id| {
            {
                let s = state.borrow();
                if let Err(e) = db::delete_habit(&s.conn, id as i64) {
                    eprintln!("删除习惯失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }

    // -------- 系统托盘图标（显示主窗口 / 开关桌面挂件 / 退出）--------
    let tray = build_tray_icon()?;
    let timer = slint::Timer::default();
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(250),
            move || {
                if TrayIconEvent::receiver().try_recv().is_ok() {
                    if let (Some(ui), Some(widget), Some(quick)) = (
                        ui_weak.upgrade(),
                        widget_weak.upgrade(),
                        quick_weak.upgrade(),
                    ) {
                        sync_quick_panel(&quick, &ui, &widget);
                        show_and_focus_quick_panel(&quick);
                    }
                }
                if TASKBAR_CLOCK_CLICKED.swap(false, Ordering::AcqRel) {
                    if let (Some(ui), Some(widget), Some(quick)) = (
                        ui_weak.upgrade(),
                        widget_weak.upgrade(),
                        quick_weak.upgrade(),
                    ) {
                        sync_quick_panel(&quick, &ui, &widget);
                        show_and_focus_quick_panel_at(&quick, last_clicked_taskbar_anchor());
                    }
                }
                if let Ok(event) = MenuEvent::receiver().try_recv() {
                    if event.id == tray.show_id {
                        if let Some(ui) = ui_weak.upgrade() {
                            show_and_focus_main_window(&ui);
                        }
                    } else if event.id == tray.widget_id {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.invoke_toggle_widget();
                        }
                    } else if event.id == tray.quit_id {
                        slint::quit_event_loop().ok();
                    }
                }
            },
        );
    }

    // -------- 桌面挂件实时时钟：每秒刷新一次 HH:MM:SS --------
    let clock_timer = slint::Timer::default();
    {
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let ui_weak = ui.as_weak();
        let now = Local::now();
        let now_text: SharedString = now.format("%H:%M:%S").to_string().into();
        widget.set_current_time_text(now_text.clone());
        widget.set_current_time_main(now.format("%H:%M").to_string().into());
        widget.set_current_seconds(now.format("%S").to_string().into());
        widget.set_current_minutes((now.hour() * 60 + now.minute()) as i32);
        update_widget_day_progress(&widget, now.time().num_seconds_from_midnight());
        quick_panel.set_time_text(now_text);
        ui.set_current_minutes((now.hour() * 60 + now.minute()) as i32);
        ui.set_current_time_text(now.format("%H:%M").to_string().into());
        clock_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(1000),
            move || {
                let now = Local::now();
                let now_text: SharedString = now.format("%H:%M:%S").to_string().into();
                if let Some(widget) = widget_weak.upgrade() {
                    widget.set_current_time_text(now_text.clone());
                    widget.set_current_time_main(now.format("%H:%M").to_string().into());
                    widget.set_current_seconds(now.format("%S").to_string().into());
                    widget.set_current_minutes((now.hour() * 60 + now.minute()) as i32);
                    update_widget_day_progress(&widget, now.time().num_seconds_from_midnight());
                }
                if let Some(quick) = quick_weak.upgrade() {
                    quick.set_time_text(now_text);
                }
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_current_minutes((now.hour() * 60 + now.minute()) as i32);
                    ui.set_current_time_text(now.format("%H:%M").to_string().into());
                    if now.second() % 10 == 0 && ui.get_taskbar_clock_enabled() {
                        update_taskbar_clock_hit_rect();
                    }
                }
            },
        );
    }
    let tools_timer = slint::Timer::default();
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        update_tool_status(&ui, &widget, &state);
        tools_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(1000),
            move || {
                if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                    update_tool_status(&ui, &widget, &state);
                }
            },
        );
    }

    ui.run()?;
    Ok(())
}

fn shift_month(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    if delta < 0 {
        if s.month == 1 {
            s.month = 12;
            s.year -= 1;
        } else {
            s.month -= 1;
        }
    } else if s.month == 12 {
        s.month = 1;
        s.year += 1;
    } else {
        s.month += 1;
    }
    s.selected_day = 1;
}

fn shift_year(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    s.year += delta;
    while NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day).is_none() {
        s.selected_day = s.selected_day.saturating_sub(1);
    }
    s.week_anchor =
        NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day).unwrap_or(s.week_anchor);
    s.timeline_anchor =
        NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day).unwrap_or(s.timeline_anchor);
}

fn goto_today(state: &Rc<RefCell<AppState>>) {
    let today = Local::now().date_naive();
    let mut s = state.borrow_mut();
    s.year = today.year();
    s.month = today.month();
    s.selected_day = today.day();
    s.week_anchor = today;
    s.timeline_anchor = today;
}

fn shift_week(state: &Rc<RefCell<AppState>>, delta_weeks: i64) {
    let mut s = state.borrow_mut();
    let new_anchor = s.week_anchor + chrono::Duration::days(7 * delta_weeks);
    s.week_anchor = new_anchor;
    s.year = new_anchor.year();
    s.month = new_anchor.month();
    s.selected_day = new_anchor.day();
}

fn select_full_date(state: &Rc<RefCell<AppState>>, date: NaiveDate) {
    let mut s = state.borrow_mut();
    s.year = date.year();
    s.month = date.month();
    s.selected_day = date.day();
    s.week_anchor = date;
    s.timeline_anchor = date;
}

/// 日/三日视图的翻页：日视图每次移动 1 天，三日视图每次移动 3 天（跟"这组显示了哪几天"保持一致）。
fn shift_timeline(state: &Rc<RefCell<AppState>>, delta: i64) {
    let mut s = state.borrow_mut();
    let step = if s.view_mode == 3 { 3 } else { 1 };
    let new_anchor = s.timeline_anchor + chrono::Duration::days(step * delta);
    s.timeline_anchor = new_anchor;
    s.year = new_anchor.year();
    s.month = new_anchor.month();
    s.selected_day = new_anchor.day();
}

/// 一周的起始日期（周一或周日，取决于设置），`anchor` 是这一周里任意一天。
fn week_start_of(anchor: NaiveDate, week_starts_sunday: bool) -> NaiveDate {
    let leading = if week_starts_sunday {
        anchor.weekday().num_days_from_sunday()
    } else {
        anchor.weekday().num_days_from_monday()
    };
    anchor - chrono::Duration::days(leading as i64)
}

fn course_week_for_date(term_start: NaiveDate, date: NaiveDate) -> i32 {
    if date < term_start {
        1
    } else {
        ((date - term_start).num_days() / 7 + 1).clamp(1, 30) as i32
    }
}

fn course_week_range(term_start: NaiveDate, week: i32) -> (NaiveDate, NaiveDate) {
    let start = term_start + chrono::Duration::days((week.clamp(1, 30) - 1) as i64 * 7);
    (start, start + chrono::Duration::days(6))
}

/// 设计规范中的 8 色强调色，同时应用到主窗口和桌面挂件的 `Theme` 全局。
fn apply_theme(ui: &AppWindow, widget: &WidgetWindow, quick_panel: &QuickPanelWindow, index: i32) {
    struct Palette {
        accent: (u8, u8, u8),
        today_bg: (u8, u8, u8),
    }
    const PALETTES: [Palette; 8] = [
        Palette {
            accent: (0x2e, 0x6b, 0xe6),
            today_bg: (0x2e, 0x6b, 0xe6),
        },
        Palette {
            accent: (0x0e, 0x9f, 0x6e),
            today_bg: (0x0e, 0x9f, 0x6e),
        },
        Palette {
            accent: (0xc7, 0x77, 0x00),
            today_bg: (0xc7, 0x77, 0x00),
        },
        Palette {
            accent: (0xd9, 0x3a, 0x49),
            today_bg: (0xd9, 0x3a, 0x49),
        },
        Palette {
            accent: (0x7c, 0x4d, 0xff),
            today_bg: (0x7c, 0x4d, 0xff),
        },
        Palette {
            accent: (0x08, 0x91, 0xb2),
            today_bg: (0x08, 0x91, 0xb2),
        },
        Palette {
            accent: (0xdb, 0x27, 0x77),
            today_bg: (0xdb, 0x27, 0x77),
        },
        Palette {
            accent: (0x65, 0xa3, 0x0d),
            today_bg: (0x65, 0xa3, 0x0d),
        },
    ];
    let palette = &PALETTES[(index.max(0) as usize) % PALETTES.len()];
    let accent = slint::Color::from_rgb_u8(palette.accent.0, palette.accent.1, palette.accent.2);
    let today_bg =
        slint::Color::from_rgb_u8(palette.today_bg.0, palette.today_bg.1, palette.today_bg.2);

    let apply = |theme: Theme<'_>| {
        theme.set_accent(accent);
        theme.set_today_bg(today_bg);
    };
    apply(ui.global::<Theme>());
    apply(widget.global::<Theme>());
    apply(quick_panel.global::<Theme>());
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        if let Some(windows) = slot.borrow().as_ref() {
            apply(windows.calendar.global::<Theme>());
            apply(windows.events.global::<Theme>());
            if let Some(editor) = windows.event_editor.borrow().as_ref() {
                apply(editor.global::<Theme>());
            }
            apply(windows.countdown.global::<Theme>());
            apply(windows.clock.global::<Theme>());
            apply(windows.focus.global::<Theme>());
            apply(windows.todo.global::<Theme>());
            for window in windows.notes.borrow().iter() {
                apply(window.global::<Theme>());
            }
        }
    });
}

/// 同步亮色、暗色和跟随系统模式到所有独立窗口的主题全局。
fn apply_visual_theme(
    ui: &AppWindow,
    widget: &WidgetWindow,
    quick_panel: &QuickPanelWindow,
    mode: i32,
) {
    let mode = mode.clamp(0, 2);
    ui.global::<Theme>().set_theme_mode(mode);
    widget.global::<Theme>().set_theme_mode(mode);
    quick_panel.global::<Theme>().set_theme_mode(mode);
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        if let Some(windows) = slot.borrow().as_ref() {
            windows.calendar.global::<Theme>().set_theme_mode(mode);
            windows.events.global::<Theme>().set_theme_mode(mode);
            if let Some(editor) = windows.event_editor.borrow().as_ref() {
                editor.global::<Theme>().set_theme_mode(mode);
            }
            windows.countdown.global::<Theme>().set_theme_mode(mode);
            windows.clock.global::<Theme>().set_theme_mode(mode);
            windows.focus.global::<Theme>().set_theme_mode(mode);
            windows.todo.global::<Theme>().set_theme_mode(mode);
            for window in windows.notes.borrow().iter() {
                window.global::<Theme>().set_theme_mode(mode);
            }
        }
    });
}

fn apply_accessibility_preferences(
    ui: &AppWindow,
    widget: &WidgetWindow,
    quick_panel: &QuickPanelWindow,
    font_size: i32,
    density: i32,
    reduce_motion: bool,
) {
    let font_delta = font_size.clamp(12, 16) - 13;
    let density = density.clamp(0, 2);
    for theme in [
        ui.global::<Theme>(),
        widget.global::<Theme>(),
        quick_panel.global::<Theme>(),
    ] {
        theme.set_font_delta(font_delta);
        theme.set_density_mode(density);
        theme.set_reduce_motion(reduce_motion);
    }
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        if let Some(windows) = slot.borrow().as_ref() {
            for theme in [
                windows.calendar.global::<Theme>(),
                windows.events.global::<Theme>(),
                windows.countdown.global::<Theme>(),
                windows.clock.global::<Theme>(),
                windows.focus.global::<Theme>(),
                windows.todo.global::<Theme>(),
            ] {
                theme.set_font_delta(font_delta);
                theme.set_density_mode(density);
                theme.set_reduce_motion(reduce_motion);
            }
            if let Some(editor) = windows.event_editor.borrow().as_ref() {
                let theme = editor.global::<Theme>();
                theme.set_font_delta(font_delta);
                theme.set_density_mode(density);
                theme.set_reduce_motion(reduce_motion);
            }
            for window in windows.notes.borrow().iter() {
                let theme = window.global::<Theme>();
                theme.set_font_delta(font_delta);
                theme.set_density_mode(density);
                theme.set_reduce_motion(reduce_motion);
            }
        }
    });
}

fn sync_quick_panel(quick: &QuickPanelWindow, ui: &AppWindow, widget: &WidgetWindow) {
    let today = Local::now().date_naive();
    let weekday = match today.weekday() {
        chrono::Weekday::Mon => "星期一",
        chrono::Weekday::Tue => "星期二",
        chrono::Weekday::Wed => "星期三",
        chrono::Weekday::Thu => "星期四",
        chrono::Weekday::Fri => "星期五",
        chrono::Weekday::Sat => "星期六",
        chrono::Weekday::Sun => "星期日",
    };
    quick.set_days(ui.get_days());
    quick.set_events(ui.get_events_for_day());
    quick.set_todos(ui.get_todos_for_day());
    quick.set_weather_text(ui.get_weather_summary());
    quick.set_time_text(widget.get_current_time_text());
    quick.set_date_title(format!("{}月{}日", today.month(), today.day()).into());
    quick.set_weekday_text(weekday.into());
    quick.set_lunar_text(widget.get_today_lunar_text());
    quick.set_month_title(ui.get_month_title());
    quick.set_countdown_count(widget.get_countdowns().row_count() as i32);
    quick.set_active_view(ui.get_view_mode());
}

/// 把逗号分隔的“提前 N 分钟”提醒偏移量转成中文说明，例如 "0,30,1440" -> "提醒：准时/提前30分钟/提前1天"。
fn format_offset_label(offsets: &str) -> String {
    let labels: Vec<String> = offsets
        .split(',')
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .map(|m| match m {
            0 => "准时".to_string(),
            n if n % 1440 == 0 => format!("提前{}天", n / 1440),
            n if n % 60 == 0 => format!("提前{}小时", n / 60),
            n => format!("提前{n}分钟"),
        })
        .collect();
    if labels.is_empty() {
        String::new()
    } else {
        format!("提醒：{}", labels.join("/"))
    }
}

/// 分类徽标：普通日程返回空字符串；生日/纪念日/倒数日返回带有具体年数/剩余天数的说明，
/// 例如"第8个生日""5周年""还有12天"。`occurrence_date` 是这条日程这一次具体落在哪天，
/// 与基准日期（`event.date`，通常是第一次发生）对比得出经过的年数。
fn category_badge(event: &db::Event, occurrence_date: NaiveDate) -> String {
    let base = NaiveDate::parse_from_str(&event.date, "%Y-%m-%d").ok();
    match event.category.as_str() {
        "birthday" => match base {
            Some(base) if occurrence_date.year() > base.year() => {
                format!("第{}个生日", occurrence_date.year() - base.year() + 1)
            }
            _ => "生日".to_string(),
        },
        "anniversary" => match base {
            Some(base) if occurrence_date.year() > base.year() => {
                format!("{}周年", occurrence_date.year() - base.year())
            }
            _ => "纪念日".to_string(),
        },
        "countdown" => {
            let today = Local::now().date_naive();
            let days = (occurrence_date - today).num_days();
            if days > 0 {
                format!("还有{days}天")
            } else if days == 0 {
                "就是今天".to_string()
            } else {
                format!("已过{}天", -days)
            }
        }
        _ => String::new(),
    }
}

fn event_meta_text(occ: &db::EventOccurrence) -> String {
    let occurrence_date = NaiveDate::parse_from_str(&occ.occurrence_date, "%Y-%m-%d")
        .unwrap_or_else(|_| NaiveDate::from_ymd_opt(1970, 1, 1).expect("固定回退日期"));
    let rule = recurrence::RepeatRule::parse(&occ.event.repeat_rule);
    let parts: Vec<String> = [
        category_badge(&occ.event, occurrence_date),
        recurrence::describe(&rule),
        format_offset_label(&occ.event.reminder_offsets),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect();
    parts.join("  ")
}

fn to_ui_event_occ(occ: &db::EventOccurrence, colors: &HashMap<i64, slint::Color>) -> EventItem {
    let color = colors
        .get(&occ.event.calendar_id)
        .copied()
        .unwrap_or_else(|| parse_hex_color("#2e6be6"));
    let (start_minutes, all_day) = occ
        .event
        .time
        .as_deref()
        .and_then(|time| {
            let (hour, minute) = time.split_once(':')?;
            Some((
                hour.parse::<i32>().ok()? * 60 + minute.parse::<i32>().ok()?,
                false,
            ))
        })
        .unwrap_or((0, true));
    EventItem {
        id: occ.event.id as i32,
        title: occ.event.title.clone().into(),
        time_text: occ.event.time.clone().unwrap_or_default().into(),
        meta_text: event_meta_text(occ).into(),
        color,
        start_minutes,
        duration_minutes: if all_day {
            24 * 60
        } else {
            occ.event.duration_minutes.max(15) as i32
        },
        all_day,
        lane: 0,
        lane_count: 1,
    }
}

/// 为同一天相互重叠的时间段分配并排泳道；传递相交的事件归入同一组，
/// 组内使用最少可用泳道，保证周视图不会把重叠日程互相遮住。
fn layout_week_event_lanes(events: &mut [EventItem]) {
    let mut indices: Vec<usize> = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| (!event.all_day).then_some(index))
        .collect();
    indices.sort_by_key(|&index| events[index].start_minutes);

    let mut group_start = 0;
    while group_start < indices.len() {
        let first = indices[group_start];
        let mut group_end_minutes =
            events[first].start_minutes + events[first].duration_minutes.max(1);
        let mut group_end = group_start + 1;
        while group_end < indices.len()
            && events[indices[group_end]].start_minutes < group_end_minutes
        {
            let index = indices[group_end];
            group_end_minutes = group_end_minutes
                .max(events[index].start_minutes + events[index].duration_minutes.max(1));
            group_end += 1;
        }

        let mut lane_ends: Vec<i32> = Vec::new();
        for &index in &indices[group_start..group_end] {
            let start = events[index].start_minutes;
            let lane = lane_ends
                .iter()
                .position(|&end| end <= start)
                .unwrap_or_else(|| {
                    lane_ends.push(0);
                    lane_ends.len() - 1
                });
            lane_ends[lane] = start + events[index].duration_minutes.max(1);
            events[index].lane = lane as i32;
        }
        let lane_count = lane_ends.len().max(1) as i32;
        for &index in &indices[group_start..group_end] {
            events[index].lane_count = lane_count;
        }
        group_start = group_end;
    }
}

fn to_ui_todo(t: db::Todo) -> TodoItem {
    TodoItem {
        id: t.id as i32,
        title: t.title.into(),
        done: t.done,
        priority: t.priority as i32,
    }
}

fn to_ui_note(n: db::Note) -> NoteItem {
    NoteItem {
        id: n.id as i32,
        title: n.title.into(),
        content: n.content.into(),
    }
}

fn to_ui_course(course: db::Course) -> CourseItem {
    let color_index = course.color_index.clamp(0, 7) as usize;
    CourseItem {
        id: course.id as i32,
        title: course.title.into(),
        teacher: course.teacher.into(),
        location: course.location.into(),
        weekday: course.weekday as i32,
        start_period: course.start_period as i32,
        period_count: course.period_count as i32,
        start_week: course.start_week as i32,
        end_week: course.end_week as i32,
        color_index: color_index as i32,
        color: parse_hex_color(CALENDAR_COLOR_CYCLE[color_index]),
    }
}

fn to_ui_habit(h: db::Habit) -> HabitItem {
    HabitItem {
        id: h.id as i32,
        title: h.title.into(),
        streak: h.streak as i32,
        done_today: h.done_today,
    }
}

fn to_ui_record(event: db::Event, today: NaiveDate) -> RecordItem {
    let category = match event.category.as_str() {
        "birthday" => "生日",
        "anniversary" => "纪念日",
        "countdown" => "倒数日",
        _ => "记录",
    };
    let meta = if event.category == "countdown" {
        match NaiveDate::parse_from_str(&event.date, "%Y-%m-%d") {
            Ok(date) => {
                let days = (date - today).num_days();
                if days > 0 {
                    format!("还有 {days} 天")
                } else if days == 0 {
                    "就是今天".to_string()
                } else {
                    format!("已过 {} 天", -days)
                }
            }
            Err(_) => "日期格式错误".to_string(),
        }
    } else {
        recurrence::describe(&recurrence::RepeatRule::parse(&event.repeat_rule))
    };
    RecordItem {
        id: event.id as i32,
        title: event.title.into(),
        category: category.into(),
        date_text: event.date.into(),
        meta_text: meta.into(),
    }
}

fn to_ui_countdown(event: db::Event, today: NaiveDate) -> Option<CountdownItem> {
    let date = NaiveDate::parse_from_str(&event.date, "%Y-%m-%d").ok()?;
    let days = (date - today).num_days();
    Some(CountdownItem {
        id: event.id as i32,
        title: event.title.into(),
        days: days as i32,
        date_text: event.date.into(),
        color: parse_hex_color(
            CALENDAR_COLOR_CYCLE[(event.id.unsigned_abs() as usize) % CALENDAR_COLOR_CYCLE.len()],
        ),
    })
}

fn to_ui_shift_type(shift: db::ShiftType) -> ShiftTypeItem {
    let time_text = match (shift.start_time, shift.end_time) {
        (Some(start), Some(end)) => format!("{start}-{end}"),
        _ => "休息".to_string(),
    };
    ShiftTypeItem {
        id: shift.id as i32,
        name: shift.name.into(),
        time_text: time_text.into(),
        color: parse_hex_color(&shift.color),
    }
}

fn to_ui_shift_assignment(shift: db::ShiftAssignment) -> ShiftAssignmentItem {
    let time_text = match (shift.start_time, shift.end_time) {
        (Some(start), Some(end)) => format!("{start}-{end}"),
        _ => "休息".to_string(),
    };
    ShiftAssignmentItem {
        id: shift.id as i32,
        date_text: shift.shift_date.into(),
        name: shift.shift_name.into(),
        time_text: time_text.into(),
        color: parse_hex_color(&shift.color),
        is_rest: shift.is_rest,
    }
}

fn to_ui_calendar(c: &db::Calendar) -> CalendarItem {
    CalendarItem {
        id: c.id as i32,
        name: c.name.clone().into(),
        color: parse_hex_color(&c.color),
        visible: c.visible,
    }
}

/// 判断待办是否"紧急"（四象限视图的紧急维度）：已过期或今明两天到期都算紧急；
/// 没有截止日期的待办不算紧急（不能凭空紧急）。已完成的待办不参与四象限展示逻辑（调用方过滤）。
fn todo_is_urgent(due_date: &Option<String>, today: NaiveDate) -> bool {
    match due_date
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
    {
        Some(d) => d <= today + chrono::Duration::days(1),
        None => false,
    }
}

fn todo_due_text(due_date: &Option<String>, today: NaiveDate) -> String {
    match due_date
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
    {
        Some(d) if d < today => format!("已逾期 {d}"),
        Some(d) if d == today => "今天到期".to_string(),
        Some(d) => format!("{d} 到期"),
        None => String::new(),
    }
}

fn to_board_item(t: &db::Todo, today: NaiveDate) -> TodoBoardItem {
    TodoBoardItem {
        id: t.id as i32,
        title: t.title.clone().into(),
        due_text: todo_due_text(&t.due_date, today).into(),
        is_urgent: todo_is_urgent(&t.due_date, today),
        status: t.status.clone().into(),
    }
}

fn build_month_days(
    conn: &Connection,
    year: i32,
    month: u32,
    week_starts_sunday: bool,
    visible_ids: &HashSet<i64>,
    colors: &HashMap<i64, slint::Color>,
    today: NaiveDate,
) -> Vec<CalendarDay> {
    let first_of_month = NaiveDate::from_ymd_opt(year, month, 1).expect("非法年月");
    let leading = if week_starts_sunday {
        first_of_month.weekday().num_days_from_sunday()
    } else {
        first_of_month.weekday().num_days_from_monday()
    };
    let grid_start = first_of_month - chrono::Duration::days(leading as i64);
    let grid_end = grid_start + chrono::Duration::days(41);
    let occurrences: Vec<db::EventOccurrence> =
        db::list_event_occurrences(conn, grid_start, grid_end)
            .unwrap_or_default()
            .into_iter()
            .filter(|o| visible_ids.contains(&o.event.calendar_id))
            .collect();

    let mut days = Vec::with_capacity(42);
    let mut date = grid_start;
    for _ in 0..42 {
        let date_str = date.to_string();
        let mut day_occs: Vec<&db::EventOccurrence> = occurrences
            .iter()
            .filter(|o| o.occurrence_date == date_str)
            .collect();
        day_occs.sort_by(|a, b| a.event.time.cmp(&b.event.time));
        let tags: Vec<EventTag> = day_occs
            .iter()
            .take(2)
            .map(|o| EventTag {
                id: o.event.id as i32,
                title: o.event.title.clone().into(),
                color: colors
                    .get(&o.event.calendar_id)
                    .copied()
                    .unwrap_or_else(|| parse_hex_color("#2e6be6")),
            })
            .collect();
        days.push(CalendarDay {
            day: date.day() as i32,
            in_current_month: date.month() == month && date.year() == year,
            is_today: date == today,
            is_weekend: holidays::is_weekend(date),
            is_holiday: holidays::holiday_name(date).is_some(),
            is_makeup_workday: holidays::is_makeup_workday(date),
            lunar_text: lunar::short_label(date).into(),
            special_text: holidays::notable_day_label(date).unwrap_or_default().into(),
            has_events: !day_occs.is_empty(),
            week_number: date.iso_week().week() as i32,
            event_tags: ModelRc::new(VecModel::from(tags)),
            extra_count: day_occs.len().saturating_sub(2) as i32,
        });
        date += chrono::Duration::days(1);
    }
    days
}

fn month_end(year: i32, month: u32) -> NaiveDate {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, 1).expect("非法年月") - chrono::Duration::days(1)
}

/// 重新计算当前月份网格 + 选中日详情 + 今日日程（挂件用），并写回两个窗口的 Slint 属性。
fn refresh_all(ui: &AppWindow, widget: &WidgetWindow, state: &Rc<RefCell<AppState>>) {
    let (
        year,
        month,
        selected_day,
        days_model,
        events_for_day,
        todos_for_day,
        notes,
        habits,
        calendars_ui,
        calendar_names,
        subscription_items,
        selected_date_text,
        today_events,
        upcoming_events,
        today_full_text,
        week_title,
        week_days,
        timeline_title,
        timeline_days,
        weather_summary,
        search_query,
        search_results,
        records,
        shift_types,
        shift_assignments,
        shift_start_date,
        shift_end_date,
        shift_sequence,
        shift_result,
        today_todos,
        recent_notes,
        countdowns,
        year_months,
        board_todo_items,
        board_doing_items,
        board_done_items,
        board_urgent_important,
        board_not_urgent_important,
        board_urgent_not_important,
        board_not_urgent_not_important,
    ) = {
        let s = state.borrow();
        let year = s.year;
        let month = s.month;
        let selected_day = s.selected_day;
        let week_starts_sunday = s.week_starts_sunday;
        let conn = &s.conn;
        let today = Local::now().date_naive();

        let calendars = db::list_calendars(conn).unwrap_or_default();
        let colors: HashMap<i64, slint::Color> = calendars
            .iter()
            .map(|c| (c.id, parse_hex_color(&c.color)))
            .collect();
        let visible_ids: HashSet<i64> = db::visible_calendar_ids(conn).unwrap_or_default();
        let calendars_ui: Vec<CalendarItem> = calendars.iter().map(to_ui_calendar).collect();
        let calendar_names: Vec<SharedString> =
            calendars.iter().map(|c| c.name.clone().into()).collect();
        let subscription_items: Vec<SubscriptionItem> = db::list_subscriptions(conn)
            .unwrap_or_default()
            .into_iter()
            .map(|subscription| {
                let has_error = subscription.last_error.is_some();
                let (status, detail) = if let Some(error) = subscription.last_error {
                    ("同步失败".to_string(), error)
                } else if let Some(last_sync) = subscription.last_sync {
                    ("已同步".to_string(), format!("上次同步 {last_sync}"))
                } else {
                    ("待同步".to_string(), "已保存，等待首次同步".to_string())
                };
                SubscriptionItem {
                    id: subscription.id as i32,
                    name: subscription.name.into(),
                    status: status.into(),
                    detail: detail.into(),
                    has_error,
                }
            })
            .collect();

        let first_of_month = NaiveDate::from_ymd_opt(year, month, 1).expect("非法年月");
        let leading = if week_starts_sunday {
            first_of_month.weekday().num_days_from_sunday()
        } else {
            first_of_month.weekday().num_days_from_monday()
        };
        let mut grid_start = first_of_month;
        for _ in 0..leading {
            grid_start = grid_start.pred_opt().expect("日期下溢");
        }
        let mut grid_end = grid_start;
        for _ in 0..41 {
            grid_end = grid_end.succ_opt().expect("日期上溢");
        }

        // 只保留"可见"分类日历下的日程发生，隐藏的分类整体从所有视图消失。
        let occurrences_in_grid: Vec<db::EventOccurrence> =
            db::list_event_occurrences(conn, grid_start, grid_end)
                .unwrap_or_default()
                .into_iter()
                .filter(|o| visible_ids.contains(&o.event.calendar_id))
                .collect();

        let mut days = Vec::with_capacity(42);
        let mut date = grid_start;
        for _ in 0..42 {
            let date_str = date.to_string();
            let mut day_occs: Vec<&db::EventOccurrence> = occurrences_in_grid
                .iter()
                .filter(|o| o.occurrence_date == date_str)
                .collect();
            day_occs.sort_by(|a, b| a.event.time.cmp(&b.event.time));
            const MAX_TAGS: usize = 2;
            let tags: Vec<EventTag> = day_occs
                .iter()
                .take(MAX_TAGS)
                .map(|o| EventTag {
                    id: o.event.id as i32,
                    title: o.event.title.clone().into(),
                    color: colors
                        .get(&o.event.calendar_id)
                        .copied()
                        .unwrap_or_else(|| parse_hex_color("#2e6be6")),
                })
                .collect();
            let extra_count = day_occs.len().saturating_sub(MAX_TAGS) as i32;
            days.push(CalendarDay {
                day: date.day() as i32,
                in_current_month: date.month() == month && date.year() == year,
                is_today: date == today,
                is_weekend: holidays::is_weekend(date),
                is_holiday: holidays::holiday_name(date).is_some(),
                is_makeup_workday: holidays::is_makeup_workday(date),
                lunar_text: lunar::short_label(date).into(),
                special_text: holidays::notable_day_label(date).unwrap_or_default().into(),
                has_events: !day_occs.is_empty(),
                week_number: date.iso_week().week() as i32,
                event_tags: ModelRc::new(VecModel::from(tags)),
                extra_count,
            });
            date = date.succ_opt().expect("日期上溢");
        }

        let year_months: Vec<YearMonth> = (1..=12)
            .map(|month| YearMonth {
                month: month as i32,
                title: format!("{}月", month).into(),
                days: ModelRc::new(VecModel::from(build_month_days(
                    conn,
                    year,
                    month,
                    week_starts_sunday,
                    &visible_ids,
                    &colors,
                    today,
                ))),
            })
            .collect();

        let selected_date =
            NaiveDate::from_ymd_opt(year, month, selected_day).unwrap_or(first_of_month);
        let selected_date_str = selected_date.to_string();

        let events_for_day: Vec<EventItem> = occurrences_in_grid
            .iter()
            .filter(|o| o.occurrence_date == selected_date_str)
            .map(|o| to_ui_event_occ(o, &colors))
            .collect();

        // “今日日程”始终对应真实的今天，与用户当前浏览的月份无关，供桌面挂件展示。
        let today_str = today.to_string();
        let today_events: Vec<EventItem> = if grid_start <= today && today <= grid_end {
            occurrences_in_grid
                .iter()
                .filter(|o| o.occurrence_date == today_str)
                .map(|o| to_ui_event_occ(o, &colors))
                .collect()
        } else {
            db::list_event_occurrences(conn, today, today)
                .unwrap_or_default()
                .iter()
                .filter(|o| visible_ids.contains(&o.event.calendar_id))
                .map(|o| to_ui_event_occ(o, &colors))
                .collect()
        };

        // 桌面日程卡片同时显示接下来的安排。展开重复日程后取未来 90 天，
        // 日期写进 meta_text，专供紧凑卡片的“接下来”列表显示。
        let tomorrow = today + chrono::Duration::days(1);
        let future_end = today + chrono::Duration::days(90);
        let upcoming_events: Vec<EventItem> =
            db::list_event_occurrences(conn, tomorrow, future_end)
                .unwrap_or_default()
                .into_iter()
                .filter(|o| {
                    visible_ids.contains(&o.event.calendar_id) && o.event.category == "event"
                })
                .take(16)
                .map(|o| {
                    let occurrence_date = NaiveDate::parse_from_str(&o.occurrence_date, "%Y-%m-%d")
                        .unwrap_or(tomorrow);
                    let mut item = to_ui_event_occ(&o, &colors);
                    let date_label = if occurrence_date == tomorrow {
                        "明天".to_string()
                    } else {
                        format!("{}/{}", occurrence_date.month(), occurrence_date.day())
                    };
                    let time_label = o.event.time.as_deref().unwrap_or("全天");
                    item.meta_text = format!("{date_label} {time_label}").into();
                    item
                })
                .collect();

        let todos_for_day: Vec<TodoItem> = db::list_todos(conn, Some(selected_date), None)
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_todo)
            .collect();

        let today_todos: Vec<TodoItem> = db::list_todos(conn, Some(today), None)
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_todo)
            .collect();

        let notes: Vec<NoteItem> = db::list_notes(conn)
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_note)
            .collect();
        // 每条便签都对应一个独立桌面卡片；中转模型因此保留全部便签，
        // 而不是只截取列表预览用的前五条。
        let recent_notes: Vec<NoteItem> = notes.clone();

        let mut countdowns: Vec<CountdownItem> = db::list_special_events(conn)
            .unwrap_or_default()
            .into_iter()
            .filter(|event| event.category == "countdown")
            .filter_map(|event| to_ui_countdown(event, today))
            .collect();
        countdowns.sort_by_key(|item| (item.days < 0, item.days, item.id));
        countdowns.truncate(3);

        let habits: Vec<HabitItem> = db::list_habits(conn, false)
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_habit)
            .collect();

        let records: Vec<RecordItem> = db::list_special_events(conn)
            .unwrap_or_default()
            .into_iter()
            .map(|event| to_ui_record(event, today))
            .collect();
        let shift_types: Vec<ShiftTypeItem> = db::list_shift_types(conn)
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_shift_type)
            .collect();
        let shift_assignments: Vec<ShiftAssignmentItem> =
            db::list_shift_assignments(conn, first_of_month, month_end(year, month))
                .unwrap_or_default()
                .into_iter()
                .map(to_ui_shift_assignment)
                .collect();
        let shift_start_date = s.shift_start_date.clone();
        let shift_end_date = s.shift_end_date.clone();
        let shift_sequence = s.shift_sequence.clone();
        let shift_result = s.shift_result.clone();

        let weekday_cn = ["一", "二", "三", "四", "五", "六", "日"]
            [selected_date.weekday().num_days_from_monday() as usize];
        let lunar_text = lunar::full_text(selected_date);
        let day_ganzhi = almanac::day_ganzhi(selected_date);
        let selected_date_text =
            format!("{selected_date_str} 星期{weekday_cn}  {lunar_text}  [{day_ganzhi}日]");

        let today_weekday_cn = ["一", "二", "三", "四", "五", "六", "日"]
            [today.weekday().num_days_from_monday() as usize];
        let today_full_text = format!(
            "{} 星期{} {}",
            today,
            today_weekday_cn,
            lunar::short_label(today)
        );

        // -------- 周视图数据：以 week_anchor 所在的一周（7 天）为准，与月视图分开计算 --------
        let week_start = week_start_of(s.week_anchor, week_starts_sunday);
        let week_end = week_start + chrono::Duration::days(6);
        let occurrences_in_week: Vec<db::EventOccurrence> =
            if week_start >= grid_start && week_end <= grid_end {
                occurrences_in_grid.clone()
            } else {
                db::list_event_occurrences(conn, week_start, week_end)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|o| visible_ids.contains(&o.event.calendar_id))
                    .collect()
            };
        let mut week_days = Vec::with_capacity(7);
        let mut wd = week_start;
        for _ in 0..7 {
            let wd_str = wd.to_string();
            let mut events: Vec<EventItem> = occurrences_in_week
                .iter()
                .filter(|o| o.occurrence_date == wd_str)
                .map(|o| to_ui_event_occ(o, &colors))
                .collect();
            layout_week_event_lanes(&mut events);
            week_days.push(WeekDayItem {
                year: wd.year(),
                month: wd.month() as i32,
                day: wd.day() as i32,
                weekday_label: format!(
                    "周{}",
                    ["一", "二", "三", "四", "五", "六", "日"]
                        [wd.weekday().num_days_from_monday() as usize]
                )
                .into(),
                lunar_text: lunar::short_label(wd).into(),
                is_today: wd == today,
                is_selected: wd == selected_date,
                is_holiday: holidays::holiday_name(wd).is_some(),
                is_weekend: holidays::is_weekend(wd),
                is_makeup_workday: holidays::is_makeup_workday(wd),
                events: ModelRc::new(VecModel::from(events)),
            });
            wd = wd.succ_opt().expect("日期上溢");
        }
        let week_title = if week_start.month() == week_end.month() {
            format!(
                "{}年{}月{}日 - {}日",
                week_start.year(),
                week_start.month(),
                week_start.day(),
                week_end.day()
            )
        } else {
            format!(
                "{}年{}月{}日 - {}年{}月{}日",
                week_start.year(),
                week_start.month(),
                week_start.day(),
                week_end.year(),
                week_end.month(),
                week_end.day()
            )
        };

        // -------- 日/三日视图数据：以 timeline_anchor 为第一天，展开 1 或 3 天的小时时间轴 --------
        let timeline_day_count = if s.view_mode == 3 { 3 } else { 1 };
        let timeline_start = s.timeline_anchor;
        let timeline_end = timeline_start + chrono::Duration::days(timeline_day_count - 1);
        let occurrences_in_timeline: Vec<db::EventOccurrence> =
            if timeline_start >= grid_start && timeline_end <= grid_end {
                occurrences_in_grid.clone()
            } else {
                db::list_event_occurrences(conn, timeline_start, timeline_end)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|o| visible_ids.contains(&o.event.calendar_id))
                    .collect()
            };
        let mut timeline_days = Vec::with_capacity(timeline_day_count as usize);
        let mut td = timeline_start;
        for _ in 0..timeline_day_count {
            let td_str = td.to_string();
            let events: Vec<TimelineEvent> = occurrences_in_timeline
                .iter()
                .filter(|o| o.occurrence_date == td_str)
                .map(|o| {
                    let (start_minutes, all_day) = match &o.event.time {
                        Some(t) => {
                            let parts: Vec<&str> = t.split(':').collect();
                            let hh: i32 = parts.first().and_then(|v| v.parse().ok()).unwrap_or(9);
                            let mm: i32 = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
                            (hh * 60 + mm, false)
                        }
                        None => (0, true),
                    };
                    TimelineEvent {
                        id: o.event.id as i32,
                        title: o.event.title.clone().into(),
                        color: colors
                            .get(&o.event.calendar_id)
                            .copied()
                            .unwrap_or_else(|| parse_hex_color("#2e6be6")),
                        start_minutes,
                        duration_minutes: o.event.duration_minutes as i32,
                        all_day,
                    }
                })
                .collect();
            timeline_days.push(TimelineDay {
                year: td.year(),
                month: td.month() as i32,
                day: td.day() as i32,
                weekday_label: format!(
                    "周{}",
                    ["一", "二", "三", "四", "五", "六", "日"]
                        [td.weekday().num_days_from_monday() as usize]
                )
                .into(),
                lunar_text: lunar::short_label(td).into(),
                is_today: td == today,
                is_weekend: holidays::is_weekend(td),
                is_holiday: holidays::holiday_name(td).is_some(),
                events: ModelRc::new(VecModel::from(events)),
            });
            td = td.succ_opt().expect("日期上溢");
        }
        let timeline_title = if timeline_day_count == 1 {
            format!(
                "{}年{}月{}日",
                timeline_start.year(),
                timeline_start.month(),
                timeline_start.day()
            )
        } else {
            format!(
                "{}年{}月{}日 - {}日",
                timeline_start.year(),
                timeline_start.month(),
                timeline_start.day(),
                timeline_end.day()
            )
        };

        // -------- 天气：只读后台线程缓存的结果，绝不在这里发网络请求（避免卡界面）--------
        let weather_summary = weather::cached(conn)
            .map(|w| {
                let (desc, _) = weather::describe_code(w.code);
                format!("{} {:.0}°C {desc}", w.city, w.temp_c)
            })
            .unwrap_or_default();

        let search_query = s.search_query.clone();
        let search_results: Vec<SearchItem> = db::search(conn, &search_query, 50)
            .unwrap_or_default()
            .into_iter()
            .map(|hit| SearchItem {
                id: hit.id as i32,
                kind: hit.kind.into(),
                title: hit.title.into(),
                meta: hit.meta.into(),
                date: hit.date.into(),
            })
            .collect();

        // -------- 待办看板 / 四象限：基于全部未完成待办（已完成的单独放进看板"已完成"列，
        // 不出现在四象限里——已经做完的事没有"重不重要/急不急"的意义）。
        let all_todos = db::list_all_todos(conn).unwrap_or_default();
        let board_todo_items: Vec<TodoBoardItem> = all_todos
            .iter()
            .filter(|t| t.status == "todo")
            .map(|t| to_board_item(t, today))
            .collect();
        let board_doing_items: Vec<TodoBoardItem> = all_todos
            .iter()
            .filter(|t| t.status == "doing")
            .map(|t| to_board_item(t, today))
            .collect();
        let board_done_items: Vec<TodoBoardItem> = all_todos
            .iter()
            .filter(|t| t.status == "done")
            .map(|t| to_board_item(t, today))
            .collect();
        let not_done_todos: Vec<&db::Todo> =
            all_todos.iter().filter(|t| t.status != "done").collect();
        let board_urgent_important: Vec<TodoBoardItem> = not_done_todos
            .iter()
            .filter(|t| t.important && todo_is_urgent(&t.due_date, today))
            .map(|t| to_board_item(t, today))
            .collect();
        let board_not_urgent_important: Vec<TodoBoardItem> = not_done_todos
            .iter()
            .filter(|t| t.important && !todo_is_urgent(&t.due_date, today))
            .map(|t| to_board_item(t, today))
            .collect();
        let board_urgent_not_important: Vec<TodoBoardItem> = not_done_todos
            .iter()
            .filter(|t| !t.important && todo_is_urgent(&t.due_date, today))
            .map(|t| to_board_item(t, today))
            .collect();
        let board_not_urgent_not_important: Vec<TodoBoardItem> = not_done_todos
            .iter()
            .filter(|t| !t.important && !todo_is_urgent(&t.due_date, today))
            .map(|t| to_board_item(t, today))
            .collect();

        (
            year,
            month,
            selected_day,
            days,
            events_for_day,
            todos_for_day,
            notes,
            habits,
            calendars_ui,
            calendar_names,
            subscription_items,
            selected_date_text,
            today_events,
            upcoming_events,
            today_full_text,
            week_title,
            week_days,
            timeline_title,
            timeline_days,
            weather_summary,
            search_query,
            search_results,
            records,
            shift_types,
            shift_assignments,
            shift_start_date,
            shift_end_date,
            shift_sequence,
            shift_result,
            today_todos,
            recent_notes,
            countdowns,
            year_months,
            board_todo_items,
            board_doing_items,
            board_done_items,
            board_urgent_important,
            board_not_urgent_important,
            board_urgent_not_important,
            board_not_urgent_not_important,
        )
    };

    let month_title: SharedString = format!("{year}年 {month}月").into();

    ui.set_month_title(month_title.clone());
    ui.set_days(ModelRc::new(VecModel::from(days_model.clone())));
    ui.set_selected_day(selected_day as i32);
    ui.set_new_event_date_default(format!("{year:04}-{month:02}-{selected_day:02}").into());
    ui.set_selected_date_text(selected_date_text.clone().into());
    ui.set_events_for_day(ModelRc::new(VecModel::from(events_for_day)));
    ui.set_todos_for_day(ModelRc::new(VecModel::from(todos_for_day)));
    ui.set_notes(ModelRc::new(VecModel::from(notes)));
    ui.set_habits(ModelRc::new(VecModel::from(habits)));
    ui.set_calendars(ModelRc::new(VecModel::from(calendars_ui)));
    ui.set_calendar_names(ModelRc::new(VecModel::from(calendar_names)));
    ui.set_subscriptions(ModelRc::new(VecModel::from(subscription_items)));
    ui.set_week_title(week_title.into());
    ui.set_week_days(ModelRc::new(VecModel::from(week_days)));
    ui.set_timeline_title(timeline_title.into());
    ui.set_timeline_days(ModelRc::new(VecModel::from(timeline_days)));
    ui.set_weather_summary(weather_summary.clone().into());
    let today = Local::now().date_naive();
    let weekday_cn = [
        "星期一",
        "星期二",
        "星期三",
        "星期四",
        "星期五",
        "星期六",
        "星期日",
    ][today.weekday().num_days_from_monday() as usize];
    let month_cn = [
        "一月",
        "二月",
        "三月",
        "四月",
        "五月",
        "六月",
        "七月",
        "八月",
        "九月",
        "十月",
        "十一月",
        "十二月",
    ][today.month0() as usize];
    let year_days = if NaiveDate::from_ymd_opt(today.year(), 2, 29).is_some() {
        366
    } else {
        365
    };
    let elapsed_days = today.ordinal();
    let year_progress = elapsed_days as f32 / year_days as f32;
    let weather_fact = weather_summary
        .split_whitespace()
        .find(|part| part.contains('°'))
        .unwrap_or("--")
        .to_string();
    ui.set_today_day_text(today.day().to_string().into());
    ui.set_today_date_heading(format!("{month_cn} · {weekday_cn}").into());
    ui.set_today_toolbar_date(
        format!(
            "{}年{}月{}日 {weekday_cn}",
            today.year(),
            today.month(),
            today.day()
        )
        .into(),
    );
    ui.set_today_lunar_heading(lunar::full_text(today).into());
    ui.set_today_day_of_year_text(format!("{elapsed_days} 天").into());
    ui.set_today_week_number_text(format!("{} 周", today.iso_week().week()).into());
    ui.set_today_remaining_days_text(format!("{} 天", year_days - elapsed_days).into());
    ui.set_today_weather_fact_text(weather_fact.into());
    ui.set_today_year_progress(year_progress);
    ui.set_today_year_progress_text(format!("{:.1}%", year_progress * 100.0).into());
    ui.set_search_query(search_query.into());
    ui.set_search_results(ModelRc::new(VecModel::from(search_results)));
    ui.set_records(ModelRc::new(VecModel::from(records)));
    ui.set_shift_types(ModelRc::new(VecModel::from(shift_types)));
    ui.set_shift_assignments(ModelRc::new(VecModel::from(shift_assignments)));
    ui.set_shift_start_date(shift_start_date.into());
    ui.set_shift_end_date(shift_end_date.into());
    ui.set_shift_sequence(shift_sequence.into());
    ui.set_shift_result(shift_result.into());
    ui.set_calendar_year(year);
    ui.set_year_months(ModelRc::new(VecModel::from(year_months)));
    {
        let s = state.borrow();
        let courses = db::list_courses(&s.conn).unwrap_or_default();
        let active_count = courses
            .iter()
            .filter(|course| {
                course.start_week <= s.course_week as i64 && s.course_week as i64 <= course.end_week
            })
            .count() as i32;
        let actual_week = course_week_for_date(s.course_term_start, today);
        let (week_start, week_end) = course_week_range(s.course_term_start, s.course_week);
        ui.set_course_items(ModelRc::new(VecModel::from(
            courses.into_iter().map(to_ui_course).collect::<Vec<_>>(),
        )));
        ui.set_course_week(s.course_week);
        ui.set_course_actual_week(actual_week);
        ui.set_course_term_start(s.course_term_start.to_string().into());
        ui.set_course_active_count(active_count);
        ui.set_course_week_title(
            format!(
                "{}月{}日 – {}月{}日",
                week_start.month(),
                week_start.day(),
                week_end.month(),
                week_end.day()
            )
            .into(),
        );
    }
    {
        let s = state.borrow();
        ui.set_calculator_start(s.calculator_start.clone().into());
        ui.set_calculator_end(s.calculator_end.clone().into());
        ui.set_calculator_offset(s.calculator_offset.clone().into());
        ui.set_calculator_result(s.calculator_result.clone().into());
        ui.set_weather_city(
            db::get_setting(&s.conn, "weather_city", "北京")
                .unwrap_or_else(|_| "北京".to_string())
                .into(),
        );
        ui.set_ai_input(s.ai_input.clone().into());
        ui.set_ai_draft(s.ai_draft.clone().into());
        ui.set_ai_draft_title(s.ai_draft_title.clone().into());
        ui.set_ai_draft_date(s.ai_draft_date.clone().into());
        ui.set_ai_draft_time(s.ai_draft_time.clone().into());
        ui.set_ai_draft_reminder(s.ai_draft_reminder.clone().into());
        ui.set_subscription_name(s.subscription_name.clone().into());
        ui.set_subscription_url(s.subscription_url.clone().into());
    }
    ui.set_almanac_text(
        format!(
            "{}\n农历：{}\n日柱：{}日",
            selected_date_text,
            lunar::full_text(
                NaiveDate::from_ymd_opt(year, month, selected_day)
                    .unwrap_or_else(|| Local::now().date_naive())
            ),
            almanac::day_ganzhi(
                NaiveDate::from_ymd_opt(year, month, selected_day)
                    .unwrap_or_else(|| Local::now().date_naive())
            )
        )
        .into(),
    );
    ui.set_weather_status(if weather_summary.is_empty() {
        "暂无天气缓存，可在工具页刷新".to_string().into()
    } else {
        weather_summary.clone().into()
    });
    ui.set_board_todo_items(ModelRc::new(VecModel::from(board_todo_items)));
    ui.set_board_doing_items(ModelRc::new(VecModel::from(board_doing_items)));
    ui.set_board_done_items(ModelRc::new(VecModel::from(board_done_items)));
    ui.set_board_urgent_important(ModelRc::new(VecModel::from(board_urgent_important)));
    ui.set_board_not_urgent_important(ModelRc::new(VecModel::from(board_not_urgent_important)));
    ui.set_board_urgent_not_important(ModelRc::new(VecModel::from(board_urgent_not_important)));
    ui.set_board_not_urgent_not_important(ModelRc::new(VecModel::from(
        board_not_urgent_not_important,
    )));

    let todo_completed_count = today_todos.iter().filter(|todo| todo.done).count() as i32;
    let today = Local::now().date_naive();
    let weekday =
        ["一", "二", "三", "四", "五", "六", "日"][today.weekday().num_days_from_monday() as usize];
    widget.set_today_date_text(
        format!("{}月{}日 星期{}", today.month(), today.day(), weekday).into(),
    );
    widget.set_today_lunar_text(format!("农历{}", lunar::short_label(today)).into());
    widget.set_todo_completed_count(todo_completed_count);
    widget.set_month_title(month_title);
    widget.set_days(ModelRc::new(VecModel::from(days_model)));
    widget.set_selected_day(selected_day as i32);
    widget.set_today_events(ModelRc::new(VecModel::from(today_events)));
    widget.set_upcoming_events(ModelRc::new(VecModel::from(upcoming_events)));
    widget.set_today_todos(ModelRc::new(VecModel::from(today_todos)));
    widget.set_recent_notes(ModelRc::new(VecModel::from(recent_notes)));
    widget.set_countdowns(ModelRc::new(VecModel::from(countdowns)));
    widget.set_weather_text(weather_summary.into());
    widget.set_today_full_text(today_full_text.into());
    update_tool_status(ui, widget, state);
}

fn parse_ui_date(value: &str) -> std::result::Result<NaiveDate, String> {
    let value = value.trim();
    match value {
        "今天" => Ok(Local::now().date_naive()),
        "明天" => Ok(Local::now().date_naive() + chrono::Duration::days(1)),
        _ => NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| format!("日期格式错误：{value}（应为 YYYY-MM-DD）")),
    }
}

fn parse_widget_time(value: &str) -> Result<String> {
    let value = value.trim();
    let time = NaiveTime::parse_from_str(value, "%H:%M")
        .with_context(|| format!("时间格式错误：{value}（应为 HH:MM）"))?;
    Ok(time.format("%H:%M").to_string())
}

fn format_duration(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

fn stopwatch_seconds(state: &AppState, now: Instant) -> u64 {
    state.stopwatch_elapsed_secs
        + state
            .stopwatch_started_at
            .map(|started| now.duration_since(started).as_secs())
            .unwrap_or(0)
}

fn pomodoro_seconds(state: &AppState, now: Instant) -> u64 {
    state
        .pomodoro_end_at
        .map(|end| end.saturating_duration_since(now).as_secs())
        .unwrap_or(state.pomodoro_remaining_secs)
}

fn update_widget_day_progress(widget: &WidgetWindow, elapsed_seconds: u32) {
    let elapsed_seconds = elapsed_seconds.min(86_400);
    let remaining_seconds = 86_400_u32.saturating_sub(elapsed_seconds);
    let remaining_hours = remaining_seconds / 3_600;
    let remaining_minutes = (remaining_seconds % 3_600) / 60;
    let percent = elapsed_seconds as f32 / 86_400.0;
    widget.set_day_progress(percent);
    widget.set_day_progress_text(format!("今日已过 {:.0}%", percent * 100.0).into());
    widget.set_day_remaining_text(
        format!("剩余 {} 小时 {:02} 分", remaining_hours, remaining_minutes).into(),
    );
    sync_desktop_widgets(widget);
}

fn update_tool_status(ui: &AppWindow, widget: &WidgetWindow, state: &Rc<RefCell<AppState>>) {
    let now = Instant::now();
    let mut s = state.borrow_mut();
    let stopwatch = stopwatch_seconds(&s, now);
    let pomodoro = pomodoro_seconds(&s, now);
    if s.pomodoro_end_at.is_some() && pomodoro == 0 {
        s.pomodoro_end_at = None;
        s.pomodoro_remaining_secs = 0;
    }
    ui.set_stopwatch_text(format_duration(stopwatch).into());
    ui.set_stopwatch_running(s.stopwatch_started_at.is_some());
    ui.set_pomodoro_text(format!("{:02}:{:02}", pomodoro / 60, pomodoro % 60).into());
    ui.set_pomodoro_running(s.pomodoro_end_at.is_some());
    widget.set_focus_time_text(format!("{:02}:{:02}", pomodoro / 60, pomodoro % 60).into());
    widget.set_focus_task(s.focus_task.clone().into());
    widget.set_focus_round(s.focus_round);
    widget.set_focus_running(s.pomodoro_end_at.is_some());
    let focus_total = s.pomodoro_total_secs.max(60);
    widget.set_focus_progress(1.0 - pomodoro.min(focus_total) as f32 / focus_total as f32);
    widget.set_focus_total_minutes(((focus_total - pomodoro.min(focus_total)) / 60) as i32);

    let local = Local::now();
    let day_seconds = local.time().num_seconds_from_midnight() as f64;
    let days_in_year = if NaiveDate::from_ymd_opt(local.year(), 12, 31)
        .unwrap()
        .ordinal()
        == 366
    {
        366.0
    } else {
        365.0
    };
    ui.set_time_progress_text(
        format!(
            "今日 {:.1}% · 本年 {:.1}%",
            day_seconds / 86400.0 * 100.0,
            local.ordinal() as f64 / days_in_year * 100.0
        )
        .into(),
    );

    let cities = [("北京", 8), ("东京", 9), ("伦敦", 0), ("纽约", -4)];
    let world = cities
        .iter()
        .map(|(name, offset)| {
            let utc = chrono::Utc::now();
            let zone = chrono::FixedOffset::east_opt(*offset * 3600).unwrap();
            format!("{} {}", name, utc.with_timezone(&zone).format("%H:%M"))
        })
        .collect::<Vec<_>>()
        .join("  ");
    ui.set_world_clock_text(world.into());
    drop(s);
    sync_desktop_widgets(widget);
}

/// 托盘图标 + 右键菜单（显示主窗口 / 开关桌面挂件 / 退出）。
struct TrayHandles {
    _tray: TrayIcon,
    show_id: tray_icon::menu::MenuId,
    widget_id: tray_icon::menu::MenuId,
    quit_id: tray_icon::menu::MenuId,
}

fn build_tray_icon() -> Result<TrayHandles> {
    let icon = build_tray_icon_image().context("生成托盘图标失败")?;

    let menu = Menu::new();
    let show_item = MenuItem::new("显示主窗口", true, None);
    let widget_item = MenuItem::new("打开/关闭桌面挂件", true, None);
    let quit_item = MenuItem::new("退出", true, None);
    menu.append(&show_item).context("添加托盘菜单项失败")?;
    menu.append(&widget_item).context("添加托盘菜单项失败")?;
    menu.append(&PredefinedMenuItem::separator())
        .context("添加托盘菜单分隔线失败")?;
    menu.append(&quit_item).context("添加托盘菜单项失败")?;

    let show_id = show_item.id().clone();
    let widget_id = widget_item.id().clone();
    let quit_id = quit_item.id().clone();

    let tray = TrayIconBuilder::new()
        .with_tooltip("日历")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .build()
        .context("创建系统托盘图标失败")?;

    Ok(TrayHandles {
        _tray: tray,
        show_id,
        widget_id,
        quit_id,
    })
}

fn build_tray_icon_image() -> Result<Icon> {
    let (width, height) = (32u32, 32u32);
    let rgba = include_bytes!("../assets/timehub-logo-32.rgba").to_vec();
    Icon::from_rgba(rgba, width, height).context("RGBA 转换为托盘图标失败")
}
