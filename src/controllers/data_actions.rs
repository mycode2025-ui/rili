use crate::*;

pub(crate) fn register_data_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
) {
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
                            db::NewEvent {
                                title: &title,
                                date,
                                time: None,
                                note: "",
                                repeat_rule,
                                reminder_offsets: reminder_offsets.as_str(),
                                category: category.as_str(),
                                calendar_id: calendar_id as i64,
                            },
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
        ui.on_delete_event_occurrence(move |id, occurrence_date, scope| {
            let result = NaiveDate::parse_from_str(occurrence_date.as_str(), "%Y-%m-%d")
                .map_err(anyhow::Error::from)
                .and_then(|date| {
                    let s = state.borrow();
                    if scope.as_str() == "future" {
                        db::delete_event_from_occurrence(&s.conn, id as i64, date)
                    } else {
                        db::delete_event_occurrence(&s.conn, id as i64, date)
                    }
                });
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                match result {
                    Ok(_) => {
                        refresh_all(&ui, &widget, &state);
                        ui.set_action_message(
                            if scope.as_str() == "future" {
                                "已删除当天及后续日程"
                            } else {
                                "已删除当天日程"
                            }
                            .into(),
                        );
                    }
                    Err(error) => ui.set_action_message(format!("删除日程失败：{error}").into()),
                }
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
                refresh_todos(&ui, &widget, &state);
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
                refresh_todos(&ui, &widget, &state);
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
                refresh_todos(&ui, &widget, &state);
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
                refresh_todos(&ui, &widget, &state);
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
                refresh_notes(&ui, &widget, &state);
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
                refresh_notes(&ui, &widget, &state);
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
                refresh_notes(&ui, &widget, &state);
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
                refresh_habits(&ui, &widget, &state);
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
                refresh_habits(&ui, &widget, &state);
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
                refresh_habits(&ui, &widget, &state);
            }
        });
    }
}
