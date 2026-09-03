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
            {
                let mut s = state.borrow_mut();
                s.week_starts_sunday = sunday;
                let _ = db::set_setting(&s.conn, "week_start", if sunday { "sun" } else { "mon" });
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                ui.set_week_starts_sunday(sunday);
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let state = state.clone();
        ui.on_set_show_week_numbers(move |show| {
            {
                let mut s = state.borrow_mut();
                s.show_week_numbers = show;
                let _ = db::set_setting(&s.conn, "show_week_number", if show { "1" } else { "0" });
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                ui.set_show_week_numbers(show);
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        let quick_weak = quick_panel.as_weak();
        let state = state.clone();
        ui.on_set_theme(move |index| {
            {
                let s = state.borrow();
                let _ = db::set_setting(&s.conn, "theme", &index.to_string());
            }
            if let (Some(ui), Some(widget), Some(quick)) = (
                ui_weak.upgrade(),
                widget_weak.upgrade(),
                quick_weak.upgrade(),
            ) {
                ui.set_theme_index(index);
                apply_theme(&ui, &widget, &quick, index);
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
                            "{} 到 {}：自然日 {} 天，工作日 {} 天",
                            diff.start, diff.end, diff.calendar_days, diff.workdays
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
                                    "从 {} {} {} 天：{}",
                                    start,
                                    if operation == "calendar" {
                                        "起算自然日"
                                    } else {
                                        "起算工作日"
                                    },
                                    n,
                                    date
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
            {
                let s = state.borrow();
                if let Err(e) = db::set_setting(&s.conn, "weather_city", city.trim()) {
                    eprintln!("保存天气城市失败: {e}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                ui.invoke_refresh_weather();
                refresh_all(&ui, &widget, &state);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let widget_weak = widget.as_weak();
        ui.on_refresh_weather(move || {
            let ui_weak = ui_weak.clone();
            let widget_weak = widget_weak.clone();
            std::thread::spawn(move || {
                let result = db::open().and_then(|conn| weather::refresh_once(&conn));
                let (weather_text, status_text, weather_data) = match result {
                    Ok(w) => {
                        let (desc, _) = weather::describe_code(w.code);
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
                        )
                    }
                    Err(e) => {
                        eprintln!("天气刷新失败（继续使用缓存）: {e:#}");
                        let cached = db::open().ok().and_then(|conn| weather::cached(&conn));
                        match cached {
                            Some(w) => {
                                let (desc, _) = weather::describe_code(w.code);
                                let text = format!("{} {:.0}°C {desc}", w.city, w.temp_c);
                                (
                                    Some(text.clone()),
                                    format!("{text} · 使用 {} 缓存；刷新失败: {e:#}", w.updated_at),
                                    Some(w),
                                )
                            }
                            None => (None, format!("天气刷新失败: {e:#}"), None),
                        }
                    }
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_weather_status(status_text.into());
                    }
                    if let Some(weather_text) = weather_text {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.set_weather_summary(weather_text.clone().into());
                        }
                        if let Some(widget) = widget_weak.upgrade() {
                            widget.set_weather_text(weather_text.into());
                            apply_weather_to_widget(&widget, weather_data.as_ref());
                            sync_desktop_widgets(&widget);
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
                    eprintln!("保存番茄钟默认时长失败: {error}");
                }
            }
            if let (Some(ui), Some(widget)) = (ui_weak.upgrade(), widget_weak.upgrade()) {
                refresh_all(&ui, &widget, &state);
            }
        });
    }

    // -------- 分类日历（工作/个人/家庭…，显示/隐藏筛选）--------
}
