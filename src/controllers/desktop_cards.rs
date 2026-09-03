use crate::*;

pub(crate) fn register_desktop_card_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    quick_panel: &QuickPanelWindow,
    desktop_widgets: &Rc<DesktopWidgetWindows>,
    state: &Rc<RefCell<AppState>>,
    desktop_widget_visibility: &Rc<RefCell<DesktopWidgetVisibility>>,
    widget_shown: &Rc<Cell<bool>>,
) {
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
                if let Err(error) = db::set_setting(
                    &state.conn,
                    "desktop_widgets_visible",
                    if any_visible { "1" } else { "0" },
                ) {
                    eprintln!("保存桌面卡片总开关失败: {error}");
                }
            });
        }};
    }

    wire_widget_chrome!(desktop_widgets.calendar, "calendar");
    wire_widget_chrome!(desktop_widgets.events, "events");
    wire_widget_chrome!(desktop_widgets.countdown, "countdown");
    wire_widget_chrome!(desktop_widgets.clock, "clock");
    wire_widget_chrome!(desktop_widgets.weather, "weather");
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
    macro_rules! forward_widget_settings {
        ($window:expr, $instance_key:literal) => {{
            let widget_weak = widget.as_weak();
            $window.on_open_widget_settings(move |_kind| {
                if let Some(widget) = widget_weak.upgrade() {
                    widget.invoke_open_widget_settings($instance_key.into());
                }
            });
        }};
    }

    forward_widget_settings!(desktop_widgets.calendar, "calendar:1");
    forward_widget_settings!(desktop_widgets.events, "events:1");
    forward_widget_settings!(desktop_widgets.countdown, "countdown:1");
    forward_widget_settings!(desktop_widgets.clock, "clock:1");
    forward_widget_settings!(desktop_widgets.weather, "weather:1");
    forward_widget_settings!(desktop_widgets.focus, "focus:1");
    forward_widget_settings!(desktop_widgets.todo, "todo:1");

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
        let windows_weak = Rc::downgrade(desktop_widgets);
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
        let windows_weak = Rc::downgrade(desktop_widgets);
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
                    db::create_event(
                        &s.conn,
                        db::NewEvent {
                            title,
                            date,
                            time: None,
                            note: "",
                            repeat_rule: "none",
                            reminder_offsets: "",
                            category: "countdown",
                            calendar_id: 1,
                        },
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
                        db::NewEvent {
                            title,
                            date,
                            time: Some(&normalized),
                            note: "",
                            repeat_rule: "none",
                            reminder_offsets: "0",
                            category: "event",
                            calendar_id: 1,
                        },
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
                    refresh_todos(&ui, &widget, &state);
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
}
