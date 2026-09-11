#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! GUI 入口：装配 Slint 主窗口 + 桌面挂件窗口、把 SQLite 数据渲染到日历网格 / 侧边栏，
//! 并把 UI 回调接回数据库读写；同时负责启动后台提醒线程和系统托盘图标。
//! 命令行子命令入口见 `cli.rs`。

mod app_state;
mod controllers;
mod desktop;
mod font_settings;
#[cfg(test)]
mod navigation_performance_tests;
mod presentation;
mod runtime;
mod system_tray;
mod windowing;

slint::include_modules!();

use anyhow::{Context, Result};
use app_state::AppState;
use chrono::{Datelike, Local, NaiveDate, NaiveTime, Timelike};
use clap::Parser;
use desktop::*;
use presentation::*;
use runtime::*;
use rusqlite::Connection;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use windowing::{set_main_view_mode, show_and_focus_main_window};

use rili::window_policy::navigation_refresh_needed;
use rili::{
    almanac, app_paths, autostart, cli, date_calc, db, error_reporter, holidays, integrations,
    lunar, natural, recurrence, reminders, single_instance, system_theme, update, weather,
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
    let cli = cli::Cli::parse();
    let gui_launch = cli.command.is_none();
    // 保持 CONSOLE 子系统以保留 CLI 输出；只有无参数桌面启动才立即脱离控制台。
    // 从资源管理器/快捷方式启动时，Windows 创建的附带黑窗会随之关闭；从终端启动
    // GUI 时只分离当前进程，不会隐藏或关闭调用方终端。
    #[cfg(target_os = "windows")]
    if gui_launch {
        detach_console_for_gui();
    }
    if cli.command.is_some() {
        return cli::run(cli);
    }
    run_gui(cli.startup)
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

fn run_gui(startup: bool) -> Result<()> {
    // 登录启动和用户重复双击都不得创建第二套后台线程、托盘图标和桌面卡片。
    let Some(mut instance_guard) = single_instance::acquire()? else {
        return Ok(());
    };
    // 兼容旧版本留下的启动命令，并在 exe 被移动后修复注册表中的路径。
    if autostart::is_enabled() {
        if let Err(error) = autostart::set_enabled(true) {
            error_reporter::report("修复开机启动项失败", &error);
        }
    }
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
    let mut interface_font_family = db::get_setting(
        &conn,
        "interface_font_family",
        font_settings::DEFAULT_FONT_FAMILY,
    )
    .unwrap_or_else(|_| font_settings::DEFAULT_FONT_FAMILY.to_string());
    let custom_font_path = db::get_setting(&conn, "custom_font_path", "").unwrap_or_default();
    let interface_density: i32 = db::get_setting(&conn, "interface_density", "1")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(|v: i32| v.clamp(0, 2))
        .unwrap_or(1);
    let reduce_motion = db::get_setting(&conn, "reduce_motion", "0").unwrap_or_default() == "1";
    let notifications_enabled =
        db::get_setting(&conn, "notifications_enabled", "1").unwrap_or_default() != "0";
    let notification_style = reminders::NotificationStyle::from_setting(
        &db::get_setting(&conn, "notification_style", "standard").unwrap_or_default(),
    );
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
    // The renderer is initialized now, so the shared collection can safely be
    // extended before any note/todo text is laid out.
    font_settings::configure_unicode_fallbacks();
    let custom_font_error = if custom_font_path.is_empty() {
        None
    } else {
        match font_settings::register_custom_font(std::path::Path::new(&custom_font_path)) {
            Ok(family) => {
                interface_font_family = family;
                None
            }
            Err(error) => {
                interface_font_family = font_settings::DEFAULT_FONT_FAMILY.to_string();
                Some(format!("自定义字体加载失败，已恢复默认字体：{error}"))
            }
        }
    };
    {
        let ui_weak = ui.as_weak();
        instance_guard.on_activate(move || {
            let ui_weak = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    show_and_focus_main_window(&ui);
                }
            });
        });
    }
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
    let notification = NotificationWindow::new()?;
    let _notification_runtime = register_notification_runtime(&ui, &notification);
    // 首次运行保持桌面整洁；已有用户写入数据库的开关仍按原样恢复。
    let (widgets_visible, initial_widget_visibility, initial_click_through) =
        load_desktop_widget_startup_state(&state.borrow().conn)?;
    let widget_shown = Rc::new(Cell::new(widgets_visible));
    let desktop_widget_visibility = Rc::new(RefCell::new(initial_widget_visibility));
    let desktop_click_through = Rc::new(Cell::new(initial_click_through));
    widget_shown.set(widgets_visible && desktop_widget_visibility.borrow().any());
    let widget_pinned = db::get_setting(&state.borrow().conn, "widget_pinned", "0")
        .unwrap_or_else(|_| "0".to_string())
        == "1";

    {
        let state = state.borrow();
        restore_widget_window(&desktop_widgets.calendar, &state.conn, "calendar", 88, 58);
        restore_widget_window_size(&desktop_widgets.calendar, &state.conn, "calendar");
        restore_widget_window(&desktop_widgets.events, &state.conn, "events", 616, 58);
        restore_widget_window_size(&desktop_widgets.events, &state.conn, "events");
        restore_widget_window(
            &desktop_widgets.countdown,
            &state.conn,
            "countdown",
            1144,
            58,
        );
        restore_widget_window_size(&desktop_widgets.countdown, &state.conn, "countdown");
        restore_widget_window(&desktop_widgets.clock, &state.conn, "clock", 88, 502);
        restore_widget_window_size(&desktop_widgets.clock, &state.conn, "clock");
        restore_widget_window(&desktop_widgets.weather, &state.conn, "weather", 88, 740);
        restore_widget_window_size(&desktop_widgets.weather, &state.conn, "weather");
        restore_widget_window(&desktop_widgets.focus, &state.conn, "focus", 616, 502);
        restore_widget_window_size(&desktop_widgets.focus, &state.conn, "focus");
        restore_widget_window(&desktop_widgets.todo, &state.conn, "todo", 1144, 502);
        restore_widget_window_size(&desktop_widgets.todo, &state.conn, "todo");
        restore_widget_window(&desktop_widgets.quote, &state.conn, "quote", 616, 740);
        restore_widget_window_size(&desktop_widgets.quote, &state.conn, "quote");
        restore_widget_window(&desktop_widgets.almanac, &state.conn, "almanac", 1144, 740);
        restore_widget_window_size(&desktop_widgets.almanac, &state.conn, "almanac");
        // The quick panel is a taskbar flyout, not a freely positioned desktop
        // widget. Always start at the taskbar corner and never restore a stale
        // user-dragged position from older builds.
        position_quick_panel_at_taskbar(&quick_panel);
        let quick_panel_pinned = db::get_setting(&state.conn, "quick_panel_pinned", "0")? == "1";
        quick_panel.set_pinned(quick_panel_pinned);
        ui.set_quick_panel_pinned(quick_panel_pinned);

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
        desktop_widgets.quote.set_pinned(
            db::get_setting(
                &state.conn,
                "widget_quote_pinned",
                if widget_pinned { "1" } else { "0" },
            )? == "1",
        );
        desktop_widgets.almanac.set_pinned(
            db::get_setting(
                &state.conn,
                "widget_almanac_pinned",
                if widget_pinned { "1" } else { "0" },
            )? == "1",
        );
        macro_rules! restore_card_lock {
            ($window:expr, $instance:literal) => {
                $window.set_locked(
                    db::get_setting(
                        &state.conn,
                        concat!("widget_instance_", $instance, "_locked"),
                        "0",
                    )? == "1",
                );
            };
        }
        restore_card_lock!(desktop_widgets.calendar, "calendar:1");
        restore_card_lock!(desktop_widgets.events, "events:1");
        restore_card_lock!(desktop_widgets.countdown, "countdown:1");
        restore_card_lock!(desktop_widgets.clock, "clock:1");
        restore_card_lock!(desktop_widgets.weather, "weather:1");
        restore_card_lock!(desktop_widgets.focus, "focus:1");
        restore_card_lock!(desktop_widgets.todo, "todo:1");
        restore_card_lock!(desktop_widgets.quote, "quote:1");
        restore_card_lock!(desktop_widgets.almanac, "almanac:1");
    }
    DESKTOP_WIDGET_WINDOWS.with(|slot| {
        *slot.borrow_mut() = Some(desktop_widgets.clone());
    });

    ui.set_week_starts_sunday(week_starts_sunday);
    ui.set_show_week_numbers(show_week_numbers);
    ui.set_theme_index(theme_index);
    ui.set_visual_theme(visual_theme);
    ui.set_interface_font_size(interface_font_size);
    ui.set_interface_font_family(font_settings::choice_from_family(&interface_font_family).into());
    ui.set_interface_density(interface_density);
    ui.set_reduce_motion(reduce_motion);
    ui.set_notifications_enabled(notifications_enabled);
    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    ui.set_notification_style(notification_style.as_setting().into());
    ui.set_notification_effect_level(notification_style.effect_level());
    ui.set_default_event_reminder(default_event_reminder.into());
    ui.set_local_only(local_only);
    ui.set_taskbar_clock_enabled(taskbar_clock_enabled);
    ui.set_auto_start_enabled(autostart::is_enabled());
    {
        let (opacity, card_theme, card_accent) = desktop_widget_global_style(&state.borrow().conn);
        ui.set_widget_style_kind("global".into());
        ui.set_widget_style_name("统一默认".into());
        ui.set_widget_style_opacity(opacity);
        ui.set_widget_style_theme(card_theme);
        ui.set_widget_style_accent(card_accent);
    }
    {
        let visible = *desktop_widget_visibility.borrow();
        sync_desktop_visibility_to_ui(&ui, visible);
    }
    ui.set_desktop_click_through(desktop_click_through.get());
    widget.set_pinned(widget_pinned);
    apply_theme(&ui, &widget, &quick_panel, theme_index);
    apply_system_theme(
        &ui,
        &widget,
        &quick_panel,
        system_theme::apps_use_dark_mode(),
    );
    apply_visual_theme(&ui, &widget, &quick_panel, visual_theme);
    {
        let state = state.borrow();
        apply_all_desktop_widget_styles(&state.conn, visual_theme, theme_index);
    }
    apply_accessibility_preferences(
        &ui,
        &widget,
        &quick_panel,
        interface_font_size,
        interface_density,
        reduce_motion,
    );
    apply_font_family(&ui, &widget, &quick_panel, &interface_font_family);
    if let Some(message) = custom_font_error {
        ui.set_action_message(message.into());
    }

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
    if debug_view.as_deref() == Some("settings-appearance") {
        ui.set_settings_section(0);
        ui.set_settings_open(true);
    }
    if debug_view.as_deref() == Some("settings-privacy") {
        ui.set_settings_section(4);
        ui.set_settings_open(true);
    }
    if debug_view.as_deref() == Some("settings-update") {
        ui.set_settings_section(7);
        ui.set_settings_open(true);
        ui.set_update_state(3);
        ui.set_update_version("0.3.0".into());
        ui.set_update_notes("• 修复日程提醒\n• 改进日历交互\n• 优化同步稳定性".into());
        ui.set_update_github_url("https://github.com/mycode2025-ui/rili/releases".into());
        ui.set_update_gitee_url("https://gitee.com/mycode2025-ui/rili/releases".into());
    }
    if debug_view.as_deref() == Some("tools-then-today") {
        let ui_weak = ui.as_weak();
        slint::Timer::single_shot(Duration::from_millis(2500), move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.invoke_set_view_mode(2);
            }
        });
    }
    sync_quick_panel(&quick_panel, &ui, &widget, &state);
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
    controllers::appearance::register_appearance_callbacks(
        &ui,
        &widget,
        &quick_panel,
        &state,
        interface_font_family,
    );
    controllers::course::register_course_callbacks(&ui, &widget, &state);
    controllers::settings::register_settings_callbacks(&ui, &widget, &state);
    if std::env::var("TIMEHUB_DEBUG_VIEW").ok().as_deref() != Some("settings-update") {
        controllers::settings::start_update_check(ui.as_weak(), false);
    }
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
    controllers::widget_bridge::register_widget_bridge_callbacks(
        &ui,
        &widget,
        &desktop_widgets,
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

    let _system_event_runtime =
        register_system_event_runtime(&ui, &widget, &quick_panel, &state, taskbar_clock_enabled)?;
    let _realtime_runtime =
        start_realtime_runtime(&ui, &widget, &quick_panel, &desktop_widgets, &state);

    if !startup {
        ui.show()?;
    }
    slint::run_event_loop_until_quit()?;
    if ui.window().is_visible() {
        ui.hide()?;
    }
    Ok(())
}
