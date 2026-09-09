//! GUI runtime services that must stay alive for the whole event loop:
//! notifications, native system events, and visibility-aware live updates.

use crate::app_state::AppState;
use crate::desktop::*;
use crate::presentation::*;
use crate::system_tray::{build_tray_icon, TrayHandles};
use crate::windowing::*;
use crate::{AppWindow, NotificationWindow, QuickPanelWindow, WidgetWindow};
use chrono::{Local, Timelike};
use rili::{error_reporter, reminders, system_theme, weather};
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, SharedString};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tray_icon::menu::MenuEvent;
use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};

pub(crate) fn register_notification_runtime(
    ui: &AppWindow,
    notification: &NotificationWindow,
) -> Rc<slint::Timer> {
    let timer = Rc::new(slint::Timer::default());
    let alert_generation = Rc::new(Cell::new(0_u64));
    {
        let ui_weak = ui.as_weak();
        let notification_weak = notification.as_weak();
        let timer = timer.clone();
        let alert_generation = alert_generation.clone();
        ui.on_show_action_notification(move |message| {
            timer.stop();
            let generation = alert_generation.get().wrapping_add(1);
            alert_generation.set(generation);
            let is_reminder = message.starts_with("日程提醒：");
            let effect_level = ui_weak
                .upgrade()
                .map(|ui| ui.get_notification_effect_level())
                .unwrap_or(1);
            let display_duration = if is_reminder && effect_level >= 2 {
                Duration::from_secs(14)
            } else if is_reminder {
                Duration::from_secs(10)
            } else {
                Duration::from_secs(4)
            };
            let Some(notification) = notification_weak.upgrade() else {
                return;
            };
            if message.is_empty() {
                let _ = notification.hide();
                return;
            }
            if let Some(ui) = ui_weak.upgrade() {
                sync_notification_theme(&ui, &notification);
            }
            notification.set_message(message);
            if let Some(ui) = ui_weak.upgrade() {
                show_screen_notification(&notification, &ui);
            } else {
                return;
            }
            if is_reminder && effect_level >= 2 {
                play_notification_sound();
                if let Some(ui) = ui_weak.upgrade() {
                    flash_window_attention(&ui);
                }
                let base = notification.window().position();
                for (step, offset) in [6, -6, 5, -5, 3, -3, 0].into_iter().enumerate() {
                    let notification_weak = notification.as_weak();
                    let alert_generation = alert_generation.clone();
                    slint::Timer::single_shot(
                        Duration::from_millis((step as u64 + 1) * 55),
                        move || {
                            if alert_generation.get() != generation {
                                return;
                            }
                            if let Some(notification) = notification_weak.upgrade() {
                                notification
                                    .window()
                                    .set_position(slint::PhysicalPosition::new(
                                        base.x + offset,
                                        base.y,
                                    ));
                            }
                        },
                    );
                }
            }

            let ui_weak = ui_weak.clone();
            let notification_weak = notification.as_weak();
            timer.start(slint::TimerMode::SingleShot, display_duration, move || {
                if let Some(notification) = notification_weak.upgrade() {
                    let _ = notification.hide();
                }
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_action_message("".into());
                }
            });
        });
    }
    {
        let ui_weak = ui.as_weak();
        let alert_generation = alert_generation.clone();
        notification.on_close_requested(move || {
            alert_generation.set(alert_generation.get().wrapping_add(1));
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_action_message("".into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        error_reporter::install_gui_notifier(move |message| {
            let _ = ui_weak.upgrade_in_event_loop(move |ui| {
                ui.set_action_message(message.into());
            });
        });
    }
    {
        let ui_weak = ui.as_weak();
        reminders::install_gui_notifier(move |alert| {
            let result = ui_weak.upgrade_in_event_loop(move |ui| {
                ui.set_notification_effect_level(alert.style.effect_level());
                // Invoke the display callback directly. Relying on an indirect
                // `changed action-message` binding can lose a background event
                // while another transient status message is being updated.
                ui.invoke_show_action_notification(alert.message.into());
            });
            if let Err(error) = &result {
                error_reporter::record("日程提醒无法进入界面事件循环", error);
            }
            result.is_ok()
        });
    }
    timer
}

pub(crate) struct SystemEventRuntime {
    _tray: TrayHandles,
    _quick_focus_timer: slint::Timer,
    _native_smoke_timers: Vec<slint::Timer>,
}

pub(crate) fn register_system_event_runtime(
    ui: &AppWindow,
    widget: &WidgetWindow,
    quick_panel: &QuickPanelWindow,
    state: &Rc<RefCell<AppState>>,
    taskbar_clock_enabled: bool,
) -> anyhow::Result<SystemEventRuntime> {
    TASKBAR_CLOCK_HOOK_ENABLED.store(taskbar_clock_enabled, Ordering::Release);
    update_taskbar_clock_hit_rect();
    {
        let ui_weak = ui.as_weak();
        set_taskbar_clock_click_handler(move || {
            let _ = ui_weak.upgrade_in_event_loop(|ui| {
                ui.invoke_taskbar_clock_clicked();
            });
        });
    }
    spawn_taskbar_clock_click_hook();

    let tray = build_tray_icon()?;
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_tray_left_clicked(move || {
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                if quick.window().is_visible() {
                    let _ = quick.hide();
                } else {
                    sync_quick_panel(&quick, &ui, &widget, &state);
                    show_and_focus_quick_panel(&quick);
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_taskbar_clock_clicked(move || {
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                sync_quick_panel(&quick, &ui, &widget, &state);
                show_and_focus_quick_panel_at(&quick, last_clicked_taskbar_anchor());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_tray_show_main(move || {
            if let Some(ui) = ui_weak.upgrade() {
                show_and_focus_main_window(&ui);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_tray_toggle_widgets(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.invoke_toggle_widget();
            }
        });
    }
    ui.on_tray_quit(|| {
        slint::quit_event_loop().ok();
    });
    {
        let ui_weak = ui.as_weak();
        std::thread::spawn(move || {
            while let Ok(event) = TrayIconEvent::receiver().recv() {
                if matches!(
                    event,
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                ) {
                    let _ = ui_weak.upgrade_in_event_loop(|ui| {
                        ui.invoke_tray_left_clicked();
                    });
                }
            }
        });
    }
    {
        let show_id = tray.show_id.clone();
        let widget_id = tray.widget_id.clone();
        let quit_id = tray.quit_id.clone();
        let ui_weak = ui.as_weak();
        std::thread::spawn(move || {
            while let Ok(event) = MenuEvent::receiver().recv() {
                let action = if event.id == show_id {
                    0
                } else if event.id == widget_id {
                    1
                } else if event.id == quit_id {
                    2
                } else {
                    continue;
                };
                let _ = ui_weak.upgrade_in_event_loop(move |ui| match action {
                    0 => ui.invoke_tray_show_main(),
                    1 => ui.invoke_tray_toggle_widgets(),
                    _ => ui.invoke_tray_quit(),
                });
            }
        });
    }

    let quick_focus_timer = slint::Timer::default();
    {
        let quick_weak = quick_panel.as_weak();
        let quick_panel_had_focus = Rc::new(Cell::new(false));
        quick_focus_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(750),
            move || {
                if let Some(quick) = quick_weak.upgrade() {
                    if quick.window().is_visible() && !quick.get_pinned() {
                        let focused = quick
                            .window()
                            .with_winit_window(|native| native.has_focus())
                            .unwrap_or(false);
                        if focused {
                            quick_panel_had_focus.set(true);
                        } else if quick_panel_had_focus.replace(false) {
                            let _ = quick.hide();
                        }
                    } else {
                        quick_panel_had_focus.set(false);
                    }
                }
            },
        );
    }

    let native_smoke_timers = schedule_native_smoke(ui, quick_panel);

    Ok(SystemEventRuntime {
        _tray: tray,
        _quick_focus_timer: quick_focus_timer,
        _native_smoke_timers: native_smoke_timers,
    })
}

fn schedule_native_smoke(ui: &AppWindow, quick_panel: &QuickPanelWindow) -> Vec<slint::Timer> {
    if std::env::var_os("TIMEHUB_NATIVE_SMOKE").is_none() {
        return Vec::new();
    }

    quick_panel.set_pinned(true);
    let mut timers = Vec::with_capacity(2);
    let tray_timer = slint::Timer::default();
    {
        let ui_weak = ui.as_weak();
        tray_timer.start(
            slint::TimerMode::SingleShot,
            Duration::from_millis(700),
            move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_action_message("Windows 原生通知冒烟测试".into());
                    ui.invoke_tray_left_clicked();
                }
            },
        );
    }
    timers.push(tray_timer);

    let taskbar_timer = slint::Timer::default();
    {
        let ui_weak = ui.as_weak();
        taskbar_timer.start(
            slint::TimerMode::SingleShot,
            Duration::from_millis(1800),
            move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.invoke_taskbar_clock_clicked();
                }
            },
        );
    }
    timers.push(taskbar_timer);
    timers
}

pub(crate) fn start_realtime_runtime(
    ui: &AppWindow,
    widget: &WidgetWindow,
    quick_panel: &QuickPanelWindow,
    desktop_widgets: &Rc<DesktopWidgetWindows>,
    state: &Rc<RefCell<AppState>>,
) -> slint::Timer {
    let timer = slint::Timer::default();
    let widget_weak = widget.as_weak();
    let quick_weak = quick_panel.as_weak();
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let desktop_widgets = desktop_widgets.clone();
    let weather_revision = Rc::new(Cell::new(weather::cache_revision()));
    let system_dark = Rc::new(Cell::new(system_theme::apps_use_dark_mode()));

    update_tool_status(ui, widget, &state);
    let now = Local::now();
    let now_text: SharedString = now.format("%H:%M:%S").to_string().into();
    widget.set_current_time_text(now_text.clone());
    widget.set_current_time_main(now.format("%H:%M").to_string().into());
    widget.set_current_seconds(now.format("%S").to_string().into());
    widget.set_current_minutes((now.hour() * 60 + now.minute()) as i32);
    update_widget_day_progress(widget, now.time().num_seconds_from_midnight());
    quick_panel.set_time_text(now_text);
    ui.set_current_minutes((now.hour() * 60 + now.minute()) as i32);
    ui.set_current_time_text(now.format("%H:%M").to_string().into());

    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(1000),
        move || {
            let now = Local::now();
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let Some(widget) = widget_weak.upgrade() else {
                return;
            };
            let quick = quick_weak.upgrade();
            let ui_visible = ui.window().is_visible();
            let quick_visible = quick
                .as_ref()
                .is_some_and(|quick| quick.window().is_visible());
            let events_visible = desktop_widgets.events.window().is_visible();
            let clock_visible = desktop_widgets.clock.window().is_visible();
            let focus_visible = desktop_widgets.focus.window().is_visible();
            let realtime_visible = rili::window_policy::realtime_refresh_needed(
                ui_visible,
                quick_visible,
                events_visible,
                clock_visible,
                focus_visible,
            );
            let maintenance_tick = rili::window_policy::idle_maintenance_due(now.second());

            if realtime_visible {
                let now_text: SharedString = now.format("%H:%M:%S").to_string().into();
                widget.set_current_time_text(now_text.clone());
                widget.set_current_time_main(now.format("%H:%M").to_string().into());
                widget.set_current_seconds(now.format("%S").to_string().into());
                widget.set_current_minutes((now.hour() * 60 + now.minute()) as i32);
                if clock_visible {
                    update_widget_day_progress_source(
                        &widget,
                        now.time().num_seconds_from_midnight(),
                    );
                }
                if ui_visible || focus_visible {
                    update_tool_status_source(&ui, &widget, &state);
                }
                desktop_widgets.sync_realtime_from(&widget);
                if quick_visible {
                    if let Some(quick) = quick.as_ref() {
                        quick.set_time_text(now_text);
                    }
                }
                if ui_visible {
                    ui.set_current_minutes((now.hour() * 60 + now.minute()) as i32);
                    ui.set_current_time_text(now.format("%H:%M").to_string().into());
                }
            }

            if maintenance_tick && ui.get_taskbar_clock_enabled() {
                update_taskbar_clock_hit_rect();
            }
            if maintenance_tick {
                let current_system_dark = system_theme::apps_use_dark_mode();
                if current_system_dark != system_dark.get() {
                    system_dark.set(current_system_dark);
                    if let Some(quick) = quick.as_ref() {
                        apply_system_theme(&ui, &widget, quick, current_system_dark);
                    }
                }
            }
            if realtime_visible || maintenance_tick {
                let latest_revision = weather::cache_revision();
                if latest_revision != weather_revision.get() {
                    weather_revision.set(latest_revision);
                    let current = weather::cached(&state.borrow().conn);
                    apply_weather_to_widget(&widget, current.as_ref());
                    desktop_widgets.sync_weather_from(&widget);
                    if quick_visible {
                        if let Some(quick) = quick.as_ref() {
                            sync_quick_weather(quick, &widget);
                        }
                    }
                    if ui_visible {
                        if let Some(current) = current {
                            let (description, _) =
                                weather::describe_current(current.code, current.is_day);
                            ui.set_weather_summary(
                                format!("{} {:.0}°C {description}", current.city, current.temp_c)
                                    .into(),
                            );
                        }
                    }
                }
            }
        },
    );
    timer
}
