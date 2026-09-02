use crate::*;

pub(crate) fn register_course_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
) {
    // -------- 课程表：周次导航、学期起点与课程增删改 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_change_course_week(move |delta| {
            {
                let mut s = state.borrow_mut();
                s.course_week = (s.course_week + delta).clamp(1, 30);
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
        ui.on_goto_course_current_week(move || {
            {
                let mut s = state.borrow_mut();
                s.course_week =
                    course_week_for_date(s.course_term_start, Local::now().date_naive());
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
        ui.on_set_course_term_start(move |value| {
            let result =
                parse_ui_date(value.trim()).map_err(|_| "请输入有效日期（YYYY-MM-DD）".to_string());
            match result {
                Ok(date) => {
                    {
                        let mut s = state.borrow_mut();
                        s.course_term_start = date;
                        s.course_week = course_week_for_date(date, Local::now().date_naive());
                        if let Err(error) =
                            db::set_setting(&s.conn, "course_term_start", &date.to_string())
                        {
                            return format!("保存开学日失败：{error}").into();
                        }
                    }
                    if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                        ui.set_action_message("课程表开学日已更新".into());
                        refresh_all(&ui, &widget, &state);
                    }
                    SharedString::default()
                }
                Err(message) => message.into(),
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_save_course(
            move |id,
                  title,
                  teacher,
                  location,
                  weekday,
                  start_period,
                  period_count,
                  start_week,
                  end_week,
                  color_index| {
                let result = {
                    let s = state.borrow();
                    db::save_course(
                        &s.conn,
                        id as i64,
                        title.as_str(),
                        teacher.as_str(),
                        location.as_str(),
                        weekday as i64,
                        start_period as i64,
                        period_count as i64,
                        start_week as i64,
                        end_week as i64,
                        color_index as i64,
                    )
                };
                match result {
                    Ok(_) => {
                        if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade())
                        {
                            ui.set_action_message(
                                if id > 0 {
                                    "课程已更新"
                                } else {
                                    "课程已创建"
                                }
                                .into(),
                            );
                            refresh_all(&ui, &widget, &state);
                        }
                        SharedString::default()
                    }
                    Err(error) => error.to_string().into(),
                }
            },
        );
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_delete_course(move |id| {
            let result = {
                let s = state.borrow();
                db::delete_course(&s.conn, id as i64)
            };
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                ui.set_action_message(
                    match result {
                        Ok(_) => "课程已删除".to_string(),
                        Err(error) => format!("删除课程失败：{error}"),
                    }
                    .into(),
                );
                refresh_all(&ui, &widget, &state);
            }
        });
    }
}
