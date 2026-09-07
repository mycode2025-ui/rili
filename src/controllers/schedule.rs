use crate::*;

pub(crate) fn register_schedule_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
) {
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_next_month(move || {
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
        ui.on_goto_today(move || {
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
        ui.on_select_day(move |day| {
            state.borrow_mut().selected_day = day.max(1) as u32;
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_open_event(move |id| {
            let (event, calendar_name) = {
                let s = state.borrow();
                let event = db::get_event(&s.conn, id as i64).ok().flatten();
                let calendar_name = event.as_ref().and_then(|event| {
                    db::list_calendars(&s.conn)
                        .ok()?
                        .into_iter()
                        .find(|calendar| calendar.id == event.calendar_id)
                        .map(|calendar| calendar.name)
                });
                (event, calendar_name)
            };
            if let (Some(ui), Some(event)) = (ui_weak.upgrade(), event) {
                let hinted_date = ui.get_event_open_date_hint().to_string();
                ui.set_event_open_date_hint("".into());
                let occurrence_date = NaiveDate::parse_from_str(&hinted_date, "%Y-%m-%d")
                    .ok()
                    .filter(|date| {
                        NaiveDate::parse_from_str(&event.date, "%Y-%m-%d")
                            .ok()
                            .is_some_and(|base| {
                                recurrence::occurrences_in_range(
                                    base,
                                    recurrence::RepeatRule::parse(&event.repeat_rule),
                                    *date,
                                    *date,
                                )
                                .contains(date)
                            })
                    })
                    .map(|date| date.to_string())
                    .unwrap_or_else(|| event.date.clone());
                ui.set_editing_event_id(id);
                ui.set_editor_occurrence_date(occurrence_date.into());
                ui.set_editor_can_delete(event.source_kind == "local");
                ui.set_editor_is_repeating(event.repeat_rule != "none");
                ui.set_editor_title(event.title.into());
                ui.set_editor_date(event.date.into());
                ui.set_editor_time(event.time.unwrap_or_default().into());
                ui.set_editor_reminder(event.reminder_offsets.into());
                ui.set_editor_repeat(event.repeat_rule.into());
                ui.set_editor_note(event.note.into());
                ui.set_editor_calendar_id(event.calendar_id as i32);
                ui.set_editor_calendar_name(
                    calendar_name.unwrap_or_else(|| "默认".to_string()).into(),
                );
                ui.set_editor_duration(event.duration_minutes as i32);
                ui.set_editor_error("".into());
                ui.set_new_event_open(true);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_save_event_dialog(
            move |id, title, date, time, reminder, repeat, note, calendar_id, duration| {
                let result = (|| -> Result<()> {
                    let title = title.trim();
                    if title.is_empty() {
                        anyhow::bail!("日程标题不能为空");
                    }
                    let date = parse_ui_date(date.as_str()).map_err(anyhow::Error::msg)?;
                    let time = (!time.trim().is_empty()).then_some(time.trim().to_string());
                    let repeat = match repeat.as_str() {
                        "daily" | "weekly" | "monthly" | "yearly" => repeat.as_str(),
                        _ => "none",
                    };
                    let calendar_id = {
                        let s = state.borrow();
                        db::list_calendars(&s.conn)?
                            .into_iter()
                            .find(|calendar| calendar.id == calendar_id as i64)
                            .map(|calendar| calendar.id)
                            .unwrap_or(1)
                    };
                    if id > 0 {
                        db::update_event(
                            &state.borrow().conn,
                            id as i64,
                            db::EventUpdate {
                                title: Some(title),
                                date: Some(date),
                                time: Some(time.as_deref()),
                                note: Some(note.trim()),
                                repeat_rule: Some(repeat),
                                reminder_offsets: Some(reminder.trim()),
                                calendar_id: Some(calendar_id),
                                duration_minutes: Some(duration.max(1) as i64),
                                ..db::EventUpdate::default()
                            },
                        )?;
                    } else {
                        db::create_event(
                            &state.borrow().conn,
                            db::NewEvent {
                                title,
                                date,
                                time: time.as_deref(),
                                note: note.trim(),
                                repeat_rule: repeat,
                                reminder_offsets: reminder.trim(),
                                category: "event",
                                calendar_id,
                            },
                        )?;
                        let created_id = state.borrow().conn.last_insert_rowid();
                        if duration != 60 {
                            db::update_event(
                                &state.borrow().conn,
                                created_id,
                                db::EventUpdate {
                                    duration_minutes: Some(duration.max(1) as i64),
                                    ..db::EventUpdate::default()
                                },
                            )?;
                        }
                    }
                    Ok(())
                })();
                let succeeded = result.is_ok();
                if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                    match &result {
                        Ok(()) => {
                            ui.set_editor_error("".into());
                            ui.set_action_message(
                                if id > 0 {
                                    "日程已更新"
                                } else {
                                    "日程已创建"
                                }
                                .into(),
                            );
                            refresh_all(&ui, &widget, &state);
                        }
                        Err(error) => {
                            let message = format!("保存失败：{error}");
                            ui.set_editor_error(message.clone().into());
                            ui.set_action_message(message.into());
                        }
                    }
                }
                succeeded
            },
        );
    }

    // -------- 视图切换：月/周/日/三日 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_set_view_mode(move |mode| {
            if mode == 10 {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_settings_open(true);
                }
                return;
            }
            let needs_refresh = {
                let mut s = state.borrow_mut();
                let previous_mode = s.view_mode;
                let previous_week_anchor = s.week_anchor;
                let previous_timeline_anchor = s.timeline_anchor;
                s.view_mode = mode;
                // 切换周、日、三日视图时，都从当前选中日期开始，避免沿用旧锚点跳到其他年份。
                if let Some(d) = NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day) {
                    if mode == 1 {
                        s.week_anchor = d;
                    }
                    if mode == 2 || mode == 3 {
                        s.timeline_anchor = d;
                    }
                }
                let anchor_changed = previous_week_anchor != s.week_anchor
                    || previous_timeline_anchor != s.timeline_anchor;
                navigation_refresh_needed(previous_mode, mode, anchor_changed)
            };
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                set_main_view_mode(&ui, mode);
                let timeline_mismatch = matches!(mode, 2 | 3)
                    && ui.get_timeline_days().row_count() != if mode == 3 { 3 } else { 1 };
                if needs_refresh || timeline_mismatch {
                    refresh_all(&ui, &widget, &state);
                } else {
                    refresh_navigation_page(&ui, &widget, &state);
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        ui.on_prepare_picker_date(move |value| {
            let fallback = {
                let s = state.borrow();
                NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day)
                    .unwrap_or_else(|| Local::now().date_naive())
            };
            let date = parse_ui_date(value.as_str()).unwrap_or(fallback);
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_picker_year(date.year());
                ui.set_picker_month(date.month() as i32);
                ui.set_picker_day(date.day() as i32);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_prepare_picker_time(move |value| {
            let time = NaiveTime::parse_from_str(value.trim(), "%H:%M")
                .unwrap_or_else(|_| NaiveTime::from_hms_opt(9, 0, 0).unwrap_or(NaiveTime::MIN));
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_picker_hour(time.hour() as i32);
                ui.set_picker_minute(time.minute() as i32);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_prev_week(move || {
            shift_week(&state, -1);
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_prev_year(move || {
            shift_year(&state, -1);
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }
}
