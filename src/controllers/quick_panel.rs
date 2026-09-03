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
                    error_reporter::report("快速面板更新待办失败", &e);
                }
            }
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                refresh_todos(&ui, &widget, &state);
                sync_quick_panel(&quick, &ui, &widget, &state);
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
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        quick_panel.on_prev_month(move || {
            if let Some(quick) = quick_weak.upgrade() {
                let (mut year, mut month) = (quick.get_browse_year(), quick.get_browse_month());
                if month <= 1 {
                    year -= 1;
                    month = 12;
                } else {
                    month -= 1;
                }
                quick.set_browse_year(year);
                quick.set_browse_month(month);
                refresh_quick_panel_calendar(&quick, &state);
            }
        });
    }
    {
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        quick_panel.on_next_month(move || {
            if let Some(quick) = quick_weak.upgrade() {
                let (mut year, mut month) = (quick.get_browse_year(), quick.get_browse_month());
                if month >= 12 {
                    year += 1;
                    month = 1;
                } else {
                    month += 1;
                }
                quick.set_browse_year(year);
                quick.set_browse_month(month);
                refresh_quick_panel_calendar(&quick, &state);
            }
        });
    }
    {
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        quick_panel.on_goto_today(move || {
            if let Some(quick) = quick_weak.upgrade() {
                let today = Local::now().date_naive();
                quick.set_browse_year(today.year());
                quick.set_browse_month(today.month() as i32);
                refresh_quick_panel_calendar(&quick, &state);
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
                let (year, month) = quick_weak
                    .upgrade()
                    .map(|quick| (quick.get_browse_year(), quick.get_browse_month() as u32))
                    .unwrap_or((s.year, s.month));
                if let Some(selected) = NaiveDate::from_ymd_opt(year, month, day as u32) {
                    s.year = year;
                    s.month = month;
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
