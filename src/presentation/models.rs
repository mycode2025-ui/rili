//! 数据库实体到 Slint 视图模型的映射。

use crate::*;

pub(crate) fn apply_weather_to_widget(
    widget: &WidgetWindow,
    current: Option<&weather::WeatherNow>,
) {
    let Some(current) = current else {
        widget.set_weather_city("".into());
        widget.set_weather_temperature("".into());
        widget.set_weather_description("".into());
        widget.set_weather_icon_kind("unknown".into());
        widget.set_weather_updated("等待后台刷新".into());
        widget.set_weather_days(ModelRc::new(VecModel::default()));
        return;
    };

    let (description, current_icon) = weather::describe_current(current.code, current.is_day);
    let updated = chrono::DateTime::parse_from_rfc3339(&current.updated_at)
        .map(|value| {
            value
                .with_timezone(&Local)
                .format("更新于 %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| "天气缓存".to_string());
    let forecast = current
        .daily
        .iter()
        .take(5)
        .map(|day| {
            let weekday = NaiveDate::parse_from_str(&day.date, "%Y-%m-%d")
                .map(|date| {
                    ["周一", "周二", "周三", "周四", "周五", "周六", "周日"]
                        [date.weekday().num_days_from_monday() as usize]
                })
                .unwrap_or("--");
            let (description, _) = weather::describe_code(day.code);
            WeatherDayItem {
                weekday: weekday.into(),
                description: description.into(),
                temperature: format!("{:.0}°/{:.0}°", day.temp_max, day.temp_min).into(),
                icon_kind: weather::icon_key(day.code).into(),
            }
        })
        .collect::<Vec<_>>();

    widget.set_weather_city(current.city.clone().into());
    widget.set_weather_temperature(format!("{:.0}°", current.temp_c).into());
    widget.set_weather_description(description.into());
    widget.set_weather_icon_kind(current_icon.into());
    widget.set_weather_updated(updated.into());
    widget.set_weather_days(ModelRc::new(VecModel::from(forecast)));
}

/// 将结构化天气数据复制到已打开的快速面板；不再依赖拼接字符串重新解析。
pub(crate) fn sync_quick_weather(quick: &QuickPanelWindow, widget: &WidgetWindow) {
    quick.set_weather_city(widget.get_weather_city());
    quick.set_weather_temperature(widget.get_weather_temperature());
    quick.set_weather_description(widget.get_weather_description());
    quick.set_weather_icon_kind(widget.get_weather_icon_kind());
}

pub(crate) fn refresh_quick_panel_calendar(
    quick: &QuickPanelWindow,
    state: &Rc<RefCell<AppState>>,
) {
    let today = Local::now().date_naive();
    let (year, month) = {
        let year = quick.get_browse_year();
        let month = quick.get_browse_month();
        if year == 0 || !(1..=12).contains(&month) {
            quick.set_browse_year(today.year());
            quick.set_browse_month(today.month() as i32);
            (today.year(), today.month())
        } else {
            (year, month as u32)
        }
    };
    let s = state.borrow();
    let calendars = db::list_calendars(&s.conn).unwrap_or_default();
    let colors: HashMap<i64, slint::Color> = calendars
        .iter()
        .map(|calendar| (calendar.id, parse_hex_color(&calendar.color)))
        .collect();
    let visible_ids: HashSet<i64> = db::visible_calendar_ids(&s.conn).unwrap_or_default();
    let days = build_month_days(
        &s.conn,
        year,
        month,
        s.week_starts_sunday,
        &visible_ids,
        &colors,
        today,
    );
    quick.set_days(ModelRc::new(VecModel::from(days)));
    quick.set_month_title(format!("{year}年 {month}月").into());
}

pub(crate) fn sync_quick_panel(
    quick: &QuickPanelWindow,
    ui: &AppWindow,
    widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
) {
    let today = Local::now().date_naive();
    let weekday = match today.weekday() {
        chrono::Weekday::Mon => "星期一",
        chrono::Weekday::Tue => "星期二",
        chrono::Weekday::Wed => "星期三",
        chrono::Weekday::Thu => "星期四",
        chrono::Weekday::Fri => "星期五",
        chrono::Weekday::Sat => "星期六",
        chrono::Weekday::Sun => "星期日",
    };
    refresh_quick_panel_calendar(quick, state);
    quick.set_events(ui.get_events_for_day());
    quick.set_todos(ui.get_todos_for_day());
    sync_quick_weather(quick, widget);
    quick.set_time_text(widget.get_current_time_text());
    quick.set_date_title(format!("{}月{}日", today.month(), today.day()).into());
    quick.set_weekday_text(weekday.into());
    quick.set_lunar_text(widget.get_today_lunar_text());
    quick.set_countdown_count(widget.get_countdowns().row_count() as i32);
    quick.set_active_view(ui.get_view_mode());
}

/// 把逗号分隔的“提前 N 分钟”提醒偏移量转成中文说明，例如 "0,30,1440" -> "提醒：准时/提前30分钟/提前1天"。
pub(crate) fn format_offset_label(offsets: &str) -> String {
    let labels: Vec<String> = offsets
        .split(',')
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .map(|m| match m {
            0 => "准时".to_string(),
            n if n % 1440 == 0 => format!("提前{}天", n / 1440),
            n if n % 60 == 0 => format!("提前{}小时", n / 60),
            n => format!("提前{n}分钟"),
        })
        .collect();
    if labels.is_empty() {
        String::new()
    } else {
        format!("提醒：{}", labels.join("/"))
    }
}

/// 分类徽标：普通日程返回空字符串；生日/纪念日/倒数日返回带有具体年数/剩余天数的说明，
/// 例如"第8个生日""5周年""还有12天"。`occurrence_date` 是这条日程这一次具体落在哪天，
/// 与基准日期（`event.date`，通常是第一次发生）对比得出经过的年数。
pub(crate) fn category_badge(event: &db::Event, occurrence_date: NaiveDate) -> String {
    let base = NaiveDate::parse_from_str(&event.date, "%Y-%m-%d").ok();
    match event.category.as_str() {
        "birthday" => match base {
            Some(base) if occurrence_date.year() > base.year() => {
                format!("第{}个生日", occurrence_date.year() - base.year() + 1)
            }
            _ => "生日".to_string(),
        },
        "anniversary" => match base {
            Some(base) if occurrence_date.year() > base.year() => {
                format!("{}周年", occurrence_date.year() - base.year())
            }
            _ => "纪念日".to_string(),
        },
        "countdown" => {
            let today = Local::now().date_naive();
            let days = (occurrence_date - today).num_days();
            if days > 0 {
                format!("还有{days}天")
            } else if days == 0 {
                "就是今天".to_string()
            } else {
                format!("已过{}天", -days)
            }
        }
        _ => String::new(),
    }
}

pub(crate) fn event_meta_text(occ: &db::EventOccurrence) -> String {
    let occurrence_date =
        NaiveDate::parse_from_str(&occ.occurrence_date, "%Y-%m-%d").unwrap_or(NaiveDate::MIN);
    let rule = recurrence::RepeatRule::parse(&occ.event.repeat_rule);
    let parts: Vec<String> = [
        category_badge(&occ.event, occurrence_date),
        recurrence::describe(&rule),
        format_offset_label(&occ.event.reminder_offsets),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect();
    parts.join("  ")
}

pub(crate) fn to_ui_event_occ(
    occ: &db::EventOccurrence,
    colors: &HashMap<i64, slint::Color>,
) -> EventItem {
    let color = colors
        .get(&occ.event.calendar_id)
        .copied()
        .unwrap_or_else(|| parse_hex_color("#2e6be6"));
    let (start_minutes, all_day) = occ
        .event
        .time
        .as_deref()
        .and_then(|time| {
            let (hour, minute) = time.split_once(':')?;
            Some((
                hour.parse::<i32>().ok()? * 60 + minute.parse::<i32>().ok()?,
                false,
            ))
        })
        .unwrap_or((0, true));
    EventItem {
        id: occ.event.id as i32,
        occurrence_date: occ.occurrence_date.clone().into(),
        title: occ.event.title.clone().into(),
        time_text: occ.event.time.clone().unwrap_or_default().into(),
        meta_text: event_meta_text(occ).into(),
        color,
        start_minutes,
        duration_minutes: if all_day {
            24 * 60
        } else {
            occ.event.duration_minutes.max(15) as i32
        },
        all_day,
        lane: 0,
        lane_count: 1,
    }
}

/// 为同一天相互重叠的时间段分配并排泳道；传递相交的事件归入同一组，
/// 组内使用最少可用泳道，保证周视图不会把重叠日程互相遮住。
pub(crate) fn layout_week_event_lanes(events: &mut [EventItem]) {
    let mut indices: Vec<usize> = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| (!event.all_day).then_some(index))
        .collect();
    indices.sort_by_key(|&index| events[index].start_minutes);

    let mut group_start = 0;
    while group_start < indices.len() {
        let first = indices[group_start];
        let mut group_end_minutes =
            events[first].start_minutes + events[first].duration_minutes.max(1);
        let mut group_end = group_start + 1;
        while group_end < indices.len()
            && events[indices[group_end]].start_minutes < group_end_minutes
        {
            let index = indices[group_end];
            group_end_minutes = group_end_minutes
                .max(events[index].start_minutes + events[index].duration_minutes.max(1));
            group_end += 1;
        }

        let mut lane_ends: Vec<i32> = Vec::new();
        for &index in &indices[group_start..group_end] {
            let start = events[index].start_minutes;
            let lane = lane_ends
                .iter()
                .position(|&end| end <= start)
                .unwrap_or_else(|| {
                    lane_ends.push(0);
                    lane_ends.len() - 1
                });
            lane_ends[lane] = start + events[index].duration_minutes.max(1);
            events[index].lane = lane as i32;
        }
        let lane_count = lane_ends.len().max(1) as i32;
        for &index in &indices[group_start..group_end] {
            events[index].lane_count = lane_count;
        }
        group_start = group_end;
    }
}

pub(crate) fn to_ui_todo(t: db::Todo) -> TodoItem {
    TodoItem {
        id: t.id as i32,
        title: t.title.into(),
        done: t.done,
        priority: t.priority as i32,
    }
}

pub(crate) fn to_ui_note(n: db::Note) -> NoteItem {
    NoteItem {
        id: n.id as i32,
        title: n.title.into(),
        content: n.content.into(),
    }
}

pub(crate) fn to_ui_course(course: db::Course) -> CourseItem {
    let color_index = course.color_index.clamp(0, 7) as usize;
    CourseItem {
        id: course.id as i32,
        title: course.title.into(),
        teacher: course.teacher.into(),
        location: course.location.into(),
        weekday: course.weekday as i32,
        start_period: course.start_period as i32,
        period_count: course.period_count as i32,
        start_week: course.start_week as i32,
        end_week: course.end_week as i32,
        color_index: color_index as i32,
        color: parse_hex_color(CALENDAR_COLOR_CYCLE[color_index]),
    }
}

pub(crate) fn to_ui_habit(h: db::Habit) -> HabitItem {
    HabitItem {
        id: h.id as i32,
        title: h.title.into(),
        streak: h.streak as i32,
        done_today: h.done_today,
    }
}

pub(crate) fn to_ui_record(event: db::Event, today: NaiveDate) -> RecordItem {
    let category = match event.category.as_str() {
        "birthday" => "生日",
        "anniversary" => "纪念日",
        "countdown" => "倒数日",
        _ => "记录",
    };
    let meta = if event.category == "countdown" {
        match NaiveDate::parse_from_str(&event.date, "%Y-%m-%d") {
            Ok(date) => {
                let days = (date - today).num_days();
                if days > 0 {
                    format!("还有 {days} 天")
                } else if days == 0 {
                    "就是今天".to_string()
                } else {
                    format!("已过 {} 天", -days)
                }
            }
            Err(_) => "日期格式错误".to_string(),
        }
    } else {
        recurrence::describe(&recurrence::RepeatRule::parse(&event.repeat_rule))
    };
    RecordItem {
        id: event.id as i32,
        title: event.title.into(),
        category: category.into(),
        date_text: event.date.into(),
        meta_text: meta.into(),
    }
}

pub(crate) fn to_ui_countdown(event: db::Event, today: NaiveDate) -> Option<CountdownItem> {
    let date = NaiveDate::parse_from_str(&event.date, "%Y-%m-%d").ok()?;
    let days = (date - today).num_days();
    Some(CountdownItem {
        id: event.id as i32,
        title: event.title.into(),
        days: days as i32,
        date_text: event.date.into(),
        color: parse_hex_color(
            CALENDAR_COLOR_CYCLE[(event.id.unsigned_abs() as usize) % CALENDAR_COLOR_CYCLE.len()],
        ),
    })
}

pub(crate) fn to_ui_shift_type(shift: db::ShiftType) -> ShiftTypeItem {
    let time_text = match (shift.start_time, shift.end_time) {
        (Some(start), Some(end)) => format!("{start}-{end}"),
        _ => "休息".to_string(),
    };
    ShiftTypeItem {
        id: shift.id as i32,
        name: shift.name.into(),
        time_text: time_text.into(),
        color: parse_hex_color(&shift.color),
    }
}

pub(crate) fn to_ui_shift_assignment(shift: db::ShiftAssignment) -> ShiftAssignmentItem {
    let time_text = match (shift.start_time, shift.end_time) {
        (Some(start), Some(end)) => format!("{start}-{end}"),
        _ => "休息".to_string(),
    };
    ShiftAssignmentItem {
        id: shift.id as i32,
        date_text: shift.shift_date.into(),
        name: shift.shift_name.into(),
        time_text: time_text.into(),
        color: parse_hex_color(&shift.color),
        is_rest: shift.is_rest,
    }
}

pub(crate) fn to_ui_calendar(c: &db::Calendar) -> CalendarItem {
    CalendarItem {
        id: c.id as i32,
        name: c.name.clone().into(),
        color: parse_hex_color(&c.color),
        visible: c.visible,
    }
}

/// 判断待办是否"紧急"（四象限视图的紧急维度）：已过期或今明两天到期都算紧急；
/// 没有截止日期的待办不算紧急（不能凭空紧急）。已完成的待办不参与四象限展示逻辑（调用方过滤）。
pub(crate) fn todo_is_urgent(due_date: &Option<String>, today: NaiveDate) -> bool {
    match due_date
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
    {
        Some(d) => d <= today + chrono::Duration::days(1),
        None => false,
    }
}

pub(crate) fn todo_due_text(due_date: &Option<String>, today: NaiveDate) -> String {
    match due_date
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
    {
        Some(d) if d < today => format!("已逾期 {d}"),
        Some(d) if d == today => "今天到期".to_string(),
        Some(d) => format!("{d} 到期"),
        None => String::new(),
    }
}

pub(crate) fn to_board_item(t: &db::Todo, today: NaiveDate) -> TodoBoardItem {
    TodoBoardItem {
        id: t.id as i32,
        title: t.title.clone().into(),
        due_text: todo_due_text(&t.due_date, today).into(),
        is_urgent: todo_is_urgent(&t.due_date, today),
        status: t.status.clone().into(),
    }
}

pub(crate) fn build_month_days(
    conn: &Connection,
    year: i32,
    month: u32,
    week_starts_sunday: bool,
    visible_ids: &HashSet<i64>,
    colors: &HashMap<i64, slint::Color>,
    today: NaiveDate,
) -> Vec<CalendarDay> {
    let Some(first_of_month) = NaiveDate::from_ymd_opt(year, month, 1) else {
        return Vec::new();
    };
    let leading = if week_starts_sunday {
        first_of_month.weekday().num_days_from_sunday()
    } else {
        first_of_month.weekday().num_days_from_monday()
    };
    let Some(grid_start) =
        first_of_month.checked_sub_signed(chrono::Duration::days(leading as i64))
    else {
        return Vec::new();
    };
    let Some(grid_end) = grid_start.checked_add_signed(chrono::Duration::days(41)) else {
        return Vec::new();
    };
    let occurrences: Vec<db::EventOccurrence> =
        db::list_event_occurrences(conn, grid_start, grid_end)
            .unwrap_or_default()
            .into_iter()
            .filter(|o| visible_ids.contains(&o.event.calendar_id))
            .collect();

    let mut days = Vec::with_capacity(42);
    let mut date = grid_start;
    for _ in 0..42 {
        let date_str = date.to_string();
        let mut day_occs: Vec<&db::EventOccurrence> = occurrences
            .iter()
            .filter(|o| o.occurrence_date == date_str)
            .collect();
        day_occs.sort_by(|a, b| a.event.time.cmp(&b.event.time));
        let tags: Vec<EventTag> = day_occs
            .iter()
            .take(2)
            .map(|o| EventTag {
                id: o.event.id as i32,
                title: o.event.title.clone().into(),
                color: colors
                    .get(&o.event.calendar_id)
                    .copied()
                    .unwrap_or_else(|| parse_hex_color("#2e6be6")),
            })
            .collect();
        days.push(CalendarDay {
            date: date_str.into(),
            day: date.day() as i32,
            in_current_month: date.month() == month && date.year() == year,
            is_today: date == today,
            is_weekend: holidays::is_weekend(date),
            is_holiday: holidays::holiday_name(date).is_some(),
            is_makeup_workday: holidays::is_makeup_workday(date),
            lunar_text: lunar::short_label(date).into(),
            special_text: holidays::notable_day_label(date).unwrap_or_default().into(),
            has_events: !day_occs.is_empty(),
            week_number: date.iso_week().week() as i32,
            event_tags: ModelRc::new(VecModel::from(tags)),
            extra_count: day_occs.len().saturating_sub(2) as i32,
        });
        let Some(next) = date.succ_opt() else {
            break;
        };
        date = next;
    }
    days
}

pub(crate) fn month_end(year: i32, month: u32) -> Option<NaiveDate> {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, 1)?.pred_opt()
}
