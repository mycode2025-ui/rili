use crate::*;

pub(crate) fn register_assistant_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
) {
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_set_todo_board_mode(move |mode| {
            state.borrow_mut().todo_board_mode = mode;
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_todo_board_mode(mode);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_move_todo_status(move |id, status| {
            let result = {
                let s = state.borrow();
                (|| -> Result<Option<String>> {
                    let previous = db::get_todo(&s.conn, id as i64)?.context("待办不存在")?;
                    let target = if status == "restore" {
                        previous.resume_status.as_str()
                    } else {
                        status.as_str()
                    };
                    db::set_todo_status(&s.conn, id as i64, target)?;
                    Ok((target == "done").then_some(previous.status))
                })()
            };
            if let Err(e) = &result {
                error_reporter::report("移动看板卡片失败", e);
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                if let Ok(previous) = result {
                    if let Some(previous) = previous {
                        ui.set_todo_undo_id(id);
                        ui.set_todo_undo_status(previous.into());
                    } else {
                        ui.set_todo_undo_id(0);
                    }
                }
                refresh_todos(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_open_todo_detail(move |id| {
            let result = db::get_todo(&state.borrow().conn, id as i64);
            match result {
                Ok(Some(todo)) => {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_todo_detail_id(id);
                        ui.set_todo_detail_title(todo.title.into());
                        ui.set_todo_detail_due(
                            todo.due_date
                                .map(|d| d.to_string())
                                .unwrap_or_else(|| "未设置截止日期".into())
                                .into(),
                        );
                        ui.set_todo_detail_status(
                            match todo.status.as_str() {
                                "doing" => "进行中",
                                "done" => "已完成",
                                _ => "待办",
                            }
                            .into(),
                        );
                        ui.set_todo_detail_open(true);
                    }
                }
                Ok(None) => {}
                Err(e) => error_reporter::report("读取待办详情失败", &e),
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_parse_natural(move |text| {
            let input = text.trim().to_string();
            let parsed = natural::parse(&input, Local::now().date_naive());
            {
                let mut s = state.borrow_mut();
                s.ai_input = input;
                match parsed {
                    Ok(draft) => {
                        let reminder = if draft.reminder.is_empty() {
                            s.default_event_reminder.clone()
                        } else {
                            draft.reminder
                        };
                        s.ai_draft = draft.explanation;
                        s.ai_draft_title = draft.title;
                        s.ai_draft_date = draft.date;
                        s.ai_draft_time = draft.time;
                        s.ai_draft_reminder = reminder;
                    }
                    Err(error) => {
                        s.ai_draft = format!("解析失败：{error}");
                        s.ai_draft_title.clear();
                        s.ai_draft_date.clear();
                        s.ai_draft_time.clear();
                        s.ai_draft_reminder.clear();
                    }
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
                let s = state.borrow();
                if !s.ai_draft_title.is_empty() {
                    ui.set_editor_error("".into());
                    ui.set_editor_title(s.ai_draft_title.clone().into());
                    ui.set_editor_date(s.ai_draft_date.clone().into());
                    ui.set_editor_time(s.ai_draft_time.clone().into());
                    ui.set_editor_reminder(s.ai_draft_reminder.clone().into());
                } else if ui.get_new_event_open() {
                    ui.set_editor_error(s.ai_draft.clone().into());
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_create_natural_event(move |title, date, time, reminder| {
            let result = (|| -> Result<()> {
                let date = parse_ui_date(date.as_str()).map_err(anyhow::Error::msg)?;
                let time = if time.trim().is_empty() {
                    None
                } else {
                    Some(parse_widget_time(time.as_str())?)
                };
                db::create_event(
                    &state.borrow().conn,
                    db::NewEvent {
                        title: title.trim(),
                        date,
                        time: time.as_deref(),
                        note: "",
                        repeat_rule: "none",
                        reminder_offsets: reminder.trim(),
                        category: "event",
                        calendar_id: 1,
                    },
                )?;
                Ok(())
            })();
            {
                let mut s = state.borrow_mut();
                match result {
                    Ok(()) => {
                        s.ai_draft = "已创建日程，可在日历中继续编辑".to_string();
                        s.ai_draft_title.clear();
                        s.ai_draft_date.clear();
                        s.ai_draft_time.clear();
                        s.ai_draft_reminder.clear();
                    }
                    Err(error) => s.ai_draft = format!("创建失败：{error}"),
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
}
