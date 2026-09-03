use crate::*;

pub(crate) fn register_planning_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
) {
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_search(move |query| {
            state.borrow_mut().search_query = query.to_string();
            if let Some(ui) = ui_weak.upgrade() {
                refresh_search(&ui, &state);
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
                    if let Err(e) = db::create_event(
                        &s.conn,
                        db::NewEvent {
                            title: &title,
                            date,
                            time: None,
                            note: "",
                            repeat_rule: repeat,
                            reminder_offsets: "",
                            category: &category,
                            calendar_id: 1,
                        },
                    ) {
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
}
