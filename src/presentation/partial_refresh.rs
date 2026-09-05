//! 高频局部刷新。
//!
//! 新增、勾选和编辑待办/便签/习惯/课程时，只重建对应模型，避免每次都重新
//! 查询 42 天月历、周视图、时间轴、搜索、订阅、天气及所有桌面卡片数据。

use crate::*;

pub(crate) fn refresh_todos(ui: &AppWindow, widget: &WidgetWindow, state: &Rc<RefCell<AppState>>) {
    let (selected, today_items, board_items) = {
        let s = state.borrow();
        let today = Local::now().date_naive();
        let selected_date = NaiveDate::from_ymd_opt(s.year, s.month, s.selected_day);
        let selected = selected_date
            .and_then(|date| db::list_todos(&s.conn, Some(date), None).ok())
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_todo)
            .collect::<Vec<_>>();
        let today_items = db::list_todos(&s.conn, Some(today), None)
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_todo)
            .collect::<Vec<_>>();
        let board_items = db::list_all_todos(&s.conn).unwrap_or_default();
        (selected, today_items, board_items)
    };

    let today = Local::now().date_naive();
    let todo = board_items
        .iter()
        .filter(|item| item.status == "todo")
        .map(|item| to_board_item(item, today))
        .collect::<Vec<_>>();
    let doing = board_items
        .iter()
        .filter(|item| item.status == "doing")
        .map(|item| to_board_item(item, today))
        .collect::<Vec<_>>();
    let done = board_items
        .iter()
        .filter(|item| item.status == "done")
        .map(|item| to_board_item(item, today))
        .collect::<Vec<_>>();
    let active = board_items
        .iter()
        .filter(|item| item.status != "done")
        .collect::<Vec<_>>();
    let quadrant = |important, urgent| {
        active
            .iter()
            .filter(|item| {
                item.important == important && todo_is_urgent(&item.due_date, today) == urgent
            })
            .map(|item| to_board_item(item, today))
            .collect::<Vec<_>>()
    };

    ui.set_todos_for_day(ModelRc::new(VecModel::from(selected)));
    ui.set_today_todo_completed_count(today_items.iter().filter(|item| item.done).count() as i32);
    ui.set_today_todo_count(today_items.len() as i32);
    ui.set_board_todo_items(ModelRc::new(VecModel::from(todo)));
    ui.set_board_doing_items(ModelRc::new(VecModel::from(doing)));
    ui.set_board_done_items(ModelRc::new(VecModel::from(done)));
    ui.set_board_urgent_important(ModelRc::new(VecModel::from(quadrant(true, true))));
    ui.set_board_not_urgent_important(ModelRc::new(VecModel::from(quadrant(true, false))));
    ui.set_board_urgent_not_important(ModelRc::new(VecModel::from(quadrant(false, true))));
    ui.set_board_not_urgent_not_important(ModelRc::new(VecModel::from(quadrant(false, false))));
    widget.set_todo_completed_count(today_items.iter().filter(|item| item.done).count() as i32);
    widget.set_today_todos(ModelRc::new(VecModel::from(today_items)));
    sync_desktop_widgets(widget);
}

pub(crate) fn refresh_notes(ui: &AppWindow, widget: &WidgetWindow, state: &Rc<RefCell<AppState>>) {
    let notes = db::list_notes(&state.borrow().conn)
        .unwrap_or_default()
        .into_iter()
        .map(to_ui_note)
        .collect::<Vec<_>>();
    ui.set_notes(ModelRc::new(VecModel::from(notes.clone())));
    widget.set_recent_notes(ModelRc::new(VecModel::from(notes)));
    sync_desktop_widgets(widget);
}

pub(crate) fn refresh_habits(
    ui: &AppWindow,
    _widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
) {
    let habits = db::list_habits(&state.borrow().conn, false)
        .unwrap_or_default()
        .into_iter()
        .map(to_ui_habit)
        .collect::<Vec<_>>();
    ui.set_habits(ModelRc::new(VecModel::from(habits)));
}

pub(crate) fn refresh_courses(
    ui: &AppWindow,
    _widget: &WidgetWindow,
    state: &Rc<RefCell<AppState>>,
) {
    let s = state.borrow();
    let today = Local::now().date_naive();
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

pub(crate) fn refresh_search(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    let results = if s.view_mode == 7 {
        db::search(&s.conn, &s.search_query, 50)
            .unwrap_or_default()
            .into_iter()
            .map(|hit| SearchItem {
                id: hit.id as i32,
                kind: hit.kind.into(),
                title: hit.title.into(),
                meta: hit.meta.into(),
                date: hit.date.into(),
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    ui.set_search_query(s.search_query.clone().into());
    ui.set_search_results(ModelRc::new(VecModel::from(results)));
}

pub(crate) fn refresh_subscriptions(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let items = db::list_subscriptions(&state.borrow().conn)
        .unwrap_or_default()
        .into_iter()
        .map(|subscription| {
            let (calendar_name, event_count) =
                db::subscription_summary(&state.borrow().conn, subscription.id).unwrap_or_default();
            let last_sync = subscription.last_sync.clone().unwrap_or_default();
            let server_label = url::Url::parse(&subscription.url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
                .unwrap_or_else(|| "服务器".into());
            let has_error = subscription.last_error.is_some();
            let (status, detail) = if let Some(error) = subscription.last_error {
                ("同步失败".to_string(), integrations::safe_error(&error))
            } else if let Some(last_sync) = subscription.last_sync {
                ("已同步".to_string(), format!("上次同步 {last_sync}"))
            } else {
                ("待同步".to_string(), "已保存，等待首次同步".to_string())
            };
            SubscriptionItem {
                id: subscription.id as i32,
                name: subscription.name.into(),
                kind: subscription.source_type.into(),
                status: status.into(),
                detail: detail.into(),
                has_error,
                calendar_name: calendar_name.into(),
                event_count,
                last_sync: last_sync.into(),
                server_label: server_label.into(),
            }
        })
        .collect::<Vec<_>>();
    let failures = items.iter().filter(|item| item.has_error).count();
    let pending = items
        .iter()
        .filter(|item| item.last_sync.is_empty() && !item.has_error)
        .count();
    ui.set_status_has_error(failures > 0);
    ui.set_status_left_text(
        if items.is_empty() {
            "本地模式".to_string()
        } else if failures > 0 {
            format!("{} 个连接同步失败 · 工具中可查看详情", failures)
        } else if pending > 0 {
            format!("{} 个连接待同步", pending)
        } else {
            format!("{} 个连接最近同步成功", items.len())
        }
        .into(),
    );
    ui.set_subscriptions(ModelRc::new(VecModel::from(items)));
}

pub(crate) fn refresh_records(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    let records = if s.view_mode == 8 {
        let today = Local::now().date_naive();
        db::list_special_events(&s.conn)
            .unwrap_or_default()
            .into_iter()
            .map(|event| to_ui_record(event, today))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    ui.set_records(ModelRc::new(VecModel::from(records)));
}

pub(crate) fn refresh_shifts(ui: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    let first_of_month = NaiveDate::from_ymd_opt(s.year, s.month, 1);
    let types = if s.view_mode == 9 {
        db::list_shift_types(&s.conn)
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_shift_type)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let assignments = if s.view_mode == 9 {
        first_of_month
            .and_then(|start| {
                db::list_shift_assignments(&s.conn, start, month_end(s.year, s.month)?).ok()
            })
            .unwrap_or_default()
            .into_iter()
            .map(to_ui_shift_assignment)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    ui.set_shift_types(ModelRc::new(VecModel::from(types)));
    ui.set_shift_assignments(ModelRc::new(VecModel::from(assignments)));
    ui.set_shift_start_date(s.shift_start_date.clone().into());
    ui.set_shift_end_date(s.shift_end_date.clone().into());
    ui.set_shift_sequence(s.shift_sequence.clone().into());
    ui.set_shift_result(s.shift_result.clone().into());
}
