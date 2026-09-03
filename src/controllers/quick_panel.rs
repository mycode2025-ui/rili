use crate::*;

pub(crate) fn register_quick_panel_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    quick_panel: &QuickPanelWindow,
    state: &Rc<RefCell<AppState>>,
) {
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
                refresh_todos(&ui, &widget, &state);
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
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        quick_panel.on_prev_month(move || {
            shift_month(&state, -1);
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
        quick_panel.on_next_month(move || {
            shift_month(&state, 1);
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
        quick_panel.on_goto_today(move || {
            goto_today(&state);
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
        let quick_weak = quick_panel.as_weak();
        quick_panel.on_open_event(move |id, occurrence_date| {
            if let (Some(ui), Some(quick)) = (ui_weak.upgrade(), quick_weak.upgrade()) {
                ui.set_event_open_date_hint(occurrence_date);
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
                if let Some(selected) = NaiveDate::from_ymd_opt(s.year, s.month, day as u32) {
                    s.selected_day = day as u32;
                    s.timeline_anchor = selected;
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
    {
        let ui_weak = ui.as_weak();
        let quick_weak = quick_panel.as_weak();
        quick_panel.on_open_settings(move || {
            if let (Some(ui), Some(quick)) = (ui_weak.upgrade(), quick_weak.upgrade()) {
                ui.set_settings_section(0);
                ui.set_settings_open(true);
                show_and_focus_main_window(&ui);
                let _ = quick.hide();
            }
        });
    }
}
