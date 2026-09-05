use crate::*;

pub(crate) fn register_tool_callbacks(
    ui: &AppWindow,
    widget: &WidgetWindow,
    quick_panel: &QuickPanelWindow,
    state: &Rc<RefCell<AppState>>,
) {
    // -------- 个性化设置：一周起始日 / 周数显示 / 主题 --------
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_set_week_starts_sunday(move |sunday| {
            let previous = state.borrow().week_starts_sunday;
            let result = db::set_setting(
                &state.borrow().conn,
                "week_start",
                if sunday { "sun" } else { "mon" },
            );
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                match result {
                    Ok(()) => {
                        state.borrow_mut().week_starts_sunday = sunday;
                        ui.set_week_starts_sunday(sunday);
                        refresh_all(&ui, &widget, &state);
                    }
                    Err(error) => {
                        ui.set_week_starts_sunday(previous);
                        ui.set_action_message(format!("保存一周起始日失败：{error}").into());
                    }
                }
            }
        });
    }
    let weather_candidates = std::sync::Arc::new(std::sync::Mutex::new((
        String::new(),
        Vec::<weather::LocationCandidate>::new(),
    )));
    {
        let ui_weak = ui.as_weak();
        let weather_candidates = weather_candidates.clone();
        ui.on_search_weather_city(move |query| {
            let query = query.trim().to_string();
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            if query.is_empty() {
                ui.set_weather_city_search_error(true);
                ui.set_weather_city_search_status("请输入城市或区县名称".into());
                return;
            }
            ui.set_weather_city_searching(true);
            ui.set_weather_city_search_error(false);
            ui.set_weather_city_search_status("正在查找城市或区县…".into());
            ui.set_weather_city_candidates(ModelRc::new(VecModel::default()));

            let ui_weak = ui_weak.clone();
            let weather_candidates = weather_candidates.clone();
            std::thread::spawn(move || {
                let result = weather::search_locations(&query);
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak.upgrade() else {
                        return;
                    };
                    ui.set_weather_city_searching(false);
                    match result {
                        Ok(candidates) => {
                            let choices = candidates
                                .iter()
                                .take(4)
                                .enumerate()
                                .map(|(index, candidate)| WeatherLocationChoice {
                                    id: index as i32,
                                    label: candidate.label.clone().into(),
                                })
                                .collect::<Vec<_>>();
                            ui.set_weather_city_search_error(false);
                            ui.set_weather_city_search_status(
                                format!("找到 {} 个地点，请选择具体地区", choices.len()).into(),
                            );
                            ui.set_weather_city_candidates(ModelRc::new(VecModel::from(choices)));
                            if let Ok(mut stored) = weather_candidates.lock() {
                                *stored = (query, candidates);
                            }
                        }
                        Err(error) => {
                            if let Ok(mut stored) = weather_candidates.lock() {
                                stored.0.clear();
                                stored.1.clear();
                            }
                            ui.set_weather_city_search_error(true);
                            ui.set_weather_city_search_status(format!("未找到：{error}").into());
                            let message = error_reporter::record("天气地点查询失败", &error);
                            ui.set_action_message(message.into());
                        }
                    }
                });
            });
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let weather_candidates = weather_candidates.clone();
        ui.on_choose_weather_city(move |index| {
            let selection = weather_candidates.lock().ok().and_then(|stored| {
                stored
                    .1
                    .get(index as usize)
                    .cloned()
                    .map(|candidate| (stored.0.clone(), candidate))
            });
            let Some((query, candidate)) = selection else {
                return;
            };
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_weather_city_searching(true);
                ui.set_weather_city_search_error(false);
                ui.set_weather_city_search_status(format!("正在更新 {}…", candidate.label).into());
                ui.set_weather_city_candidates(ModelRc::new(VecModel::default()));
            }

            let ui_weak = ui_weak.clone();
            let widget_weak = widget_weak.clone();
            let quick_weak = quick_weak.clone();
            std::thread::spawn(move || {
                let result = db::open()
                    .and_then(|conn| weather::refresh_for_location(&conn, &query, &candidate));
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak.upgrade() else {
                        return;
                    };
                    ui.set_weather_city_searching(false);
                    match result {
                        Ok(current) => {
                            let (description, _) =
                                weather::describe_current(current.code, current.is_day);
                            let summary =
                                format!("{} {:.0}°C {description}", current.city, current.temp_c);
                            let source = if current.provider.is_empty() {
                                "天气服务"
                            } else {
                                current.provider.as_str()
                            };
                            ui.set_weather_city_search_error(false);
                            ui.set_weather_city_search_status(
                                format!("已切换至 {}", candidate.label).into(),
                            );
                            ui.set_weather_city(query.into());
                            ui.set_weather_summary(summary.clone().into());
                            ui.set_weather_status(
                                format!("{summary} · 已更新 {} · {source}", current.updated_at)
                                    .into(),
                            );
                            ui.set_action_message(
                                format!("天气已切换至 {}", candidate.label).into(),
                            );
                            if let Some(widget) = widget_weak.upgrade() {
                                apply_weather_to_widget(&widget, Some(&current));
                                sync_desktop_widgets(&widget);
                                if let Some(quick) = quick_weak.upgrade() {
                                    sync_quick_weather(&quick, &widget);
                                }
                            }
                        }
                        Err(error) => {
                            ui.set_weather_city_search_error(true);
                            ui.set_weather_city_search_status(format!("更新失败：{error}").into());
                            let message = error_reporter::record("天气更新失败", &error);
                            ui.set_action_message(message.into());
                        }
                    }
                });
            });
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_set_show_week_numbers(move |show| {
            let previous = state.borrow().show_week_numbers;
            let result = db::set_setting(
                &state.borrow().conn,
                "show_week_number",
                if show { "1" } else { "0" },
            );
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                match result {
                    Ok(()) => {
                        state.borrow_mut().show_week_numbers = show;
                        ui.set_show_week_numbers(show);
                        // 周数只是 Slint 对既有 CalendarDay.week-number 的显示开关。
                        update_tool_status(&ui, &widget, &state);
                    }
                    Err(error) => {
                        ui.set_show_week_numbers(previous);
                        ui.set_action_message(format!("保存周数设置失败：{error}").into());
                    }
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_set_theme(move |index| {
            let previous = ui_weak
                .upgrade()
                .map(|ui| ui.get_theme_index())
                .unwrap_or(0);
            let result = db::set_setting(&state.borrow().conn, "theme", &index.to_string());
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                if let Err(error) = result {
                    ui.set_theme_index(previous);
                    apply_theme(&ui, &widget, &quick, previous);
                    ui.set_action_message(format!("保存强调色失败：{error}").into());
                } else {
                    ui.set_theme_index(index);
                    apply_theme(&ui, &widget, &quick, index);
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_calculate_tool(move |operation, start, end, offset| {
            let result = match operation.as_str() {
                "diff" => match (parse_ui_date(start.as_str()), parse_ui_date(end.as_str())) {
                    (Ok(start), Ok(end)) => {
                        let diff = date_calc::diff(start, end);
                        format!(
                            "{} 到 {}：自然日 {} 天，工作日 {} 天（含开始日，不含结束日）。{}",
                            diff.start,
                            diff.end,
                            diff.calendar_days,
                            diff.workdays,
                            date_calc::coverage_note(start, end)
                        )
                    }
                    (Err(e), _) | (_, Err(e)) => e,
                },
                "calendar" | "workday" => {
                    match (parse_ui_date(start.as_str()), offset.trim().parse::<i64>()) {
                        (Ok(start), Ok(n)) => {
                            let date = if operation == "calendar" {
                                date_calc::add_calendar_days(start, n)
                            } else {
                                date_calc::add_workdays(start, n)
                            };
                            match date {
                                Ok(date) => format!(
                                    "从 {} {} {} 天：{}（不含起始日）{}",
                                    start,
                                    if operation == "calendar" {
                                        "起算自然日"
                                    } else {
                                        "起算工作日"
                                    },
                                    n,
                                    date,
                                    if operation == "workday" {
                                        format!("。{}", date_calc::coverage_note(start, date))
                                    } else {
                                        String::new()
                                    }
                                ),
                                Err(error) => error.to_string(),
                            }
                        }
                        (Err(e), _) => e,
                        (_, Err(_)) => "推算天数必须是整数，例如 10 或 -3".to_string(),
                    }
                }
                _ => "未知的日期计算操作".to_string(),
            };
            {
                let mut s = state.borrow_mut();
                s.calculator_start = start.to_string();
                s.calculator_end = end.to_string();
                s.calculator_offset = offset.to_string();
                s.calculator_result = result;
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
        ui.on_set_weather_city(move |city| {
            let city = city.trim().to_string();
            {
                let s = state.borrow();
                if let Err(e) = db::set_setting(&s.conn, "weather_city", &city) {
                    error_reporter::report("保存天气城市失败", &e);
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
                ui.set_weather_status(format!("正在查询“{city}”…").into());
                ui.set_action_message(format!("正在查询天气：{city}").into());
                ui.invoke_refresh_weather();
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        ui.on_refresh_weather(move || {
            if let Some(widget) = widget_weak.upgrade() {
                widget.set_weather_refreshing(true);
                widget.set_weather_refresh_error(false);
                if let Some(quick) = quick_weak.upgrade() {
                    sync_quick_weather(&quick, &widget);
                }
                sync_desktop_widgets(&widget);
            }
            let ui_weak = ui_weak.clone();
            let widget_weak = widget_weak.clone();
            let quick_weak = quick_weak.clone();
            std::thread::spawn(move || {
                let result = db::open().and_then(|conn| weather::refresh_once(&conn));
                let (weather_text, status_text, weather_data, action_text, refresh_failed) =
                    match result {
                        Ok(w) => {
                            let (desc, _) = weather::describe_current(w.code, w.is_day);
                            let text = format!("{} {:.0}°C {desc}", w.city, w.temp_c);
                            let source = if w.provider.is_empty() {
                                "天气服务"
                            } else {
                                w.provider.as_str()
                            };
                            (
                                Some(text.clone()),
                                format!("{text} · 已更新 {} · {source}", w.updated_at),
                                Some(w),
                                format!("天气更新成功：{text}"),
                                false,
                            )
                        }
                        Err(e) => {
                            error_reporter::record("天气刷新失败，继续使用缓存", &e);
                            let cached = db::open().ok().and_then(|conn| weather::cached(&conn));
                            match cached {
                                Some(w) => {
                                    let (desc, _) = weather::describe_current(w.code, w.is_day);
                                    let text = format!("{} {:.0}°C {desc}", w.city, w.temp_c);
                                    (
                                        Some(text.clone()),
                                        format!(
                                            "{text} · 使用 {} 缓存；刷新失败: {e:#}",
                                            w.updated_at
                                        ),
                                        Some(w),
                                        "天气更新失败，已继续使用缓存".to_string(),
                                        true,
                                    )
                                }
                                None => (
                                    None,
                                    format!("天气刷新失败: {e:#}"),
                                    None,
                                    "天气更新失败，请检查城市名称或网络".to_string(),
                                    true,
                                ),
                            }
                        }
                    };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_weather_status(status_text.into());
                        ui.set_action_message(action_text.into());
                    }
                    if let Some(weather_text) = weather_text.as_ref() {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.set_weather_summary(weather_text.clone().into());
                        }
                    }
                    if let Some(widget) = widget_weak.upgrade() {
                        widget.set_weather_refreshing(false);
                        widget.set_weather_refresh_error(refresh_failed);
                        if let Some(weather_text) = weather_text {
                            widget.set_weather_text(weather_text.into());
                        }
                        if weather_data.is_some() {
                            apply_weather_to_widget(&widget, weather_data.as_ref());
                        }
                        if let Some(quick) = quick_weak.upgrade() {
                            sync_quick_weather(&quick, &widget);
                        }
                        sync_desktop_widgets(&widget);
                    }
                });
            });
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_toggle_stopwatch(move || {
            {
                let mut s = state.borrow_mut();
                let now = Instant::now();
                if s.stopwatch_started_at.is_some() {
                    s.stopwatch_elapsed_secs = stopwatch_seconds(&s, now);
                    s.stopwatch_started_at = None;
                } else {
                    s.stopwatch_started_at = Some(now);
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
        ui.on_reset_stopwatch(move || {
            {
                let mut s = state.borrow_mut();
                s.stopwatch_elapsed_secs = 0;
                s.stopwatch_started_at = None;
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
        ui.on_toggle_pomodoro(move || {
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
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_reset_pomodoro(move || {
            {
                let mut s = state.borrow_mut();
                // “重置25分”是固定动作，不能沿用桌面专注控件上一次的自定义时长。
                s.pomodoro_total_secs = 25 * 60;
                s.pomodoro_remaining_secs = 25 * 60;
                s.pomodoro_end_at = None;
                if let Err(error) = db::set_setting(&s.conn, "focus_minutes", "25") {
                    error_reporter::report("保存番茄钟默认时长失败", &error);
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }

    // -------- 分类日历（工作/个人/家庭…，显示/隐藏筛选）--------
}
