use super::*;

// ------------------------------ Local backup（本地 JSON 备份） ------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupHabit {
    pub id: i64,
    pub title: String,
    pub archived: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupHabitLog {
    pub habit_id: i64,
    pub log_date: String,
}

/// 随本地备份一起导出的课程字段说明。其他软件可先读取 `schema_id`，再按
/// `fields` 与取值范围解析 `courses`，不需要依赖 TimeHub 的内部实现。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseJsonField {
    pub name: String,
    pub json_type: String,
    pub required: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseJsonSchema {
    pub schema_id: String,
    pub version: u32,
    pub encoding: String,
    pub description: String,
    pub weekday_values: Vec<String>,
    pub period_range: [i64; 2],
    pub week_range: [i64; 2],
    pub color_palette: Vec<String>,
    pub fields: Vec<CourseJsonField>,
}

impl Default for CourseJsonSchema {
    fn default() -> Self {
        let field =
            |name: &str, json_type: &str, required: bool, description: &str| CourseJsonField {
                name: name.to_string(),
                json_type: json_type.to_string(),
                required,
                description: description.to_string(),
            };
        Self {
            schema_id: "timehub.course/v1".to_string(),
            version: 1,
            encoding: "UTF-8".to_string(),
            description:
                "TimeHub 周课程表。星期采用 ISO 顺序，节次和周次均从 1 开始；区间端点均包含。"
                    .to_string(),
            weekday_values: vec![
                "1=Monday/周一".to_string(),
                "2=Tuesday/周二".to_string(),
                "3=Wednesday/周三".to_string(),
                "4=Thursday/周四".to_string(),
                "5=Friday/周五".to_string(),
                "6=Saturday/周六".to_string(),
                "7=Sunday/周日".to_string(),
            ],
            period_range: [1, 12],
            week_range: [1, 30],
            color_palette: vec![
                "#2e6be6".to_string(),
                "#0e9f6e".to_string(),
                "#c77700".to_string(),
                "#d93a49".to_string(),
                "#7c4dff".to_string(),
                "#0891b2".to_string(),
                "#db2777".to_string(),
                "#65a30d".to_string(),
            ],
            fields: vec![
                field(
                    "id",
                    "integer",
                    false,
                    "TimeHub 本地主键；跨软件导入时可忽略或重新生成。",
                ),
                field("title", "string", true, "课程名称。"),
                field("teacher", "string", false, "教师姓名；未知时为空字符串。"),
                field(
                    "location",
                    "string",
                    false,
                    "教室或上课地点；未知时为空字符串。",
                ),
                field("weekday", "integer", true, "星期：1=周一，…，7=周日。"),
                field("start_period", "integer", true, "开始节次，范围 1–12。"),
                field(
                    "period_count",
                    "integer",
                    true,
                    "连续占用节数，范围 1–6。结束节次=start_period+period_count-1。",
                ),
                field("start_week", "integer", true, "生效起始周，包含该周。"),
                field("end_week", "integer", true, "生效结束周，包含该周。"),
                field(
                    "color_index",
                    "integer",
                    false,
                    "颜色索引 0–7，对应 color_palette；缺省可用 0。",
                ),
                field(
                    "created_at",
                    "string",
                    false,
                    "本地创建时间，格式 YYYY-MM-DD HH:MM:SS。",
                ),
                field(
                    "updated_at",
                    "string",
                    false,
                    "本地更新时间，格式 YYYY-MM-DD HH:MM:SS。",
                ),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupEventException {
    pub event_id: i64,
    pub occurrence_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalBackup {
    pub format_version: u32,
    pub exported_at: String,
    pub calendars: Vec<Calendar>,
    pub events: Vec<Event>,
    #[serde(default)]
    pub event_exceptions: Vec<BackupEventException>,
    pub todos: Vec<Todo>,
    pub notes: Vec<Note>,
    #[serde(default)]
    pub course_schema: CourseJsonSchema,
    #[serde(default)]
    pub courses: Vec<Course>,
    pub habits: Vec<BackupHabit>,
    pub habit_logs: Vec<BackupHabitLog>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportStats {
    pub calendars: usize,
    pub events: usize,
    pub todos: usize,
    pub notes: usize,
    pub courses: usize,
    pub habits: usize,
    pub habit_logs: usize,
}

pub fn export_backup(conn: &Connection) -> Result<LocalBackup> {
    let habits = list_habits(conn, true)?
        .into_iter()
        .map(|habit| BackupHabit {
            id: habit.id,
            title: habit.title,
            archived: habit.archived,
            created_at: habit.created_at,
        })
        .collect();
    let mut stmt =
        conn.prepare("SELECT habit_id, log_date FROM habit_logs ORDER BY log_date, habit_id")?;
    let habit_logs = stmt
        .query_map([], |row| {
            Ok(BackupHabitLog {
                habit_id: row.get(0)?,
                log_date: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(LocalBackup {
        format_version: 1,
        exported_at: now(),
        calendars: list_calendars(conn)?,
        events: list_all_events(conn)?,
        event_exceptions: conn.prepare("SELECT x.event_id, x.occurrence_date FROM event_exceptions x JOIN events e ON e.id = x.event_id ORDER BY x.event_id, x.occurrence_date")?
            .query_map([], |row| Ok(BackupEventException { event_id: row.get(0)?, occurrence_date: row.get(1)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        todos: list_all_todos(conn)?,
        notes: list_notes(conn)?,
        course_schema: CourseJsonSchema::default(),
        courses: list_courses(conn)?,
        habits,
        habit_logs,
    })
}

/// 导入采用“追加”策略，不覆盖现有数据；主键重新生成，分类日历按名称复用。
pub fn import_backup(conn: &mut Connection, backup: &LocalBackup) -> Result<ImportStats> {
    anyhow::ensure!(
        backup.format_version == 1,
        "不支持的备份版本：{}",
        backup.format_version
    );
    let tx = conn.transaction()?;
    let mut calendar_ids = std::collections::HashMap::new();
    let mut calendar_count = 0;
    for calendar in &backup.calendars {
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM calendars WHERE name = ?1 ORDER BY id LIMIT 1",
                params![calendar.name],
                |row| row.get(0),
            )
            .optional()?;
        let id = if let Some(id) = existing {
            id
        } else {
            tx.execute(
                "INSERT INTO calendars (name, color, visible, sort_order, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![calendar.name, calendar.color, calendar.visible as i64, calendar.sort_order, calendar.created_at],
            )?;
            calendar_count += 1;
            tx.last_insert_rowid()
        };
        calendar_ids.insert(calendar.id, id);
    }
    let mut event_count = 0;
    let mut event_ids = std::collections::HashMap::new();
    for event in &backup.events {
        tx.execute(
            "INSERT INTO events (title, date, time, duration_minutes, note, repeat_rule, reminder_offsets, category, calendar_id, created_at, updated_at, source_kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![event.title, event.date, event.time, event.duration_minutes, event.note, event.repeat_rule, event.reminder_offsets, event.category, calendar_ids.get(&event.calendar_id).copied().unwrap_or(1), event.created_at, event.updated_at, event.source_kind],
        )?;
        event_count += 1;
        event_ids.insert(event.id, tx.last_insert_rowid());
    }
    for exception in &backup.event_exceptions {
        let event_id = event_ids
            .get(&exception.event_id)
            .context("备份日程例外引用了不存在的日程")?;
        let value = exception
            .occurrence_date
            .strip_prefix("from:")
            .unwrap_or(&exception.occurrence_date);
        NaiveDate::parse_from_str(value, "%Y-%m-%d").context("备份日程例外日期无效")?;
        tx.execute(
            "INSERT OR IGNORE INTO event_exceptions(event_id, occurrence_date) VALUES (?1, ?2)",
            params![event_id, exception.occurrence_date],
        )?;
    }
    let mut todo_count = 0;
    for todo in &backup.todos {
        let status = if matches!(todo.status.as_str(), "todo" | "doing" | "done") {
            todo.status.as_str()
        } else {
            "todo"
        };
        let done = status == "done" || todo.done;
        tx.execute(
            "INSERT INTO todos (title, done, due_date, priority, important, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![todo.title, done as i64, todo.due_date, todo.priority, todo.important as i64, if done { "done" } else { status }, todo.created_at, todo.updated_at],
        )?;
        todo_count += 1;
    }
    let mut note_count = 0;
    for note in &backup.notes {
        tx.execute(
            "INSERT INTO notes (title, content, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![note.title, note.content, note.created_at, note.updated_at],
        )?;
        note_count += 1;
    }
    let mut course_count = 0;
    for course in &backup.courses {
        tx.execute(
            "INSERT INTO courses (title, teacher, location, weekday, start_period, period_count, start_week, end_week, color_index, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![course.title, course.teacher, course.location, course.weekday, course.start_period, course.period_count, course.start_week, course.end_week, course.color_index, course.created_at, course.updated_at],
        )?;
        course_count += 1;
    }
    let mut habit_ids = std::collections::HashMap::new();
    let mut habit_count = 0;
    for habit in &backup.habits {
        tx.execute(
            "INSERT INTO habits (title, archived, created_at) VALUES (?1, ?2, ?3)",
            params![habit.title, habit.archived as i64, habit.created_at],
        )?;
        habit_ids.insert(habit.id, tx.last_insert_rowid());
        habit_count += 1;
    }
    let mut habit_log_count = 0;
    for log in &backup.habit_logs {
        if let Some(new_habit_id) = habit_ids.get(&log.habit_id) {
            tx.execute(
                "INSERT OR IGNORE INTO habit_logs (habit_id, log_date) VALUES (?1, ?2)",
                params![new_habit_id, log.log_date],
            )?;
            habit_log_count += 1;
        }
    }
    tx.commit()?;
    Ok(ImportStats {
        calendars: calendar_count,
        events: event_count,
        todos: todo_count,
        notes: note_count,
        courses: course_count,
        habits: habit_count,
        habit_logs: habit_log_count,
    })
}
