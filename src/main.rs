#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! GUI 入口：装配 Slint 主窗口 + 桌面挂件窗口、把 SQLite 数据渲染到日历网格 / 侧边栏，
//! 并把 UI 回调接回数据库读写；同时负责启动后台提醒线程和系统托盘图标。
//! 命令行子命令入口见 `cli.rs`。

mod app_state;
mod controllers;
mod desktop;
mod presentation;
mod system_tray;
mod windowing;

slint::include_modules!();

use anyhow::{Context, Result};
use app_state::AppState;
use chrono::{Datelike, Local, NaiveDate, NaiveTime, Timelike};
use clap::Parser;
use desktop::*;
use presentation::*;
use rusqlite::Connection;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use system_tray::*;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windowing::{set_main_view_mode, show_and_focus_main_window};

use rili::window_policy::navigation_refresh_needed;
use rili::{
    almanac, app_paths, autostart, cli, date_calc, db, holidays, integrations, lunar, natural,
    recurrence, reminders, weather,
};

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
        weather: db::get_setting(&state.borrow().conn, "widget_weather_visible", "0")? == "1",
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
        restore_widget_window(&desktop_widgets.weather, &state.conn, "weather", 88, 740);
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
        desktop_widgets.weather.set_pinned(
            db::get_setting(
                &state.conn,
                "widget_weather_pinned",
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
    ui.set_auto_start_enabled(autostart::is_enabled());
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
    if debug_view.as_deref() == Some("settings-privacy") {
        ui.set_settings_section(4);
        ui.set_settings_open(true);
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
    controllers::appearance::register_appearance_callbacks(&ui, &widget, &quick_panel, &state);
    controllers::course::register_course_callbacks(&ui, &widget, &state);
    controllers::settings::register_settings_callbacks(&ui, &widget, &state);
    controllers::widget_content::register_widget_content_callbacks(
        &ui,
        &widget,
        &state,
        &desktop_widget_visibility,
        &widget_shown,
    );
    controllers::schedule::register_schedule_callbacks(&ui, &widget, &state);
    controllers::planning::register_planning_callbacks(&ui, &widget, &state);
    controllers::timeline::register_timeline_callbacks(&ui, &widget, &state);
    controllers::assistant::register_assistant_callbacks(&ui, &widget, &state);
    controllers::integration::register_integration_callbacks(&ui, &widget, &state);
    controllers::desktop_cards::register_desktop_card_callbacks(
        &ui,
        &widget,
        &quick_panel,
        &desktop_widgets,
        &state,
        &desktop_widget_visibility,
        &widget_shown,
    );
    controllers::widget_bridge::register_widget_bridge_callbacks(
        &ui,
        &widget,
        &state,
        &widget_shown,
    );
    controllers::widget_manager::register_widget_manager_callbacks(
        controllers::WidgetControllerContext {
            ui: &ui,
            widget: &widget,
            quick_panel: &quick_panel,
            desktop_widgets: &desktop_widgets,
            state: &state,
            visibility: &desktop_widget_visibility,
            click_through: &desktop_click_through,
            shown: &widget_shown,
        },
    );
    controllers::quick_panel::register_quick_panel_callbacks(&ui, &widget, &quick_panel, &state);
    controllers::tools::register_tool_callbacks(&ui, &widget, &quick_panel, &state);
    controllers::data_actions::register_data_callbacks(&ui, &widget, &state);

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
                    if now.second().is_multiple_of(10) && ui.get_taskbar_clock_enabled() {
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
