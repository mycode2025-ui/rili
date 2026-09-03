use crate::*;

pub(crate) fn register_widget_bridge_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
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
        let state = state.clone();
        widget.on_open_widget_settings(move |kind| {
            if let Some(ui) = ui_weak.upgrade() {
                if kind.as_str() == "weather" {
                    state.borrow_mut().view_mode = 6;
                    ui.set_view_mode(6);
                    ui.set_settings_open(false);
                    ui.set_action_message("已打开天气设置".into());
                    show_and_focus_main_window(&ui);
                    return;
                }
                let section = match kind.as_str() {
                    "calendar" | "events" => 1,
                    "countdown" | "focus" => 2,
                    _ => 0,
                };
                ui.set_settings_section(section);
                ui.set_settings_open(true);
                ui.set_action_message(format!("已打开{}挂件设置", kind).into());
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
