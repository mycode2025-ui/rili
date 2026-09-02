//! 聚合数据库数据并刷新主界面与桌面卡片。

use crate::*;

/// 重新计算当前月份网格 + 选中日详情 + 今日日程（挂件用），并写回两个窗口的 Slint 属性。
pub(crate) fn refresh_all(ui: &AppWindow, widget: &WidgetWindow, state: &Rc<RefCell<AppState>>) {
    let (
        year,
        month,
        selected_day,
        days_model,
        events_for_day,
        todos_for_day,
        notes,
        habits,
        calendars_ui,
        calendar_names,
        subscription_items,
        selected_date_text,
        today_events,
        upcoming_events,
        today_full_text,
        week_title,
        week_days,
        timeline_title,
        timeline_days,
        weather_summary,
        search_query,
        search_results,
        records,
        shift_types,
        shift_assignments,
        shift_start_date,
        shift_end_date,
        shift_sequence,
        shift_result,
        today_todos,
        recent_notes,
        countdowns,
        year_months,
        board_todo_items,
        board_doing_items,
        board_done_items,
        board_urgent_important,
        board_not_urgent_important,
        board_urgent_not_important,
        board_not_urgent_not_important,
    ) = {
        let s = state.borrow();
        let year = s.year;
        let month = s.month;
        let selected_day = s.selected_day;
        let week_starts_sunday = s.week_starts_sunday;
        let conn = &s.conn;
        let today = Local::now().date_naive();

        let calendars = db::list_calendars(conn).unwrap_or_default();
        let colors: HashMap<i64, slint::Color> = calendars
            .iter()
            .map(|c| (c.id, parse_hex_color(&c.color)))
            .collect();
        let visible_ids: HashSet<i64> = db::visible_calendar_ids(conn).unwrap_or_default();
        let calendars_ui: Vec<CalendarItem> = calendars.iter().map(to_ui_calendar).collect();
        let calendar_names: Vec<SharedString> =
            calendars.iter().map(|c| c.name.clone().into()).collect();
        let subscription_items: Vec<SubscriptionItem> = db::list_subscriptions(conn)
            .unwrap_or_default()
            .into_iter()
            .map(|subscription| {
                let has_error = subscription.last_error.is_some();
                let (status, detail) = if let Some(error) = subscription.last_error {
                    ("同步失败".to_string(), error)
                } else if let Some(last_sync) = subscription.last_sync {
                    ("已同步".to_string(), format!("上次同步 {last_sync}"))
                } else {
                    ("待同步".to_string(), "已保存，等待首次同步".to_string())
                };
                SubscriptionItem {
                    id: subscription.id as i32,
                    name: subscription.name.into(),
                    status: status.into(),
                    detail: detail.into(),
                    has_error,
                }
            })
            .collect();

        let Some(first_of_month) = NaiveDate::from_ymd_opt(year, month, 1) else {
            return;
        };
        let leading = if week_starts_sunday {
            first_of_month.weekday().num_days_from_sunday()
        } else {
            first_of_month.weekday().num_days_from_monday()
        };
        let Some(grid_start) =
            first_of_month.checked_sub_signed(chrono::Duration::days(leading as i64))
        else {
            return;
        };
        let Some(grid_end) = grid_start.checked_add_signed(chrono::Duration::days(41)) else {
            return;
        };

        // 只保留"可见"分类日历下的日程发生，隐藏的分类整体从所有视图消失。
        let occurrences_in_grid: Vec<db::EventOccurrence> =
            db::list_event_occurrences(conn, grid_start, grid_end)
                .unwrap_or_default()
                .into_iter()
                .filter(|o| visible_ids.contains(&o.event.calendar_id))
                .collect();

        let mut days = Vec::with_capacity(42);
        let mut date = grid_start;
        for _ in 0..42 {
            let date_str = date.to_string();
            let mut day_occs: Vec<&db::EventOccurrence> = occurrences_in_grid
                .iter()
                .filter(|o| o.occurrence_date == date_str)
                .collect();
            day_occs.sort_by(|a, b| a.event.time.cmp(&b.event.time));
            const MAX_TAGS: usize = 2;
            let tags: Vec<EventTag> = day_occs
                .iter()
                .take(MAX_TAGS)
                .map(|o| EventTag {
                    id: o.event.id as i32,
                    title: o.event.title.clone().into(),
                    color: colors
                        .get(&o.event.calendar_id)
                        .copied()
                        .unwrap_or_else(|| parse_hex_color("#2e6be6")),
                })
                .collect();
            let extra_count = day_occs.len().saturating_sub(MAX_TAGS) as i32;
            days.push(CalendarDay {
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
                extra_count,
            });
            let Some(next) = date.succ_opt() else {
                break;
            };
            date = next;
        }

        let year_months: Vec<YearMonth> = if s.view_mode == 5 {
            (1..=12)
                .map(|month| YearMonth {
                    month: month as i32,
                    title: format!("{}月", month).into(),
                    days: ModelRc::new(VecModel::from(build_month_days(
                        conn,
                        year,
                        month,
                        week_starts_sunday,
                        &visible_ids,
                        &colors,
                        today,
                    ))),
                })
                .collect()
        } else {
            Vec::new()
        };

        let selected_date =
            NaiveDate::from_ymd_opt(year, month, selected_day).unwrap_or(first_of_month);
        let selected_date_str = selected_date.to_string();

        let events_for_day: Vec<EventItem> = occurrences_in_grid
            .iter()
            .filter(|o| o.occurrence_date == selected_date_str)
            .map(|o| to_ui_event_occ(o, &colors))
            .collect();

        // “今日日程”始终对应真实的今天，与用户当前浏览的月份无关，供桌面挂件展示。
        let today_str = today.to_string();
        let today_events: Vec<EventItem> = if grid_start <= today && today <= grid_end {
            occurrences_in_grid
                .iter()
                .filter(|o| o.occurrence_date == today_str)
                .map(|o| to_ui_event_occ(o, &colors))
                .collect()
        } else {
            db::list_event_occurrences(conn, today, today)
                .unwrap_or_default()
                .iter()
                .filter(|o| visible_ids.contains(&o.event.calendar_id))
                .map(|o| to_ui_event_occ(o, &colors))
                .collect()
        };

        // 桌面日程卡片同时显示接下来的安排。展开重复日程后取未来 90 天，
        // 日期写进 meta_text，专供紧凑卡片的“接下来”列表显示。
        let tomorrow = today + chrono::Duration::days(1);
        let future_end = today + chrono::Duration::days(90);
        let upcoming_events: Vec<EventItem> =
            db::list_event_occurrences(conn, tomorrow, future_end)
                .unwrap_or_default()
                .into_iter()
                .filter(|o| {
                    visible_ids.contains(&o.event.calendar_id) && o.event.category == "event"
                })
                .take(16)
                .map(|o| {
                    let occurrence_date = NaiveDate::parse_from_str(&o.occurrence_date, "%Y-%m-%d")
                        .unwrap_or(tomorrow);
                    let mut item = to_ui_event_occ(&o, &colors);
                    let date_label = if occurrence_date == tomorrow {
                        "明天".to_string()
                    } else {
                        format!("{}/{}", occurrence_date.month(), occurrence_date.day())
                    };
                    let time_label = o.event.time.as_deref().unwrap_or("全天");
                    item.meta_text = format!("{date_label} {time_label}").into();
                    item
                })
                .collect();

        let todos_for_day: Vec<TodoItem> = db::list_todos(conn, Some(selected_date), None)
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_todo)
            .collect();

        let today_todos: Vec<TodoItem> = db::list_todos(conn, Some(today), None)
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_todo)
            .collect();

        let notes: Vec<NoteItem> = db::list_notes(conn)
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_note)
            .collect();
        // 每条便签都对应一个独立桌面卡片；中转模型因此保留全部便签，
        // 而不是只截取列表预览用的前五条。
        let recent_notes: Vec<NoteItem> = notes.clone();

        let mut countdowns: Vec<CountdownItem> = db::list_special_events(conn)
            .unwrap_or_default()
            .into_iter()
            .filter(|event| event.category == "countdown")
            .filter_map(|event| to_ui_countdown(event, today))
            .collect();
        countdowns.sort_by_key(|item| (item.days < 0, item.days, item.id));
        countdowns.truncate(3);

        let habits: Vec<HabitItem> = db::list_habits(conn, false)
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_habit)
            .collect();

        let records: Vec<RecordItem> = if s.view_mode == 8 {
            db::list_special_events(conn)
                .unwrap_or_default()
                .into_iter()
                .map(|event| to_ui_record(event, today))
                .collect()
        } else {
            Vec::new()
        };
        let shift_types: Vec<ShiftTypeItem> = if s.view_mode == 9 {
            db::list_shift_types(conn)
                .unwrap_or_default()
                .into_iter()
                .map(to_ui_shift_type)
                .collect()
        } else {
            Vec::new()
        };
        let shift_assignments: Vec<ShiftAssignmentItem> = if s.view_mode == 9 {
            db::list_shift_assignments(
                conn,
                first_of_month,
                month_end(year, month).unwrap_or(first_of_month),
            )
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_shift_assignment)
            .collect()
        } else {
            Vec::new()
        };
        let shift_start_date = s.shift_start_date.clone();
        let shift_end_date = s.shift_end_date.clone();
        let shift_sequence = s.shift_sequence.clone();
        let shift_result = s.shift_result.clone();

        let weekday_cn = ["一", "二", "三", "四", "五", "六", "日"]
            [selected_date.weekday().num_days_from_monday() as usize];
        let lunar_text = lunar::full_text(selected_date);
        let day_ganzhi = almanac::day_ganzhi(selected_date);
        let selected_date_text =
            format!("{selected_date_str} 星期{weekday_cn}  {lunar_text}  [{day_ganzhi}日]");

        let today_weekday_cn = ["一", "二", "三", "四", "五", "六", "日"]
            [today.weekday().num_days_from_monday() as usize];
        let today_full_text = format!(
            "{} 星期{} {}",
            today,
            today_weekday_cn,
            lunar::short_label(today)
        );

        // -------- 周视图数据：以 week_anchor 所在的一周（7 天）为准，与月视图分开计算 --------
        let week_start = week_start_of(s.week_anchor, week_starts_sunday);
        let week_end = week_start + chrono::Duration::days(6);
        let occurrences_in_week: Vec<db::EventOccurrence> =
            if week_start >= grid_start && week_end <= grid_end {
                occurrences_in_grid.clone()
            } else {
                db::list_event_occurrences(conn, week_start, week_end)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|o| visible_ids.contains(&o.event.calendar_id))
                    .collect()
            };
        let mut week_days = Vec::with_capacity(7);
        let mut wd = week_start;
        for _ in 0..7 {
            let wd_str = wd.to_string();
            let mut events: Vec<EventItem> = occurrences_in_week
                .iter()
                .filter(|o| o.occurrence_date == wd_str)
                .map(|o| to_ui_event_occ(o, &colors))
                .collect();
            layout_week_event_lanes(&mut events);
            week_days.push(WeekDayItem {
                year: wd.year(),
                month: wd.month() as i32,
                day: wd.day() as i32,
                weekday_label: format!(
                    "周{}",
                    ["一", "二", "三", "四", "五", "六", "日"]
                        [wd.weekday().num_days_from_monday() as usize]
                )
                .into(),
                lunar_text: lunar::short_label(wd).into(),
                is_today: wd == today,
                is_selected: wd == selected_date,
                is_holiday: holidays::holiday_name(wd).is_some(),
                is_weekend: holidays::is_weekend(wd),
                is_makeup_workday: holidays::is_makeup_workday(wd),
                events: ModelRc::new(VecModel::from(events)),
            });
            let Some(next) = wd.succ_opt() else {
                break;
            };
            wd = next;
        }
        let week_title = if week_start.month() == week_end.month() {
            format!(
                "{}年{}月{}日 - {}日",
                week_start.year(),
                week_start.month(),
                week_start.day(),
                week_end.day()
            )
        } else {
            format!(
                "{}年{}月{}日 - {}年{}月{}日",
                week_start.year(),
                week_start.month(),
                week_start.day(),
                week_end.year(),
                week_end.month(),
                week_end.day()
            )
        };

        // -------- 日/三日视图数据：以 timeline_anchor 为第一天，展开 1 或 3 天的小时时间轴 --------
        let timeline_day_count = if s.view_mode == 3 { 3 } else { 1 };
        let timeline_start = s.timeline_anchor;
        let timeline_end = timeline_start + chrono::Duration::days(timeline_day_count - 1);
        let occurrences_in_timeline: Vec<db::EventOccurrence> =
            if timeline_start >= grid_start && timeline_end <= grid_end {
                occurrences_in_grid.clone()
            } else {
                db::list_event_occurrences(conn, timeline_start, timeline_end)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|o| visible_ids.contains(&o.event.calendar_id))
                    .collect()
            };
        let mut timeline_days = Vec::with_capacity(timeline_day_count as usize);
        let mut td = timeline_start;
        for _ in 0..timeline_day_count {
            let td_str = td.to_string();
            let events: Vec<TimelineEvent> = occurrences_in_timeline
                .iter()
                .filter(|o| o.occurrence_date == td_str)
                .map(|o| {
                    let (start_minutes, all_day) = match &o.event.time {
                        Some(t) => {
                            let parts: Vec<&str> = t.split(':').collect();
                            let hh: i32 = parts.first().and_then(|v| v.parse().ok()).unwrap_or(9);
                            let mm: i32 = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
                            (hh * 60 + mm, false)
                        }
                        None => (0, true),
                    };
                    TimelineEvent {
                        id: o.event.id as i32,
                        title: o.event.title.clone().into(),
                        color: colors
                            .get(&o.event.calendar_id)
                            .copied()
                            .unwrap_or_else(|| parse_hex_color("#2e6be6")),
                        start_minutes,
                        duration_minutes: o.event.duration_minutes as i32,
                        all_day,
                    }
                })
                .collect();
            timeline_days.push(TimelineDay {
                year: td.year(),
                month: td.month() as i32,
                day: td.day() as i32,
                weekday_label: format!(
                    "周{}",
                    ["一", "二", "三", "四", "五", "六", "日"]
                        [td.weekday().num_days_from_monday() as usize]
                )
                .into(),
                lunar_text: lunar::short_label(td).into(),
                is_today: td == today,
                is_weekend: holidays::is_weekend(td),
                is_holiday: holidays::holiday_name(td).is_some(),
                events: ModelRc::new(VecModel::from(events)),
            });
            let Some(next) = td.succ_opt() else {
                break;
            };
            td = next;
        }
        let timeline_title = if timeline_day_count == 1 {
            format!(
                "{}年{}月{}日",
                timeline_start.year(),
                timeline_start.month(),
                timeline_start.day()
            )
        } else {
            format!(
                "{}年{}月{}日 - {}日",
                timeline_start.year(),
                timeline_start.month(),
                timeline_start.day(),
                timeline_end.day()
            )
        };

        // -------- 天气：只读后台线程缓存的结果，绝不在这里发网络请求（避免卡界面）--------
        let weather_summary = weather::cached(conn)
            .map(|w| {
                let (desc, _) = weather::describe_code(w.code);
                format!("{} {:.0}°C {desc}", w.city, w.temp_c)
            })
            .unwrap_or_default();

        let search_query = s.search_query.clone();
        let search_results: Vec<SearchItem> = if s.view_mode == 7 {
            db::search(conn, &search_query, 50)
                .unwrap_or_default()
                .into_iter()
                .map(|hit| SearchItem {
                    id: hit.id as i32,
                    kind: hit.kind.into(),
                    title: hit.title.into(),
                    meta: hit.meta.into(),
                    date: hit.date.into(),
                })
                .collect()
        } else {
            Vec::new()
        };

        // -------- 待办看板 / 四象限：基于全部未完成待办（已完成的单独放进看板"已完成"列，
        // 不出现在四象限里——已经做完的事没有"重不重要/急不急"的意义）。
        let all_todos = if s.view_mode == 4 {
            db::list_all_todos(conn).unwrap_or_default()
        } else {
            Vec::new()
        };
        let board_todo_items: Vec<TodoBoardItem> = all_todos
            .iter()
            .filter(|t| t.status == "todo")
            .map(|t| to_board_item(t, today))
            .collect();
        let board_doing_items: Vec<TodoBoardItem> = all_todos
            .iter()
            .filter(|t| t.status == "doing")
            .map(|t| to_board_item(t, today))
            .collect();
        let board_done_items: Vec<TodoBoardItem> = all_todos
            .iter()
            .filter(|t| t.status == "done")
            .map(|t| to_board_item(t, today))
            .collect();
        let not_done_todos: Vec<&db::Todo> =
            all_todos.iter().filter(|t| t.status != "done").collect();
        let board_urgent_important: Vec<TodoBoardItem> = not_done_todos
            .iter()
            .filter(|t| t.important && todo_is_urgent(&t.due_date, today))
            .map(|t| to_board_item(t, today))
            .collect();
        let board_not_urgent_important: Vec<TodoBoardItem> = not_done_todos
            .iter()
            .filter(|t| t.important && !todo_is_urgent(&t.due_date, today))
            .map(|t| to_board_item(t, today))
            .collect();
        let board_urgent_not_important: Vec<TodoBoardItem> = not_done_todos
            .iter()
            .filter(|t| !t.important && todo_is_urgent(&t.due_date, today))
            .map(|t| to_board_item(t, today))
            .collect();
        let board_not_urgent_not_important: Vec<TodoBoardItem> = not_done_todos
            .iter()
            .filter(|t| !t.important && !todo_is_urgent(&t.due_date, today))
            .map(|t| to_board_item(t, today))
            .collect();

        (
            year,
            month,
            selected_day,
            days,
            events_for_day,
            todos_for_day,
            notes,
            habits,
            calendars_ui,
            calendar_names,
            subscription_items,
            selected_date_text,
            today_events,
            upcoming_events,
            today_full_text,
            week_title,
            week_days,
            timeline_title,
            timeline_days,
            weather_summary,
            search_query,
            search_results,
            records,
            shift_types,
            shift_assignments,
            shift_start_date,
            shift_end_date,
            shift_sequence,
            shift_result,
            today_todos,
            recent_notes,
            countdowns,
            year_months,
            board_todo_items,
            board_doing_items,
            board_done_items,
            board_urgent_important,
            board_not_urgent_important,
            board_urgent_not_important,
            board_not_urgent_not_important,
        )
    };

    let month_title: SharedString = format!("{year}年 {month}月").into();

    ui.set_month_title(month_title.clone());
    ui.set_days(ModelRc::new(VecModel::from(days_model.clone())));
    ui.set_selected_day(selected_day as i32);
    ui.set_new_event_date_default(format!("{year:04}-{month:02}-{selected_day:02}").into());
    ui.set_selected_date_text(selected_date_text.clone().into());
    ui.set_events_for_day(ModelRc::new(VecModel::from(events_for_day)));
    ui.set_todos_for_day(ModelRc::new(VecModel::from(todos_for_day)));
    ui.set_notes(ModelRc::new(VecModel::from(notes)));
    ui.set_habits(ModelRc::new(VecModel::from(habits)));
    ui.set_calendars(ModelRc::new(VecModel::from(calendars_ui)));
    ui.set_calendar_names(ModelRc::new(VecModel::from(calendar_names)));
    ui.set_subscriptions(ModelRc::new(VecModel::from(subscription_items)));
    ui.set_week_title(week_title.into());
    ui.set_week_days(ModelRc::new(VecModel::from(week_days)));
    ui.set_timeline_title(timeline_title.into());
    ui.set_timeline_days(ModelRc::new(VecModel::from(timeline_days)));
    ui.set_weather_summary(weather_summary.clone().into());
    let today = Local::now().date_naive();
    let weekday_cn = [
        "星期一",
        "星期二",
        "星期三",
        "星期四",
        "星期五",
        "星期六",
        "星期日",
    ][today.weekday().num_days_from_monday() as usize];
    let month_cn = [
        "一月",
        "二月",
        "三月",
        "四月",
        "五月",
        "六月",
        "七月",
        "八月",
        "九月",
        "十月",
        "十一月",
        "十二月",
    ][today.month0() as usize];
    let year_days = if NaiveDate::from_ymd_opt(today.year(), 2, 29).is_some() {
        366
    } else {
        365
    };
    let elapsed_days = today.ordinal();
    let year_progress = elapsed_days as f32 / year_days as f32;
    let weather_fact = weather_summary
        .split_whitespace()
        .find(|part| part.contains('°'))
        .unwrap_or("--")
        .to_string();
    ui.set_today_day_text(today.day().to_string().into());
    ui.set_today_date_heading(format!("{month_cn} · {weekday_cn}").into());
    ui.set_today_toolbar_date(
        format!(
            "{}年{}月{}日 {weekday_cn}",
            today.year(),
            today.month(),
            today.day()
        )
        .into(),
    );
    ui.set_today_lunar_heading(lunar::full_text(today).into());
    ui.set_today_day_of_year_text(format!("{elapsed_days} 天").into());
    ui.set_today_week_number_text(format!("{} 周", today.iso_week().week()).into());
    ui.set_today_remaining_days_text(format!("{} 天", year_days - elapsed_days).into());
    ui.set_today_weather_fact_text(weather_fact.into());
    ui.set_today_year_progress(year_progress);
    ui.set_today_year_progress_text(format!("{:.1}%", year_progress * 100.0).into());
    ui.set_search_query(search_query.into());
    ui.set_search_results(ModelRc::new(VecModel::from(search_results)));
    ui.set_records(ModelRc::new(VecModel::from(records)));
    ui.set_shift_types(ModelRc::new(VecModel::from(shift_types)));
    ui.set_shift_assignments(ModelRc::new(VecModel::from(shift_assignments)));
    ui.set_shift_start_date(shift_start_date.into());
    ui.set_shift_end_date(shift_end_date.into());
    ui.set_shift_sequence(shift_sequence.into());
    ui.set_shift_result(shift_result.into());
    ui.set_calendar_year(year);
    ui.set_year_months(ModelRc::new(VecModel::from(year_months)));
    if state.borrow().view_mode == 12 {
        let s = state.borrow();
        let courses = db::list_courses(&s.conn).unwrap_or_default();
        let active_count = courses
            .iter()
            .filter(|course| {
                course.start_week <= s.course_week as i64 && s.course_week as i64 <= course.end_week
            })
            .count() as i32;
        let actual_week = course_week_for_date(s.course_term_start, today);
        let (week_start, week_end) = course_week_range(s.course_term_start, s.course_week);
        ui.set_course_items(ModelRc::new(VecModel::from(
            courses.into_iter().map(to_ui_course).collect::<Vec<_>>(),
        )));
        ui.set_course_week(s.course_week);
        ui.set_course_actual_week(actual_week);
        ui.set_course_term_start(s.course_term_start.to_string().into());
        ui.set_course_active_count(active_count);
        ui.set_course_week_title(
            format!(
                "{}月{}日 – {}月{}日",
                week_start.month(),
                week_start.day(),
                week_end.month(),
                week_end.day()
            )
            .into(),
        );
    }
    if state.borrow().view_mode == 6 {
        let s = state.borrow();
        ui.set_calculator_start(s.calculator_start.clone().into());
        ui.set_calculator_end(s.calculator_end.clone().into());
        ui.set_calculator_offset(s.calculator_offset.clone().into());
        ui.set_calculator_result(s.calculator_result.clone().into());
        ui.set_weather_city(
            db::get_setting(&s.conn, "weather_city", "北京")
                .unwrap_or_else(|_| "北京".to_string())
                .into(),
        );
        ui.set_ai_input(s.ai_input.clone().into());
        ui.set_ai_draft(s.ai_draft.clone().into());
        ui.set_ai_draft_title(s.ai_draft_title.clone().into());
        ui.set_ai_draft_date(s.ai_draft_date.clone().into());
        ui.set_ai_draft_time(s.ai_draft_time.clone().into());
        ui.set_ai_draft_reminder(s.ai_draft_reminder.clone().into());
        ui.set_subscription_name(s.subscription_name.clone().into());
        ui.set_subscription_url(s.subscription_url.clone().into());
    }
    ui.set_almanac_text(
        format!(
            "{}\n农历：{}\n日柱：{}日",
            selected_date_text,
            lunar::full_text(
                NaiveDate::from_ymd_opt(year, month, selected_day)
                    .unwrap_or_else(|| Local::now().date_naive())
            ),
            almanac::day_ganzhi(
                NaiveDate::from_ymd_opt(year, month, selected_day)
                    .unwrap_or_else(|| Local::now().date_naive())
            )
        )
        .into(),
    );
    ui.set_weather_status(if weather_summary.is_empty() {
        "暂无天气缓存，可在工具页刷新".to_string().into()
    } else {
        weather_summary.clone().into()
    });
    ui.set_board_todo_items(ModelRc::new(VecModel::from(board_todo_items)));
    ui.set_board_doing_items(ModelRc::new(VecModel::from(board_doing_items)));
    ui.set_board_done_items(ModelRc::new(VecModel::from(board_done_items)));
    ui.set_board_urgent_important(ModelRc::new(VecModel::from(board_urgent_important)));
    ui.set_board_not_urgent_important(ModelRc::new(VecModel::from(board_not_urgent_important)));
    ui.set_board_urgent_not_important(ModelRc::new(VecModel::from(board_urgent_not_important)));
    ui.set_board_not_urgent_not_important(ModelRc::new(VecModel::from(
        board_not_urgent_not_important,
    )));

    let todo_completed_count = today_todos.iter().filter(|todo| todo.done).count() as i32;
    let today = Local::now().date_naive();
    let weekday =
        ["一", "二", "三", "四", "五", "六", "日"][today.weekday().num_days_from_monday() as usize];
    widget.set_today_date_text(
        format!("{}月{}日 星期{}", today.month(), today.day(), weekday).into(),
    );
    widget.set_today_lunar_text(format!("农历{}", lunar::short_label(today)).into());
    widget.set_todo_completed_count(todo_completed_count);
    widget.set_month_title(month_title);
    widget.set_days(ModelRc::new(VecModel::from(days_model)));
    widget.set_selected_day(selected_day as i32);
    widget.set_today_events(ModelRc::new(VecModel::from(today_events)));
    widget.set_upcoming_events(ModelRc::new(VecModel::from(upcoming_events)));
    widget.set_today_todos(ModelRc::new(VecModel::from(today_todos)));
    widget.set_recent_notes(ModelRc::new(VecModel::from(recent_notes)));
    widget.set_countdowns(ModelRc::new(VecModel::from(countdowns)));
    widget.set_weather_text(weather_summary.into());
    widget.set_today_full_text(today_full_text.into());
    update_tool_status(ui, widget, state);
}
