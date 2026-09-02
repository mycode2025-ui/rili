use crate::*;

pub(crate) fn register_timeline_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
) {
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
                    db::NewEvent {
                        title: "新日程",
                        date,
                        time: Some(&time),
                        note: "",
                        repeat_rule: "none",
                        reminder_offsets: &s.default_event_reminder,
                        category: "event",
                        calendar_id: 1,
                    },
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
                        db::EventUpdate {
                            time: Some(Some(&time)),
                            ..db::EventUpdate::default()
                        },
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
                    db::EventUpdate {
                        duration_minutes: Some(new_duration_minutes as i64),
                        ..db::EventUpdate::default()
                    },
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
}
