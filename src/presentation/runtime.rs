//! 日期输入、计时器与实时状态更新。

use crate::*;

pub(crate) fn parse_ui_date(value: &str) -> std::result::Result<NaiveDate, String> {
    let value = value.trim();
    match value {
        "今天" => Ok(Local::now().date_naive()),
        "明天" => Ok(Local::now().date_naive() + chrono::Duration::days(1)),
        _ => NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| format!("日期格式错误：{value}（应为 YYYY-MM-DD）")),
    }
}

pub(crate) fn parse_widget_time(value: &str) -> Result<String> {
    let value = value.trim();
    let time = NaiveTime::parse_from_str(value, "%H:%M")
        .with_context(|| format!("时间格式错误：{value}（应为 HH:MM）"))?;
    Ok(time.format("%H:%M").to_string())
}

pub(crate) fn format_duration(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

pub(crate) fn stopwatch_seconds(state: &AppState, now: Instant) -> u64 {
    state.stopwatch_elapsed_secs
        + state
            .stopwatch_started_at
            .map(|started| now.duration_since(started).as_secs())
            .unwrap_or(0)
}

pub(crate) fn pomodoro_seconds(state: &AppState, now: Instant) -> u64 {
    state
        .pomodoro_end_at
        .map(|end| end.saturating_duration_since(now).as_secs())
        .unwrap_or(state.pomodoro_remaining_secs)
}

pub(crate) fn update_widget_day_progress(widget: &WidgetWindow, elapsed_seconds: u32) {
    let elapsed_seconds = elapsed_seconds.min(86_400);
    let remaining_seconds = 86_400_u32.saturating_sub(elapsed_seconds);
    let remaining_hours = remaining_seconds / 3_600;
    let remaining_minutes = (remaining_seconds % 3_600) / 60;
    let percent = elapsed_seconds as f32 / 86_400.0;
    widget.set_day_progress(percent);
    widget.set_day_progress_text(format!("今日已过 {:.0}%", percent * 100.0).into());
    widget.set_day_remaining_text(
        format!("剩余 {} 小时 {:02} 分", remaining_hours, remaining_minutes).into(),
    );
    sync_desktop_widgets(widget);
}

pub(crate) fn update_tool_status(
    ui: &AppWindow,
    widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
) {
    let now = Instant::now();
    let mut s = state.borrow_mut();
    let stopwatch = stopwatch_seconds(&s, now);
    let pomodoro = pomodoro_seconds(&s, now);
    if s.pomodoro_end_at.is_some() && pomodoro == 0 {
        s.pomodoro_end_at = None;
        s.pomodoro_remaining_secs = 0;
    }
    ui.set_stopwatch_text(format_duration(stopwatch).into());
    ui.set_stopwatch_running(s.stopwatch_started_at.is_some());
    ui.set_pomodoro_text(format!("{:02}:{:02}", pomodoro / 60, pomodoro % 60).into());
    ui.set_pomodoro_running(s.pomodoro_end_at.is_some());
    widget.set_focus_time_text(format!("{:02}:{:02}", pomodoro / 60, pomodoro % 60).into());
    widget.set_focus_task(s.focus_task.clone().into());
    widget.set_focus_round(s.focus_round);
    widget.set_focus_running(s.pomodoro_end_at.is_some());
    let focus_total = s.pomodoro_total_secs.max(60);
    widget.set_focus_progress(1.0 - pomodoro.min(focus_total) as f32 / focus_total as f32);
    widget.set_focus_total_minutes(((focus_total - pomodoro.min(focus_total)) / 60) as i32);

    let local = Local::now();
    let day_seconds = local.time().num_seconds_from_midnight() as f64;
    let days_in_year = if NaiveDate::from_ymd_opt(local.year(), 12, 31)
        .is_some_and(|date| date.ordinal() == 366)
    {
        366.0
    } else {
        365.0
    };
    ui.set_time_progress_text(
        format!(
            "今日 {:.1}% · 本年 {:.1}%",
            day_seconds / 86400.0 * 100.0,
            local.ordinal() as f64 / days_in_year * 100.0
        )
        .into(),
    );

    let cities = [("北京", 8), ("东京", 9), ("伦敦", 0), ("纽约", -4)];
    let world = cities
        .iter()
        .filter_map(|(name, offset)| {
            let utc = chrono::Utc::now();
            chrono::FixedOffset::east_opt(*offset * 3600)
                .map(|zone| format!("{} {}", name, utc.with_timezone(&zone).format("%H:%M")))
        })
        .collect::<Vec<_>>()
        .join("  ");
    ui.set_world_clock_text(world.into());
    drop(s);
    sync_desktop_widgets(widget);
}
