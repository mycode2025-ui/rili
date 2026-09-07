//! 本地 SQLite 存储：日程 / 待办 / 便签的建表与增删改查。
//! 数据结构刻意保持简单、字段自解释，方便后续扩展（标签/清单/重复规则等）。

use anyhow::{Context, Result};
use chrono::{Local, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

mod backup;
mod course;

pub use backup::*;
pub use course::*;

pub fn open() -> Result<Connection> {
    let conn = Connection::open(crate::app_paths::db_path()?).context("打开数据库失败")?;
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS events (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            title             TEXT NOT NULL,
            date              TEXT NOT NULL,      -- YYYY-MM-DD，重复日程的“基准日期”
            time              TEXT,               -- HH:MM，NULL 表示全天
            duration_minutes  INTEGER NOT NULL DEFAULT 60,  -- 日/三日视图拖拽调整时长用，全天日程忽略此字段
            note              TEXT NOT NULL DEFAULT '',
            repeat_rule       TEXT NOT NULL DEFAULT 'none',   -- none/daily/weekly/weekly:1,3/monthly/monthly-nth:3:4/yearly
            reminder_offsets  TEXT NOT NULL DEFAULT '',       -- 逗号分隔的“提前 N 分钟”，如 "0,30,1440"
            category          TEXT NOT NULL DEFAULT 'event',  -- event/birthday/anniversary/countdown
            source_kind       TEXT NOT NULL DEFAULT 'local',  -- local/shared/imported/subscription
            created_at        TEXT NOT NULL,
            updated_at        TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_events_date ON events(date);

        CREATE TABLE IF NOT EXISTS todos (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            title       TEXT NOT NULL,
            done        INTEGER NOT NULL DEFAULT 0,
            due_date    TEXT,               -- YYYY-MM-DD，可为空
            priority    INTEGER NOT NULL DEFAULT 0,
            important   INTEGER NOT NULL DEFAULT 0,   -- 四象限视图的"重要"维度；"紧急"维度由 due_date 是否临近自动推算
            status      TEXT NOT NULL DEFAULT 'todo',  -- todo/doing/done，供看板视图使用
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_todos_due_date ON todos(due_date);

        CREATE TABLE IF NOT EXISTS notes (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            title       TEXT NOT NULL,
            content     TEXT NOT NULL DEFAULT '',
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        );

        -- 提醒发送记录，用于去重：同一条日程的同一次发生日期 + 同一级提醒只发一次。
        CREATE TABLE IF NOT EXISTS reminder_log (
            event_id        INTEGER NOT NULL,
            occurrence_date TEXT NOT NULL,
            offset_minutes  INTEGER NOT NULL,
            notified_at     TEXT NOT NULL,
            PRIMARY KEY (event_id, occurrence_date, offset_minutes)
        );

        -- 习惯打卡：每个习惯每天最多一条打卡记录，用于统计连续打卡天数。
        CREATE TABLE IF NOT EXISTS habits (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            title       TEXT NOT NULL,
            archived    INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS habit_logs (
            habit_id  INTEGER NOT NULL,
            log_date  TEXT NOT NULL,   -- YYYY-MM-DD
            PRIMARY KEY (habit_id, log_date)
        );

        -- 应用设置：主题 / 一周起始日 / 是否显示周数等，键值对形式方便后续扩展。
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        -- 分类日历（类似"工作/个人/家庭"这种带颜色的日程分组），用于日历格子里的彩色标签
        -- 和侧边栏的显示/隐藏筛选。
        CREATE TABLE IF NOT EXISTS calendars (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL,
            color       TEXT NOT NULL,             -- #RRGGBB
            visible     INTEGER NOT NULL DEFAULT 1,
            sort_order  INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL
        );

        -- ICS URL 订阅源；外部日程通过 event_sources 映射到本地事件，按 UID 同步更新。
        CREATE TABLE IF NOT EXISTS subscriptions (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            name          TEXT NOT NULL,
            url           TEXT NOT NULL UNIQUE,
            calendar_id   INTEGER NOT NULL DEFAULT 1,
            enabled       INTEGER NOT NULL DEFAULT 1,
            last_sync     TEXT,
            last_error    TEXT,
            source_type   TEXT NOT NULL DEFAULT 'ics',
            username      TEXT NOT NULL DEFAULT '',
            secret        TEXT NOT NULL DEFAULT '',
            created_at    TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS event_sources (
            event_id         INTEGER PRIMARY KEY,
            subscription_id  INTEGER NOT NULL,
            external_uid     TEXT NOT NULL,
            UNIQUE(subscription_id, external_uid)
        );
        CREATE INDEX IF NOT EXISTS idx_event_sources_subscription ON event_sources(subscription_id);

        -- 普通日期表示跳过单次发生；from:YYYY-MM-DD 表示从该次起截断重复序列。
        CREATE TABLE IF NOT EXISTS event_exceptions (
            event_id        INTEGER NOT NULL,
            occurrence_date TEXT NOT NULL,
            PRIMARY KEY (event_id, occurrence_date)
        );

        -- 本地排班：班次定义与日期分配分开保存，生成周期时同一天采用 upsert，避免重复生成。
        CREATE TABLE IF NOT EXISTS shift_types (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            name              TEXT NOT NULL,
            start_time        TEXT,
            end_time          TEXT,
            color             TEXT NOT NULL DEFAULT '#2e6be6',
            is_rest            INTEGER NOT NULL DEFAULT 0,
            duration_minutes  INTEGER NOT NULL DEFAULT 0,
            created_at        TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS shift_assignments (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            shift_type_id INTEGER NOT NULL,
            shift_date    TEXT NOT NULL UNIQUE,
            created_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_shift_assignments_date ON shift_assignments(shift_date);

        -- 课程按星期、节次和生效周次保存，与普通日程分开，便于周次过滤和冲突检查。
        CREATE TABLE IF NOT EXISTS courses (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            title         TEXT NOT NULL,
            teacher       TEXT NOT NULL DEFAULT '',
            location      TEXT NOT NULL DEFAULT '',
            weekday       INTEGER NOT NULL,
            start_period  INTEGER NOT NULL,
            period_count  INTEGER NOT NULL DEFAULT 2,
            start_week    INTEGER NOT NULL DEFAULT 1,
            end_week      INTEGER NOT NULL DEFAULT 18,
            color_index   INTEGER NOT NULL DEFAULT 0,
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_courses_slot ON courses(weekday, start_period, start_week, end_week);
        "#,
    )
    .context("初始化数据库表结构失败")?;
    migrate_add_columns(&conn)?;
    migrate_subscription_columns(&conn)?;
    seed_default_calendars(&conn)?;
    migrate_legacy_calendar_colors(&conn)?;
    seed_default_shift_types(&conn)?;
    Ok(conn)
}

fn migrate_subscription_columns(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(subscriptions)")?;
    let existing: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (column, definition) in [
        ("source_type", "TEXT NOT NULL DEFAULT 'ics'"),
        ("username", "TEXT NOT NULL DEFAULT ''"),
        ("secret", "TEXT NOT NULL DEFAULT ''"),
    ] {
        if !existing.iter().any(|item| item == column) {
            conn.execute(
                &format!("ALTER TABLE subscriptions ADD COLUMN {column} {definition}"),
                [],
            )?;
        }
    }
    Ok(())
}

/// 首次运行时数据库里还没有任何日历分类，插入设计稿定义的默认分类颜色，
/// 让"分类日历"这个功能开箱即用，不需要用户先手动建一个才能开始用。
fn seed_default_calendars(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM calendars", [], |row| row.get(0))?;
    if count > 0 {
        return Ok(());
    }
    let ts = now();
    for (name, color) in [
        ("默认", "#2e6be6"),
        ("工作", "#2e6be6"),
        ("个人", "#0e9f6e"),
        ("家庭", "#c77700"),
    ] {
        conn.execute(
            "INSERT INTO calendars (name, color, visible, sort_order, created_at) VALUES (?1, ?2, 1, 0, ?3)",
            params![name, color, ts],
        )?;
    }
    Ok(())
}

/// 只迁移旧版本自动创建且仍保持旧默认色的分类；用户改过的自定义颜色不动。
fn migrate_legacy_calendar_colors(conn: &Connection) -> Result<()> {
    for (name, old_color, new_color) in [
        ("工作", "#8a5cf5", "#2e6be6"),
        ("个人", "#e0568f", "#0e9f6e"),
        ("家庭", "#f0a63f", "#c77700"),
    ] {
        conn.execute(
            "UPDATE calendars SET color = ?3 WHERE name = ?1 AND lower(color) = ?2",
            params![name, old_color, new_color],
        )?;
    }
    Ok(())
}

fn seed_default_shift_types(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM shift_types", [], |row| row.get(0))?;
    if count > 0 {
        return Ok(());
    }
    let ts = now();
    for (name, start, end, color, is_rest, duration) in [
        ("白班", Some("08:00"), Some("20:00"), "#2e6be6", 0, 720),
        ("夜班", Some("20:00"), Some("08:00"), "#8a5cf5", 0, 720),
        ("休息", None, None, "#6b7280", 1, 0),
        ("培训", Some("09:00"), Some("17:00"), "#2fa86a", 0, 480),
    ] {
        conn.execute(
            "INSERT INTO shift_types (name, start_time, end_time, color, is_rest, duration_minutes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![name, start, end, color, is_rest, duration, ts],
        )?;
    }
    Ok(())
}

/// 早期版本创建的 `events` 表没有 `repeat_rule` / `reminder_offsets` / `category` 列，
/// 这里做一次幂等的“加列”迁移，保证老数据库升级后也能正常工作。
fn migrate_add_columns(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(events)")?;
    let existing: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !existing.iter().any(|c| c == "repeat_rule") {
        conn.execute(
            "ALTER TABLE events ADD COLUMN repeat_rule TEXT NOT NULL DEFAULT 'none'",
            [],
        )?;
    }
    if !existing.iter().any(|c| c == "reminder_offsets") {
        conn.execute(
            "ALTER TABLE events ADD COLUMN reminder_offsets TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    if !existing.iter().any(|c| c == "category") {
        conn.execute(
            "ALTER TABLE events ADD COLUMN category TEXT NOT NULL DEFAULT 'event'",
            [],
        )?;
    }
    if !existing.iter().any(|c| c == "calendar_id") {
        conn.execute(
            "ALTER TABLE events ADD COLUMN calendar_id INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }
    if !existing.iter().any(|c| c == "duration_minutes") {
        conn.execute(
            "ALTER TABLE events ADD COLUMN duration_minutes INTEGER NOT NULL DEFAULT 60",
            [],
        )?;
    }
    if !existing.iter().any(|c| c == "source_kind") {
        conn.execute(
            "ALTER TABLE events ADD COLUMN source_kind TEXT NOT NULL DEFAULT 'local'",
            [],
        )?;
    }
    conn.execute(
        "UPDATE events SET source_kind = 'subscription' WHERE id IN (SELECT event_id FROM event_sources)",
        [],
    )?;

    let mut stmt = conn.prepare("PRAGMA table_info(todos)")?;
    let existing: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !existing.iter().any(|c| c == "important") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN important INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !existing.iter().any(|c| c == "status") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN status TEXT NOT NULL DEFAULT 'todo'",
            [],
        )?;
        // 老数据：已完成的待办直接映射到看板"已完成"列，保持视图间数据一致。
        conn.execute("UPDATE todos SET status = 'done' WHERE done = 1", [])?;
    }
    Ok(())
}

/// 读取一项设置，不存在时返回传入的默认值。
pub fn get_setting(conn: &Connection, key: &str, default: &str) -> Result<String> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;
    Ok(rows
        .next()
        .transpose()?
        .unwrap_or_else(|| default.to_string()))
}

/// 写入一项设置（不存在则插入，存在则覆盖）。
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// 原子写入多项互相关联的设置，避免只保存一半导致下次启动状态不一致。
pub fn set_settings(conn: &mut Connection, values: &[(&str, &str)]) -> Result<()> {
    let tx = conn.transaction()?;
    for (key, value) in values {
        tx.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub const DEFAULT_EVENT_REMINDER_KEY: &str = "default_event_reminder";
pub const DEFAULT_EVENT_REMINDER: &str = "10";

/// 返回统一的日程默认提醒。仅接受界面提供的五种模式；数据库里出现旧值或损坏值时
/// 回退到提前 10 分钟，避免把不可识别的数据继续写入新日程。
pub fn default_event_reminder(conn: &Connection) -> Result<String> {
    let value = get_setting(conn, DEFAULT_EVENT_REMINDER_KEY, DEFAULT_EVENT_REMINDER)?;
    Ok(
        if matches!(value.as_str(), "" | "0" | "10" | "60" | "1440") {
            value
        } else {
            DEFAULT_EVENT_REMINDER.to_string()
        },
    )
}

/// 保存统一默认提醒，并立即覆盖所有现有日程。这样导入日历和历史日程无需逐条打开。
pub fn apply_default_event_reminder(conn: &Connection, value: &str) -> Result<usize> {
    anyhow::ensure!(
        matches!(value, "" | "0" | "10" | "60" | "1440"),
        "不支持的默认提醒模式"
    );
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![DEFAULT_EVENT_REMINDER_KEY, value],
    )?;
    let changed = tx.execute(
        "UPDATE events SET reminder_offsets = ?1, updated_at = ?2 WHERE reminder_offsets <> ?1",
        params![value, now()],
    )?;
    tx.commit()?;
    Ok(changed)
}

fn now() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

// ------------------------------ Calendar（分类日历：工作/个人/家庭…）------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calendar {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub visible: bool,
    pub sort_order: i64,
    pub created_at: String,
}

fn row_to_calendar(row: &rusqlite::Row) -> rusqlite::Result<Calendar> {
    Ok(Calendar {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row.get(2)?,
        visible: row.get::<_, i64>(3)? != 0,
        sort_order: row.get(4)?,
        created_at: row.get(5)?,
    })
}

const CALENDAR_COLUMNS: &str = "id, name, color, visible, sort_order, created_at";

pub fn list_calendars(conn: &Connection) -> Result<Vec<Calendar>> {
    let sql = format!("SELECT {CALENDAR_COLUMNS} FROM calendars ORDER BY sort_order, id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], row_to_calendar)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 仅返回当前"可见"的日历 id 集合，用于筛选日程展示（隐藏日历里的日程不出现在任何视图）。
pub fn visible_calendar_ids(conn: &Connection) -> Result<std::collections::HashSet<i64>> {
    Ok(list_calendars(conn)?
        .into_iter()
        .filter(|c| c.visible)
        .map(|c| c.id)
        .collect())
}

pub fn create_calendar(conn: &Connection, name: &str, color: &str) -> Result<Calendar> {
    let ts = now();
    conn.execute(
        "INSERT INTO calendars (name, color, visible, sort_order, created_at) VALUES (?1, ?2, 1, 0, ?3)",
        params![name, color, ts],
    )?;
    let id = conn.last_insert_rowid();
    let sql = format!("SELECT {CALENDAR_COLUMNS} FROM calendars WHERE id = ?1");
    Ok(conn.query_row(&sql, params![id], row_to_calendar)?)
}

pub fn set_calendar_visible(conn: &Connection, id: i64, visible: bool) -> Result<()> {
    conn.execute(
        "UPDATE calendars SET visible = ?1 WHERE id = ?2",
        params![visible as i64, id],
    )?;
    Ok(())
}

pub fn delete_calendar(conn: &Connection, id: i64) -> Result<usize> {
    // 该分类下的日程改挂到"默认"日历（id=1），而不是级联删除，避免误删用户日程数据。
    conn.execute(
        "UPDATE events SET calendar_id = 1 WHERE calendar_id = ?1",
        params![id],
    )?;
    Ok(conn.execute("DELETE FROM calendars WHERE id = ?1", params![id])?)
}

// ------------------------------ Subscription（ICS URL 订阅） ------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub calendar_id: i64,
    pub enabled: bool,
    pub last_sync: Option<String>,
    pub last_error: Option<String>,
    pub source_type: String,
    pub username: String,
    #[serde(skip_serializing, default)]
    pub secret: String,
    pub created_at: String,
}

fn row_to_subscription(row: &rusqlite::Row) -> rusqlite::Result<Subscription> {
    Ok(Subscription {
        id: row.get(0)?,
        name: row.get(1)?,
        url: row.get(2)?,
        calendar_id: row.get(3)?,
        enabled: row.get::<_, i64>(4)? != 0,
        last_sync: row.get(5)?,
        last_error: row.get(6)?,
        source_type: row.get(7)?,
        username: row.get(8)?,
        secret: row.get(9)?,
        created_at: row.get(10)?,
    })
}

pub fn create_subscription(
    conn: &Connection,
    name: &str,
    url: &str,
    calendar_id: i64,
) -> Result<Subscription> {
    let url = url.trim();
    anyhow::ensure!(
        url.starts_with("https://") || url.starts_with("http://"),
        "订阅地址必须是 http:// 或 https:// URL"
    );
    anyhow::ensure!(!name.trim().is_empty(), "订阅名称不能为空");
    if let Some(existing) = get_subscription_by_url(conn, url)? {
        return Ok(existing);
    }
    conn.execute(
        "INSERT INTO subscriptions (name, url, calendar_id, enabled, created_at) VALUES (?1, ?2, ?3, 1, ?4)",
        params![name.trim(), url, calendar_id, now()],
    )?;
    let id = conn.last_insert_rowid();
    get_subscription(conn, id)?.context("刚插入的订阅读取失败")
}

pub fn create_or_update_caldav_subscription(
    conn: &Connection,
    name: &str,
    url: &str,
    username: &str,
    password: &str,
    calendar_id: i64,
) -> Result<Subscription> {
    let url = normalize_caldav_server_url(url)?;
    anyhow::ensure!(!username.trim().is_empty(), "CalDAV 用户名不能为空");
    anyhow::ensure!(!password.is_empty(), "CalDAV 专用密码不能为空");
    let secret = crate::secret_store::protect(password)?;
    conn.execute(
        "INSERT INTO subscriptions (name, url, calendar_id, enabled, last_sync, last_error, source_type, username, secret, created_at)
         VALUES (?1, ?2, ?3, 1, NULL, NULL, 'caldav', ?4, ?5, ?6)
         ON CONFLICT(url) DO UPDATE SET
            name = excluded.name, calendar_id = excluded.calendar_id, enabled = 1,
            last_error = NULL, source_type = 'caldav', username = excluded.username,
            secret = excluded.secret",
        params![name.trim(), &url, calendar_id, username.trim(), secret, now()],
    )?;
    get_subscription_by_url(conn, &url)?.context("保存后的 CalDAV 账号读取失败")
}

fn normalize_caldav_server_url(value: &str) -> Result<String> {
    let value = value.trim();
    anyhow::ensure!(!value.is_empty(), "CalDAV 服务器地址不能为空");

    let candidate = if value.contains("://") {
        value.to_owned()
    } else {
        format!("https://{value}")
    };
    let mut parsed = url::Url::parse(&candidate).context("CalDAV 服务器地址格式无效")?;
    anyhow::ensure!(
        parsed.scheme() == "https",
        "CalDAV 服务器必须使用 HTTPS 安全连接"
    );
    anyhow::ensure!(parsed.host_str().is_some(), "CalDAV 服务器地址缺少域名");

    // 钉钉展示给用户的是服务器域名，但 CalDAV 服务实际挂载在 /dav。
    // 其他服务仍保留用户输入的路径，避免对通用 CalDAV 地址作错误猜测。
    if parsed.host_str() == Some("calendar.dingtalk.com") && parsed.path() == "/" {
        parsed.set_path("/dav");
    }

    Ok(parsed.to_string().trim_end_matches('/').to_owned())
}

pub fn get_subscription(conn: &Connection, id: i64) -> Result<Option<Subscription>> {
    let mut stmt = conn.prepare("SELECT id, name, url, calendar_id, enabled, last_sync, last_error, source_type, username, secret, created_at FROM subscriptions WHERE id = ?1")?;
    Ok(stmt
        .query_row(params![id], row_to_subscription)
        .optional()?)
}

/// Editing metadata/credentials never changes the identity or ownership of mirrors.
pub fn edit_subscription(conn: &Connection, id: i64, name: &str, password: &str) -> Result<()> {
    anyhow::ensure!(!name.trim().is_empty(), "连接名称不能为空");
    let subscription = get_subscription(conn, id)?.context("同步连接已不存在")?;
    let secret = if password.is_empty() {
        subscription.secret
    } else {
        anyhow::ensure!(
            subscription.source_type == "caldav",
            "ICS 连接不使用专用密码"
        );
        crate::secret_store::protect(password)?
    };
    conn.execute(
        "UPDATE subscriptions SET name = ?1, secret = ?2 WHERE id = ?3",
        params![name.trim(), secret, id],
    )?;
    Ok(())
}

pub fn subscription_summary(conn: &Connection, id: i64) -> Result<(String, i32)> {
    conn.query_row("SELECT COALESCE(c.name, '未分类'), (SELECT COUNT(*) FROM event_sources es WHERE es.subscription_id = s.id) FROM subscriptions s LEFT JOIN calendars c ON c.id = s.calendar_id WHERE s.id = ?1", params![id], |r| Ok((r.get(0)?, r.get(1)?))).context("读取同步来源失败")
}

pub fn get_subscription_by_url(conn: &Connection, url: &str) -> Result<Option<Subscription>> {
    let mut stmt = conn.prepare("SELECT id, name, url, calendar_id, enabled, last_sync, last_error, source_type, username, secret, created_at FROM subscriptions WHERE url = ?1")?;
    Ok(stmt
        .query_row(params![url.trim()], row_to_subscription)
        .optional()?)
}

pub fn list_subscriptions(conn: &Connection) -> Result<Vec<Subscription>> {
    let mut stmt = conn.prepare("SELECT id, name, url, calendar_id, enabled, last_sync, last_error, source_type, username, secret, created_at FROM subscriptions ORDER BY id DESC")?;
    let rows = stmt
        .query_map([], row_to_subscription)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn set_subscription_result(
    conn: &Connection,
    id: i64,
    last_sync: Option<&str>,
    last_error: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE subscriptions SET last_sync = COALESCE(?1, last_sync), last_error = ?2 WHERE id = ?3",
        params![last_sync, last_error, id],
    )?;
    Ok(())
}

pub fn upsert_subscribed_event(conn: &Connection, subscribed: SubscribedEvent<'_>) -> Result<i64> {
    anyhow::ensure!(
        !subscribed.external_uid.trim().is_empty(),
        "订阅事件缺少 UID"
    );
    let existing: Option<i64> = conn
        .query_row(
            "SELECT event_id FROM event_sources WHERE subscription_id = ?1 AND external_uid = ?2",
            params![subscribed.subscription_id, subscribed.external_uid],
            |row| row.get(0),
        )
        .optional()?;
    let event_id = if let Some(event_id) = existing {
        update_event(
            conn,
            event_id,
            EventUpdate {
                title: Some(subscribed.title),
                date: Some(subscribed.date),
                time: Some(subscribed.time),
                duration_minutes: Some(subscribed.duration_minutes.max(1)),
                note: Some(subscribed.note),
                repeat_rule: Some(subscribed.repeat_rule),
                category: Some("event"),
                calendar_id: Some(subscribed.calendar_id),
                ..EventUpdate::default()
            },
        )?;
        set_event_source_kind(conn, event_id, "subscription")?;
        event_id
    } else {
        let reminder = default_event_reminder(conn)?;
        let event = create_event(
            conn,
            NewEvent {
                title: subscribed.title,
                date: subscribed.date,
                time: subscribed.time,
                note: subscribed.note,
                repeat_rule: subscribed.repeat_rule,
                reminder_offsets: &reminder,
                category: "event",
                calendar_id: subscribed.calendar_id,
            },
        )?;
        update_event(
            conn,
            event.id,
            EventUpdate {
                duration_minutes: Some(subscribed.duration_minutes.max(1)),
                ..EventUpdate::default()
            },
        )?;
        set_event_source_kind(conn, event.id, "subscription")?;
        conn.execute(
            "INSERT INTO event_sources (event_id, subscription_id, external_uid) VALUES (?1, ?2, ?3)",
            params![
                event.id,
                subscribed.subscription_id,
                subscribed.external_uid
            ],
        )?;
        event.id
    };
    Ok(event_id)
}

/// 设置外部周期事件的结束边界。`until` 是远端 RRULE 中包含的最后发生日；
/// 数据库沿用 `from:` 截断标记，因此存储下一天作为首个不再显示的日期。
pub fn set_subscribed_event_repeat_until(
    conn: &Connection,
    event_id: i64,
    until: Option<NaiveDate>,
) -> Result<()> {
    conn.execute(
        "DELETE FROM event_exceptions WHERE event_id = ?1 AND occurrence_date LIKE 'from:%'",
        params![event_id],
    )?;
    if let Some(cutoff) = until.and_then(|date| date.succ_opt()) {
        conn.execute(
            "INSERT OR REPLACE INTO event_exceptions (event_id, occurrence_date) VALUES (?1, ?2)",
            params![event_id, format!("from:{cutoff}")],
        )?;
    }
    Ok(())
}

/// 删除订阅源明确标记为已取消的事件。删除映射与本地镜像记录必须在同一事务中完成，
/// 避免日历里留下一个仍可点击、但来源状态已经失效的孤立条目。
pub fn delete_subscribed_event(
    conn: &Connection,
    subscription_id: i64,
    external_uid: &str,
) -> Result<bool> {
    let event_id: Option<i64> = conn
        .query_row(
            "SELECT event_id FROM event_sources WHERE subscription_id = ?1 AND external_uid = ?2",
            params![subscription_id, external_uid],
            |row| row.get(0),
        )
        .optional()?;
    let Some(event_id) = event_id else {
        return Ok(false);
    };

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM event_sources WHERE subscription_id = ?1 AND external_uid = ?2",
        params![subscription_id, external_uid],
    )?;
    tx.execute("DELETE FROM events WHERE id = ?1", params![event_id])?;
    tx.execute(
        "DELETE FROM event_exceptions WHERE event_id = ?1",
        params![event_id],
    )?;
    tx.execute(
        "DELETE FROM reminder_log WHERE event_id = ?1",
        params![event_id],
    )?;
    tx.commit()?;
    Ok(true)
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteSubscriptionResult {
    pub deleted_connection: bool,
    pub deleted_events: usize,
    pub deleted_calendar: bool,
    pub calendar_name: String,
}

/// 删除同步连接及其本地镜像日程。
///
/// 默认分类和仍被其他数据使用的分类会保留；只有已经完全空闲的非默认分类
/// （例如自动创建的“钉钉会议”）才随连接一并移除。
pub fn delete_subscription(conn: &Connection, id: i64) -> Result<DeleteSubscriptionResult> {
    let tx = conn.unchecked_transaction()?;
    let subscription: Option<(i64, String)> = tx
        .query_row(
            "SELECT s.calendar_id, COALESCE(c.name, '')
             FROM subscriptions s
             LEFT JOIN calendars c ON c.id = s.calendar_id
             WHERE s.id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((calendar_id, calendar_name)) = subscription else {
        tx.commit()?;
        return Ok(DeleteSubscriptionResult {
            deleted_connection: false,
            deleted_events: 0,
            deleted_calendar: false,
            calendar_name: String::new(),
        });
    };
    let event_ids: Vec<i64> = {
        let mut stmt =
            tx.prepare("SELECT event_id FROM event_sources WHERE subscription_id = ?1")?;
        let rows = stmt.query_map(params![id], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    tx.execute(
        "DELETE FROM event_sources WHERE subscription_id = ?1",
        params![id],
    )?;
    for event_id in &event_ids {
        tx.execute(
            "DELETE FROM reminder_log WHERE event_id = ?1",
            params![event_id],
        )?;
        tx.execute(
            "DELETE FROM event_exceptions WHERE event_id = ?1",
            params![event_id],
        )?;
        tx.execute("DELETE FROM events WHERE id = ?1", params![event_id])?;
    }
    tx.execute("DELETE FROM subscriptions WHERE id = ?1", params![id])?;

    let remaining_subscriptions: i64 = tx.query_row(
        "SELECT COUNT(*) FROM subscriptions WHERE calendar_id = ?1",
        params![calendar_id],
        |row| row.get(0),
    )?;
    let remaining_events: i64 = tx.query_row(
        "SELECT COUNT(*) FROM events WHERE calendar_id = ?1",
        params![calendar_id],
        |row| row.get(0),
    )?;
    let is_default_calendar = matches!(calendar_name.as_str(), "默认" | "工作" | "个人" | "家庭");
    let deleted_calendar = calendar_id != 1
        && !is_default_calendar
        && remaining_subscriptions == 0
        && remaining_events == 0
        && tx.execute("DELETE FROM calendars WHERE id = ?1", params![calendar_id])? > 0;
    tx.commit()?;
    Ok(DeleteSubscriptionResult {
        deleted_connection: true,
        deleted_events: event_ids.len(),
        deleted_calendar,
        calendar_name,
    })
}

// ------------------------------ Shift（本地排班） ------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftType {
    pub id: i64,
    pub name: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub color: String,
    pub is_rest: bool,
    pub duration_minutes: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShiftAssignment {
    pub id: i64,
    pub shift_type_id: i64,
    pub shift_date: String,
    pub shift_name: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub color: String,
    pub is_rest: bool,
    pub duration_minutes: i64,
}

fn row_to_shift_type(row: &rusqlite::Row) -> rusqlite::Result<ShiftType> {
    Ok(ShiftType {
        id: row.get(0)?,
        name: row.get(1)?,
        start_time: row.get(2)?,
        end_time: row.get(3)?,
        color: row.get(4)?,
        is_rest: row.get::<_, i64>(5)? != 0,
        duration_minutes: row.get(6)?,
        created_at: row.get(7)?,
    })
}

pub fn list_shift_types(conn: &Connection) -> Result<Vec<ShiftType>> {
    let mut stmt = conn.prepare("SELECT id, name, start_time, end_time, color, is_rest, duration_minutes, created_at FROM shift_types ORDER BY id")?;
    let rows = stmt
        .query_map([], row_to_shift_type)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn create_shift_type(
    conn: &Connection,
    name: &str,
    start_time: Option<&str>,
    end_time: Option<&str>,
    color: &str,
    is_rest: bool,
    duration_minutes: i64,
) -> Result<ShiftType> {
    anyhow::ensure!(!name.trim().is_empty(), "班次名称不能为空");
    conn.execute(
        "INSERT INTO shift_types (name, start_time, end_time, color, is_rest, duration_minutes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![name.trim(), start_time, end_time, color, is_rest as i64, duration_minutes.max(0), now()],
    )?;
    let id = conn.last_insert_rowid();
    let mut stmt = conn.prepare("SELECT id, name, start_time, end_time, color, is_rest, duration_minutes, created_at FROM shift_types WHERE id = ?1")?;
    Ok(stmt.query_row(params![id], row_to_shift_type)?)
}

pub fn list_shift_assignments(
    conn: &Connection,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<ShiftAssignment>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.shift_type_id, a.shift_date, t.name, t.start_time, t.end_time, t.color, t.is_rest, t.duration_minutes
         FROM shift_assignments a JOIN shift_types t ON t.id = a.shift_type_id
         WHERE a.shift_date BETWEEN ?1 AND ?2 ORDER BY a.shift_date, a.id",
    )?;
    let rows = stmt
        .query_map(params![start.to_string(), end.to_string()], |row| {
            Ok(ShiftAssignment {
                id: row.get(0)?,
                shift_type_id: row.get(1)?,
                shift_date: row.get(2)?,
                shift_name: row.get(3)?,
                start_time: row.get(4)?,
                end_time: row.get(5)?,
                color: row.get(6)?,
                is_rest: row.get::<_, i64>(7)? != 0,
                duration_minutes: row.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn generate_shift_assignments(
    conn: &Connection,
    start: NaiveDate,
    end: NaiveDate,
    sequence: &[i64],
) -> Result<usize> {
    anyhow::ensure!(end >= start, "排班结束日期不能早于开始日期");
    anyhow::ensure!(!sequence.is_empty(), "排班序列不能为空");
    for id in sequence {
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) FROM shift_types WHERE id = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )? > 0;
        anyhow::ensure!(exists, "不存在的班次 ID：{id}");
    }
    let mut day = start;
    let mut index = 0usize;
    let mut count = 0usize;
    while day <= end {
        conn.execute(
            "INSERT INTO shift_assignments (shift_type_id, shift_date, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(shift_date) DO UPDATE SET shift_type_id = excluded.shift_type_id, created_at = excluded.created_at",
            params![sequence[index % sequence.len()], day.to_string(), now()],
        )?;
        count += 1;
        index += 1;
        day += chrono::Duration::days(1);
    }
    Ok(count)
}

pub fn delete_shift_assignment(conn: &Connection, id: i64) -> Result<usize> {
    Ok(conn.execute("DELETE FROM shift_assignments WHERE id = ?1", params![id])?)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub title: String,
    pub date: String,
    pub time: Option<String>,
    pub duration_minutes: i64,
    pub note: String,
    pub repeat_rule: String,
    pub reminder_offsets: String,
    pub category: String,
    pub calendar_id: i64,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default = "default_event_source_kind")]
    pub source_kind: String,
}

fn default_event_source_kind() -> String {
    "local".to_string()
}

/// 新建日程所需的完整字段。使用命名字段避免多个 `&str` / `Option<&str>`
/// 在调用处因顺序相近而被误传。
#[derive(Debug, Clone, Copy)]
pub struct NewEvent<'a> {
    pub title: &'a str,
    pub date: NaiveDate,
    pub time: Option<&'a str>,
    pub note: &'a str,
    pub repeat_rule: &'a str,
    pub reminder_offsets: &'a str,
    pub category: &'a str,
    pub calendar_id: i64,
}

/// 日程的局部修改；`None` 表示保留原值，`Some(None)` 可明确清空时间。
#[derive(Debug, Clone, Copy, Default)]
pub struct EventUpdate<'a> {
    pub title: Option<&'a str>,
    pub date: Option<NaiveDate>,
    pub time: Option<Option<&'a str>>,
    pub note: Option<&'a str>,
    pub repeat_rule: Option<&'a str>,
    pub reminder_offsets: Option<&'a str>,
    pub category: Option<&'a str>,
    pub calendar_id: Option<i64>,
    pub duration_minutes: Option<i64>,
}

/// 订阅日历中的一条远端事件镜像。
#[derive(Debug, Clone, Copy)]
pub struct SubscribedEvent<'a> {
    pub subscription_id: i64,
    pub calendar_id: i64,
    pub external_uid: &'a str,
    pub title: &'a str,
    pub date: NaiveDate,
    pub time: Option<&'a str>,
    pub duration_minutes: i64,
    pub note: &'a str,
    pub repeat_rule: &'a str,
}

fn row_to_event(row: &rusqlite::Row) -> rusqlite::Result<Event> {
    Ok(Event {
        id: row.get(0)?,
        title: row.get(1)?,
        date: row.get(2)?,
        time: row.get(3)?,
        duration_minutes: row.get(4)?,
        note: row.get(5)?,
        repeat_rule: row.get(6)?,
        reminder_offsets: row.get(7)?,
        category: row.get(8)?,
        calendar_id: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        source_kind: row.get(12)?,
    })
}

const EVENT_COLUMNS: &str =
    "id, title, date, time, duration_minutes, note, repeat_rule, reminder_offsets, category, calendar_id, created_at, updated_at, source_kind";

/// 列出在 [start, end] 区间内有"至少一次基准记录落在区间"的日程（不展开重复）。
/// 展示层应优先使用 [`list_event_occurrences`]，它会把重复日程展开成每一次具体发生日期。
pub fn list_events(conn: &Connection, start: NaiveDate, end: NaiveDate) -> Result<Vec<Event>> {
    let sql = format!(
        "SELECT {EVENT_COLUMNS} FROM events WHERE date BETWEEN ?1 AND ?2 ORDER BY date, time IS NULL, time"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![start.to_string(), end.to_string()], row_to_event)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list_all_events(conn: &Connection) -> Result<Vec<Event>> {
    let sql = format!("SELECT {EVENT_COLUMNS} FROM events ORDER BY date, time IS NULL, time, id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], row_to_event)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 列出生日、纪念日和倒数日等长期节点，供记录中心管理。
pub fn list_special_events(conn: &Connection) -> Result<Vec<Event>> {
    let sql = format!(
        "SELECT {EVENT_COLUMNS} FROM events WHERE category != 'event' ORDER BY date, id DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], row_to_event)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 一条具体的日程发生：`event` 是原始记录（重复规则、提醒设置等都在这里），
/// `occurrence_date` 是这一次具体落在哪一天。
#[derive(Debug, Clone, Serialize)]
pub struct EventOccurrence {
    pub event: Event,
    pub occurrence_date: String,
}

/// 列出 [start, end] 区间内的所有日程发生（含重复日程展开后的每一次）。
/// 用于日历网格标记、当日详情列表、提醒扫描等所有需要“具体哪天有什么事”的场景。
pub fn list_event_occurrences(
    conn: &Connection,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<EventOccurrence>> {
    // 非重复日程只需要基准日期落在区间内；重复日程的基准日期可能早于区间开始，
    // 所以要单独取出全部“正在重复”的日程再展开。
    let sql = format!(
        "SELECT {EVENT_COLUMNS} FROM events WHERE repeat_rule != 'none' OR date BETWEEN ?1 AND ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let events = stmt
        .query_map(params![start.to_string(), end.to_string()], row_to_event)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Exact ISO dates skip one occurrence. A `from:YYYY-MM-DD` marker truncates
    // a recurring series immediately before that occurrence without changing
    // the recurrence rule or manufacturing a large number of future rows.
    let mut exception_stmt = conn.prepare(
        "SELECT event_id, occurrence_date FROM event_exceptions \
         WHERE occurrence_date BETWEEN ?1 AND ?2 OR occurrence_date LIKE 'from:%'",
    )?;
    let exception_rows = exception_stmt
        .query_map(params![start.to_string(), end.to_string()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut exact_exceptions = std::collections::HashSet::new();
    let mut series_cutoffs: std::collections::HashMap<i64, NaiveDate> =
        std::collections::HashMap::new();
    for (event_id, value) in exception_rows {
        if let Some(date) = value.strip_prefix("from:") {
            if let Ok(date) = NaiveDate::parse_from_str(date, "%Y-%m-%d") {
                series_cutoffs
                    .entry(event_id)
                    .and_modify(|current| *current = (*current).min(date))
                    .or_insert(date);
            }
        } else {
            exact_exceptions.insert((event_id, value));
        }
    }

    let mut out = Vec::new();
    for event in events {
        let Ok(base) = NaiveDate::parse_from_str(&event.date, "%Y-%m-%d") else {
            continue;
        };
        let rule = crate::recurrence::RepeatRule::parse(&event.repeat_rule);
        for occ in crate::recurrence::occurrences_in_range(base, rule, start, end) {
            if series_cutoffs
                .get(&event.id)
                .is_some_and(|cutoff| occ >= *cutoff)
                || exact_exceptions.contains(&(event.id, occ.to_string()))
            {
                continue;
            }
            out.push(EventOccurrence {
                event: event.clone(),
                occurrence_date: occ.to_string(),
            });
        }
    }
    out.sort_by(|a, b| {
        (&a.occurrence_date, &a.event.time).cmp(&(&b.occurrence_date, &b.event.time))
    });
    Ok(out)
}

pub fn get_event(conn: &Connection, id: i64) -> Result<Option<Event>> {
    let sql = format!("SELECT {EVENT_COLUMNS} FROM events WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], row_to_event)?;
    Ok(rows.next().transpose()?)
}

pub fn create_event(conn: &Connection, event: NewEvent<'_>) -> Result<Event> {
    let ts = now();
    conn.execute(
        "INSERT INTO events (title, date, time, note, repeat_rule, reminder_offsets, category, calendar_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![
            event.title,
            event.date.to_string(),
            event.time,
            event.note,
            event.repeat_rule,
            event.reminder_offsets,
            event.category,
            event.calendar_id,
            ts
        ],
    )?;
    let id = conn.last_insert_rowid();
    get_event(conn, id)?.context("刚插入的日程读取失败")
}

pub fn update_event(conn: &Connection, id: i64, changes: EventUpdate<'_>) -> Result<Event> {
    let existing = get_event(conn, id)?.context("日程不存在")?;
    let new_title = changes.title.unwrap_or(&existing.title);
    let new_date = changes
        .date
        .map(|d| d.to_string())
        .unwrap_or(existing.date.clone());
    let new_time = changes.time.unwrap_or(existing.time.as_deref());
    let new_note = changes.note.unwrap_or(&existing.note);
    let new_repeat_rule = changes.repeat_rule.unwrap_or(&existing.repeat_rule);
    let new_reminder_offsets = changes
        .reminder_offsets
        .unwrap_or(&existing.reminder_offsets);
    let new_category = changes.category.unwrap_or(&existing.category);
    let new_calendar_id = changes.calendar_id.unwrap_or(existing.calendar_id);
    let new_duration_minutes = changes
        .duration_minutes
        .unwrap_or(existing.duration_minutes);
    conn.execute(
        "UPDATE events SET title = ?1, date = ?2, time = ?3, note = ?4, repeat_rule = ?5, \
         reminder_offsets = ?6, category = ?7, calendar_id = ?8, duration_minutes = ?9, updated_at = ?10 WHERE id = ?11",
        params![
            new_title,
            new_date,
            new_time,
            new_note,
            new_repeat_rule,
            new_reminder_offsets,
            new_category,
            new_calendar_id,
            new_duration_minutes,
            now(),
            id
        ],
    )?;
    get_event(conn, id)?.context("更新后的日程读取失败")
}

pub fn delete_event(conn: &Connection, id: i64) -> Result<usize> {
    let is_local = conn
        .query_row(
            "SELECT source_kind = 'local' FROM events WHERE id = ?1",
            params![id],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(false);
    if !is_local {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM reminder_log WHERE event_id = ?1", params![id])?;
    tx.execute(
        "DELETE FROM event_exceptions WHERE event_id = ?1",
        params![id],
    )?;
    tx.execute("DELETE FROM event_sources WHERE event_id = ?1", params![id])?;
    let affected = tx.execute("DELETE FROM events WHERE id = ?1", params![id])?;
    tx.commit()?;
    Ok(affected)
}

pub fn set_event_source_kind(conn: &Connection, id: i64, source_kind: &str) -> Result<()> {
    anyhow::ensure!(
        matches!(
            source_kind,
            "local" | "shared" | "imported" | "subscription"
        ),
        "未知日程来源"
    );
    conn.execute(
        "UPDATE events SET source_kind = ?1 WHERE id = ?2",
        params![source_kind, id],
    )?;
    Ok(())
}

fn local_event_at_occurrence(
    conn: &Connection,
    id: i64,
    occurrence_date: NaiveDate,
) -> Result<Event> {
    let event = get_event(conn, id)?.context("日程不存在")?;
    anyhow::ensure!(event.source_kind == "local", "共享或外部日程不能在本地删除");
    let base = NaiveDate::parse_from_str(&event.date, "%Y-%m-%d").context("日程日期无效")?;
    let occurs = crate::recurrence::occurrences_in_range(
        base,
        crate::recurrence::RepeatRule::parse(&event.repeat_rule),
        occurrence_date,
        occurrence_date,
    )
    .contains(&occurrence_date);
    anyhow::ensure!(occurs, "所选日期不是该日程的发生日期");
    Ok(event)
}

/// 删除一条本地日程的本次发生。非重复日程直接删除记录；重复日程只写入
/// 当天例外，过去和未来的其他发生均保留。
pub fn delete_event_occurrence(
    conn: &Connection,
    id: i64,
    occurrence_date: NaiveDate,
) -> Result<usize> {
    let event = local_event_at_occurrence(conn, id, occurrence_date)?;
    if event.repeat_rule == "none" {
        return delete_event(conn, id);
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM reminder_log WHERE event_id = ?1 AND occurrence_date = ?2",
        params![id, occurrence_date.to_string()],
    )?;
    let affected = tx.execute(
        "INSERT OR IGNORE INTO event_exceptions (event_id, occurrence_date) VALUES (?1, ?2)",
        params![id, occurrence_date.to_string()],
    )?;
    tx.commit()?;
    Ok(affected)
}

/// 删除一条本地日程从本次开始的整个后续序列。非重复日程等同于删除该条；
/// 重复日程使用一个持久化截止标记，避免无限枚举未来日期。
pub fn delete_event_from_occurrence(
    conn: &Connection,
    id: i64,
    occurrence_date: NaiveDate,
) -> Result<usize> {
    let event = local_event_at_occurrence(conn, id, occurrence_date)?;
    if event.repeat_rule == "none" {
        return delete_event(conn, id);
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM reminder_log WHERE event_id = ?1 AND occurrence_date >= ?2",
        params![id, occurrence_date.to_string()],
    )?;
    tx.execute(
        "DELETE FROM event_exceptions WHERE event_id = ?1 AND occurrence_date LIKE 'from:%'",
        params![id],
    )?;
    let affected = tx.execute(
        "INSERT INTO event_exceptions (event_id, occurrence_date) VALUES (?1, ?2)",
        params![id, format!("from:{occurrence_date}")],
    )?;
    tx.commit()?;
    Ok(affected)
}

/// 记录一条提醒已经发送过，返回 false 表示这条提醒之前已经发送过（本次应跳过）。
pub fn mark_reminder_sent(
    conn: &Connection,
    event_id: i64,
    occurrence_date: &str,
    offset_minutes: i64,
) -> Result<bool> {
    let affected = conn.execute(
        "INSERT OR IGNORE INTO reminder_log (event_id, occurrence_date, offset_minutes, notified_at) VALUES (?1, ?2, ?3, ?4)",
        params![event_id, occurrence_date, offset_minutes, now()],
    )?;
    Ok(affected > 0)
}

// ------------------------------ Todo ------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: i64,
    pub title: String,
    pub done: bool,
    pub due_date: Option<String>,
    pub priority: i64,
    pub important: bool,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

fn row_to_todo(row: &rusqlite::Row) -> rusqlite::Result<Todo> {
    Ok(Todo {
        id: row.get(0)?,
        title: row.get(1)?,
        done: row.get::<_, i64>(2)? != 0,
        due_date: row.get(3)?,
        priority: row.get(4)?,
        important: row.get::<_, i64>(5)? != 0,
        status: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

const TODO_COLUMNS: &str =
    "id, title, done, due_date, priority, important, status, created_at, updated_at";

pub fn list_todos(
    conn: &Connection,
    due_date: Option<NaiveDate>,
    done: Option<bool>,
) -> Result<Vec<Todo>> {
    let mut sql = format!("SELECT {TODO_COLUMNS} FROM todos WHERE 1=1");
    let mut owned_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(d) = due_date {
        sql.push_str(" AND due_date = ?");
        owned_params.push(Box::new(d.to_string()));
    }
    if let Some(done) = done {
        sql.push_str(" AND done = ?");
        owned_params.push(Box::new(done as i64));
    }
    sql.push_str(" ORDER BY done, priority DESC, due_date IS NULL, due_date, id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = owned_params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(param_refs.as_slice(), row_to_todo)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 列出全部待办（不按日期/完成状态过滤），供看板视图/四象限视图使用——这两个视图关心的是
/// "所有还没做完的事怎么分布"，而不是"今天有哪些"。
pub fn list_all_todos(conn: &Connection) -> Result<Vec<Todo>> {
    let sql = format!("SELECT {TODO_COLUMNS} FROM todos ORDER BY done, priority DESC, due_date IS NULL, due_date, id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], row_to_todo)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get_todo(conn: &Connection, id: i64) -> Result<Option<Todo>> {
    let sql = format!("SELECT {TODO_COLUMNS} FROM todos WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], row_to_todo)?;
    Ok(rows.next().transpose()?)
}

pub fn create_todo(
    conn: &Connection,
    title: &str,
    due_date: Option<NaiveDate>,
    priority: i64,
) -> Result<Todo> {
    let ts = now();
    conn.execute(
        "INSERT INTO todos (title, done, due_date, priority, important, status, created_at, updated_at) VALUES (?1, 0, ?2, ?3, 0, 'todo', ?4, ?4)",
        params![title, due_date.map(|d| d.to_string()), priority, ts],
    )?;
    let id = conn.last_insert_rowid();
    get_todo(conn, id)?.context("刚插入的待办读取失败")
}

pub fn set_todo_done(conn: &Connection, id: i64, done: bool) -> Result<Todo> {
    conn.execute(
        "UPDATE todos SET done = ?1, status = ?2, updated_at = ?3 WHERE id = ?4",
        params![done as i64, if done { "done" } else { "todo" }, now(), id],
    )?;
    get_todo(conn, id)?.context("更新后的待办读取失败")
}

pub fn toggle_todo(conn: &Connection, id: i64) -> Result<Todo> {
    let existing = get_todo(conn, id)?.context("待办不存在")?;
    set_todo_done(conn, id, !existing.done)
}

/// 设置"重要"维度（四象限视图用；"紧急"维度由 due_date 是否临近自动推算，不需要单独存储）。
pub fn set_todo_important(conn: &Connection, id: i64, important: bool) -> Result<Todo> {
    conn.execute(
        "UPDATE todos SET important = ?1, updated_at = ?2 WHERE id = ?3",
        params![important as i64, now(), id],
    )?;
    get_todo(conn, id)?.context("更新后的待办读取失败")
}

/// 设置看板列状态（todo/doing/done）；切到 done 时同步旧的 `done` 布尔字段，
/// 保持"列表视图勾选完成"和"看板视图拖到已完成列"两种操作方式的数据一致。
pub fn set_todo_status(conn: &Connection, id: i64, status: &str) -> Result<Todo> {
    anyhow::ensure!(
        matches!(status, "todo" | "doing" | "done"),
        "无效的待办状态：{status}（应为 todo、doing 或 done）"
    );
    let done = status == "done";
    conn.execute(
        "UPDATE todos SET status = ?1, done = ?2, updated_at = ?3 WHERE id = ?4",
        params![status, done as i64, now(), id],
    )?;
    get_todo(conn, id)?.context("更新后的待办读取失败")
}

pub fn delete_todo(conn: &Connection, id: i64) -> Result<usize> {
    Ok(conn.execute("DELETE FROM todos WHERE id = ?1", params![id])?)
}

// ------------------------------ Note ------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

fn row_to_note(row: &rusqlite::Row) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

const NOTE_COLUMNS: &str = "id, title, content, created_at, updated_at";

pub fn list_notes(conn: &Connection) -> Result<Vec<Note>> {
    let sql = format!("SELECT {NOTE_COLUMNS} FROM notes ORDER BY id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], row_to_note)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn create_note(conn: &Connection, title: &str, content: &str) -> Result<Note> {
    let ts = now();
    conn.execute(
        "INSERT INTO notes (title, content, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![title, content, ts],
    )?;
    let id = conn.last_insert_rowid();
    let sql = format!("SELECT {NOTE_COLUMNS} FROM notes WHERE id = ?1");
    Ok(conn.query_row(&sql, params![id], row_to_note)?)
}

pub fn update_note(conn: &Connection, id: i64, title: &str, content: &str) -> Result<Note> {
    conn.execute(
        "UPDATE notes SET title = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
        params![title, content, now(), id],
    )?;
    let sql = format!("SELECT {NOTE_COLUMNS} FROM notes WHERE id = ?1");
    Ok(conn.query_row(&sql, params![id], row_to_note)?)
}

pub fn delete_note(conn: &Connection, id: i64) -> Result<usize> {
    Ok(conn.execute("DELETE FROM notes WHERE id = ?1", params![id])?)
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub meta: String,
    pub date: String,
}

pub fn search(conn: &Connection, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let pattern = format!("%{query}%");
    let max = limit.min(100) as i64;
    let mut hits = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, title, date, time, category FROM events
             WHERE title LIKE ?1 OR note LIKE ?1 ORDER BY date DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, max], |row| {
            let date: String = row.get(2)?;
            let time: Option<String> = row.get(3)?;
            let category: String = row.get(4)?;
            Ok(SearchHit {
                id: row.get(0)?,
                kind: if category == "event" {
                    "日程".to_string()
                } else {
                    category_label(&category).to_string()
                },
                title: row.get(1)?,
                meta: format!(
                    "{}{}",
                    date,
                    time.map(|v| format!(" {v}")).unwrap_or_default()
                ),
                date,
            })
        })?;
        hits.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    {
        let mut stmt = conn.prepare(
            "SELECT id, title, due_date, status FROM todos
             WHERE title LIKE ?1 ORDER BY due_date IS NULL, due_date, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, max], |row| {
            let due_date: Option<String> = row.get(2)?;
            let status: String = row.get(3)?;
            Ok(SearchHit {
                id: row.get(0)?,
                kind: "待办".to_string(),
                title: row.get(1)?,
                meta: format!(
                    "{}{}",
                    status_label(&status),
                    due_date
                        .as_deref()
                        .map(|v| format!(" · {v}"))
                        .unwrap_or_default()
                ),
                date: due_date.unwrap_or_default(),
            })
        })?;
        hits.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    {
        let mut stmt = conn.prepare(
            "SELECT id, title, content, created_at FROM notes
             WHERE title LIKE ?1 OR content LIKE ?1 ORDER BY updated_at DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, max], |row| {
            let created_at: String = row.get(3)?;
            let content: String = row.get(2)?;
            Ok(SearchHit {
                id: row.get(0)?,
                kind: "便签".to_string(),
                title: row.get(1)?,
                meta: content.chars().take(80).collect(),
                date: created_at.chars().take(10).collect(),
            })
        })?;
        hits.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    {
        let mut stmt = conn.prepare(
            "SELECT id, title, teacher, location, weekday, start_period, start_week, end_week FROM courses
             WHERE title LIKE ?1 OR teacher LIKE ?1 OR location LIKE ?1
             ORDER BY weekday, start_period, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, max], |row| {
            let teacher: String = row.get(2)?;
            let location: String = row.get(3)?;
            let weekday: i64 = row.get(4)?;
            let start_period: i64 = row.get(5)?;
            let start_week: i64 = row.get(6)?;
            let end_week: i64 = row.get(7)?;
            let mut details = Vec::new();
            if !location.is_empty() {
                details.push(location);
            }
            if !teacher.is_empty() {
                details.push(teacher);
            }
            Ok(SearchHit {
                id: row.get(0)?,
                kind: "课程".to_string(),
                title: row.get(1)?,
                meta: format!(
                    "周{} 第{}节 · 第{}–{}周{}",
                    weekday,
                    start_period,
                    start_week,
                    end_week,
                    if details.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", details.join(" · "))
                    }
                ),
                date: String::new(),
            })
        })?;
        hits.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    hits.truncate(limit.min(100));
    Ok(hits)
}

fn category_label(category: &str) -> &'static str {
    match category {
        "birthday" => "生日",
        "anniversary" => "纪念日",
        "countdown" => "倒数日",
        _ => "日程",
    }
}

fn status_label(status: &str) -> &'static str {
    match status {
        "doing" => "进行中",
        "done" => "已完成",
        _ => "待办",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Habit {
    pub id: i64,
    pub title: String,
    pub archived: bool,
    pub created_at: String,
    /// 截至今天的连续打卡天数（今天未打卡则从昨天往前算）。
    pub streak: i64,
    /// 今天是否已打卡。
    pub done_today: bool,
}

fn row_to_habit_base(row: &rusqlite::Row) -> rusqlite::Result<(i64, String, bool, String)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get::<_, i64>(2)? != 0,
        row.get(3)?,
    ))
}

/// 计算某个习惯截至 `today` 的连续打卡天数：从 `today`（若今天已打卡）或 `today` 的前一天开始，
/// 逐日往前数，遇到没打卡的日子就停止。
fn compute_streak(conn: &Connection, habit_id: i64, today: NaiveDate) -> Result<(i64, bool)> {
    let done_today: bool = conn.query_row(
        "SELECT COUNT(*) FROM habit_logs WHERE habit_id = ?1 AND log_date = ?2",
        params![habit_id, today.to_string()],
        |row| row.get::<_, i64>(0),
    )? > 0;

    let mut streak = 0i64;
    let mut day = if done_today {
        today
    } else {
        today.pred_opt().context("日期下溢")?
    };
    loop {
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) FROM habit_logs WHERE habit_id = ?1 AND log_date = ?2",
            params![habit_id, day.to_string()],
            |row| row.get::<_, i64>(0),
        )? > 0;
        if !exists {
            break;
        }
        streak += 1;
        let Some(prev) = day.pred_opt() else { break };
        day = prev;
    }
    Ok((streak, done_today))
}

pub fn list_habits(conn: &Connection, include_archived: bool) -> Result<Vec<Habit>> {
    let today = Local::now().date_naive();
    let sql = if include_archived {
        "SELECT id, title, archived, created_at FROM habits ORDER BY id DESC"
    } else {
        "SELECT id, title, archived, created_at FROM habits WHERE archived = 0 ORDER BY id DESC"
    };
    let mut stmt = conn.prepare(sql)?;
    let base = stmt
        .query_map([], row_to_habit_base)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::with_capacity(base.len());
    for (id, title, archived, created_at) in base {
        let (streak, done_today) = compute_streak(conn, id, today)?;
        out.push(Habit {
            id,
            title,
            archived,
            created_at,
            streak,
            done_today,
        });
    }
    Ok(out)
}

pub fn create_habit(conn: &Connection, title: &str) -> Result<Habit> {
    conn.execute(
        "INSERT INTO habits (title, archived, created_at) VALUES (?1, 0, ?2)",
        params![title, now()],
    )?;
    let id = conn.last_insert_rowid();
    let today = Local::now().date_naive();
    Ok(Habit {
        id,
        title: title.to_string(),
        archived: false,
        created_at: now(),
        streak: 0,
        done_today: compute_streak(conn, id, today)?.1,
    })
}

/// 打卡/取消打卡某一天（再次调用即取消），返回打卡后的最新连续天数。
pub fn toggle_habit_log(conn: &Connection, habit_id: i64, date: NaiveDate) -> Result<i64> {
    let existing: bool = conn.query_row(
        "SELECT COUNT(*) FROM habit_logs WHERE habit_id = ?1 AND log_date = ?2",
        params![habit_id, date.to_string()],
        |row| row.get::<_, i64>(0),
    )? > 0;
    if existing {
        conn.execute(
            "DELETE FROM habit_logs WHERE habit_id = ?1 AND log_date = ?2",
            params![habit_id, date.to_string()],
        )?;
    } else {
        conn.execute(
            "INSERT INTO habit_logs (habit_id, log_date) VALUES (?1, ?2)",
            params![habit_id, date.to_string()],
        )?;
    }
    Ok(compute_streak(conn, habit_id, Local::now().date_naive())?.0)
}

pub fn delete_habit(conn: &Connection, id: i64) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM habit_logs WHERE habit_id = ?1", params![id])?;
    let count = tx.execute("DELETE FROM habits WHERE id = ?1", params![id])?;
    tx.commit()?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subscription_delete_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE calendars (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                color TEXT NOT NULL,
                visible INTEGER NOT NULL DEFAULT 1,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
             );
             CREATE TABLE events (
                id INTEGER PRIMARY KEY,
                title TEXT NOT NULL,
                date TEXT NOT NULL,
                time TEXT,
                duration_minutes INTEGER NOT NULL DEFAULT 60,
                note TEXT NOT NULL DEFAULT '',
                repeat_rule TEXT NOT NULL DEFAULT 'none',
                reminder_offsets TEXT NOT NULL DEFAULT '',
                category TEXT NOT NULL DEFAULT 'event',
                source_kind TEXT NOT NULL DEFAULT 'local',
                calendar_id INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );
             CREATE TABLE subscriptions (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                url TEXT NOT NULL UNIQUE,
                calendar_id INTEGER NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                last_sync TEXT,
                last_error TEXT,
                source_type TEXT NOT NULL DEFAULT 'ics',
                username TEXT NOT NULL DEFAULT '',
                secret TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL
             );
             CREATE TABLE event_sources (
                event_id INTEGER PRIMARY KEY,
                subscription_id INTEGER NOT NULL,
                external_uid TEXT NOT NULL
             );
             CREATE TABLE event_exceptions (
                event_id INTEGER NOT NULL,
                occurrence_date TEXT NOT NULL,
                PRIMARY KEY (event_id, occurrence_date)
             );
             CREATE TABLE reminder_log (
                event_id INTEGER NOT NULL,
                occurrence_date TEXT NOT NULL,
                offset_minutes INTEGER NOT NULL,
                notified_at TEXT NOT NULL,
                PRIMARY KEY (event_id, occurrence_date, offset_minutes)
             );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn deleting_subscription_clears_mirrors_and_unused_calendar() {
        let conn = subscription_delete_test_db();
        conn.execute_batch(
            "INSERT INTO calendars VALUES (6, '钉钉会议', '#1677ff', 1, 0, 'now');
             INSERT INTO subscriptions VALUES (2, '钉钉日历', 'https://calendar.dingtalk.com/dav', 6, 1, NULL, NULL, 'caldav', 'user', 'secret', 'now');
             INSERT INTO events VALUES (20, '会议', '2026-10-01', '10:00', 60, '', 'none', '10', 'event', 'subscription', 6, 'now', 'now');
             INSERT INTO event_sources VALUES (20, 2, 'meeting-20');
             INSERT INTO event_exceptions VALUES (20, 'from:2026-10-02');
             INSERT INTO reminder_log VALUES (20, '2026-10-01', 10, 'now');",
        )
        .unwrap();

        let report = delete_subscription(&conn, 2).unwrap();
        assert!(report.deleted_connection);
        assert_eq!(report.deleted_events, 1);
        assert!(report.deleted_calendar);
        assert_eq!(report.calendar_name, "钉钉会议");
        for table in [
            "subscriptions",
            "events",
            "event_sources",
            "event_exceptions",
            "reminder_log",
            "calendars",
        ] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{table} should be empty");
        }
    }

    #[test]
    fn deleting_subscription_preserves_default_calendar_and_local_events() {
        let conn = subscription_delete_test_db();
        conn.execute_batch(
            "INSERT INTO calendars VALUES (1, '默认', '#2e6be6', 1, 0, 'now');
             INSERT INTO subscriptions VALUES (1, 'Outlook', 'https://example.com/a.ics', 1, 1, NULL, NULL, 'ics', '', '', 'now');
             INSERT INTO events VALUES (10, '订阅日程', '2026-09-04', '09:00', 60, '', 'none', '', 'event', 'subscription', 1, 'now', 'now');
             INSERT INTO events VALUES (11, '本地日程', '2026-09-04', '10:00', 60, '', 'none', '', 'event', 'local', 1, 'now', 'now');
             INSERT INTO event_sources VALUES (10, 1, 'outlook-10');",
        )
        .unwrap();

        let report = delete_subscription(&conn, 1).unwrap();
        assert!(report.deleted_connection);
        assert_eq!(report.deleted_events, 1);
        assert!(!report.deleted_calendar);
        assert!(get_event(&conn, 10).unwrap().is_none());
        assert_eq!(get_event(&conn, 11).unwrap().unwrap().title, "本地日程");
        let calendar_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM calendars", [], |row| row.get(0))
            .unwrap();
        assert_eq!(calendar_count, 1);
    }

    #[test]
    fn caldav_server_accepts_dingtalk_domain_without_scheme() {
        assert_eq!(
            normalize_caldav_server_url(" calendar.dingtalk.com ").unwrap(),
            "https://calendar.dingtalk.com/dav"
        );
        assert_eq!(
            normalize_caldav_server_url("https://calendar.dingtalk.com/").unwrap(),
            "https://calendar.dingtalk.com/dav"
        );
        assert_eq!(
            normalize_caldav_server_url("calendar.dingtalk.com/dav").unwrap(),
            "https://calendar.dingtalk.com/dav"
        );
        assert!(normalize_caldav_server_url("http://calendar.dingtalk.com")
            .unwrap_err()
            .to_string()
            .contains("HTTPS"));
    }

    fn reminder_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                date TEXT NOT NULL,
                time TEXT,
                duration_minutes INTEGER NOT NULL DEFAULT 60,
                note TEXT NOT NULL DEFAULT '',
                repeat_rule TEXT NOT NULL DEFAULT 'none',
                reminder_offsets TEXT NOT NULL DEFAULT '',
                category TEXT NOT NULL DEFAULT 'event',
                calendar_id INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                source_kind TEXT NOT NULL DEFAULT 'local'
             );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn notes_and_todos_round_trip_mixed_unicode_labels() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE todos (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                done INTEGER NOT NULL DEFAULT 0,
                due_date TEXT,
                priority INTEGER NOT NULL DEFAULT 0,
                important INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'todo',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );
             CREATE TABLE notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                content TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );",
        )
        .unwrap();

        let todo_text = "🏷️ #研发／紧急 · ①测试 𠮷";
        let note_title = "📌 灵感 #生活";
        let note_content = "中英混排 Café → ✓ ♫ 😀 𝄞\n第二行\n第三行";
        create_todo(&conn, todo_text, None, 2).unwrap();
        create_note(&conn, note_title, note_content).unwrap();

        let todos = list_todos(&conn, None, None).unwrap();
        let notes = list_notes(&conn).unwrap();
        assert_eq!(todos[0].title, todo_text);
        assert_eq!(notes[0].title, note_title);
        assert_eq!(notes[0].content, note_content);
    }

    #[test]
    fn default_reminder_is_validated_and_applied_to_existing_events() {
        let conn = reminder_test_db();
        create_event(
            &conn,
            NewEvent {
                title: "已有日程",
                date: NaiveDate::from_ymd_opt(2026, 9, 2).unwrap(),
                time: Some("09:00"),
                note: "",
                repeat_rule: "none",
                reminder_offsets: "",
                category: "event",
                calendar_id: 1,
            },
        )
        .unwrap();

        assert_eq!(default_event_reminder(&conn).unwrap(), "10");
        assert_eq!(apply_default_event_reminder(&conn, "60").unwrap(), 1);
        assert_eq!(default_event_reminder(&conn).unwrap(), "60");
        assert_eq!(get_event(&conn, 1).unwrap().unwrap().reminder_offsets, "60");
        assert!(apply_default_event_reminder(&conn, "25").is_err());
    }

    #[test]
    fn occurrence_deletion_targets_one_local_series_only() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                date TEXT NOT NULL,
                time TEXT,
                duration_minutes INTEGER NOT NULL DEFAULT 60,
                note TEXT NOT NULL DEFAULT '',
                repeat_rule TEXT NOT NULL DEFAULT 'none',
                reminder_offsets TEXT NOT NULL DEFAULT '',
                category TEXT NOT NULL DEFAULT 'event',
                calendar_id INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                source_kind TEXT NOT NULL DEFAULT 'local'
             );
             CREATE TABLE reminder_log (
                event_id INTEGER NOT NULL,
                occurrence_date TEXT NOT NULL,
                offset_minutes INTEGER NOT NULL,
                notified_at TEXT NOT NULL,
                PRIMARY KEY (event_id, occurrence_date, offset_minutes)
             );
             CREATE TABLE event_sources (
                event_id INTEGER PRIMARY KEY,
                subscription_id INTEGER NOT NULL,
                external_uid TEXT NOT NULL
             );
             CREATE TABLE event_exceptions (
                event_id INTEGER NOT NULL,
                occurrence_date TEXT NOT NULL,
                PRIMARY KEY (event_id, occurrence_date)
             );",
        )
        .unwrap();

        let target = NaiveDate::from_ymd_opt(2026, 9, 3).unwrap();
        let make = |title: &str, date: NaiveDate, repeat_rule: &str| {
            create_event(
                &conn,
                NewEvent {
                    title,
                    date,
                    time: Some("09:00"),
                    note: "",
                    repeat_rule,
                    reminder_offsets: "10",
                    category: "event",
                    calendar_id: 1,
                },
            )
            .unwrap()
        };
        let local_once = make("本地一次", target, "none");
        let repeating = make("本地重复", target - chrono::Duration::days(7), "weekly");
        let shared = make("共享", target, "none");
        set_event_source_kind(&conn, shared.id, "shared").unwrap();
        let imported = make("外部文件", target, "none");
        set_event_source_kind(&conn, imported.id, "imported").unwrap();
        let subscribed = make("订阅", target, "none");
        set_event_source_kind(&conn, subscribed.id, "subscription").unwrap();

        assert!(delete_event_occurrence(&conn, shared.id, target).is_err());
        assert!(get_event(&conn, shared.id).unwrap().is_some());

        assert_eq!(
            delete_event_occurrence(&conn, repeating.id, target).unwrap(),
            1
        );
        let target_events = list_event_occurrences(&conn, target, target).unwrap();
        assert_eq!(target_events.len(), 4);
        assert!(target_events
            .iter()
            .any(|item| item.event.id == local_once.id));
        assert!(!target_events
            .iter()
            .any(|item| item.event.id == repeating.id));
        assert!(get_event(&conn, repeating.id).unwrap().is_some());

        let next = target + chrono::Duration::days(7);
        assert_eq!(
            delete_event_from_occurrence(&conn, repeating.id, next).unwrap(),
            1
        );
        assert!(list_event_occurrences(&conn, next, next)
            .unwrap()
            .iter()
            .all(|item| item.event.id != repeating.id));

        assert_eq!(
            delete_event_occurrence(&conn, local_once.id, target).unwrap(),
            1
        );
        let remaining = list_all_events(&conn).unwrap();
        assert_eq!(remaining.len(), 4);
        assert_eq!(
            remaining
                .iter()
                .filter(|event| event.source_kind == "local")
                .map(|event| event.id)
                .collect::<Vec<_>>(),
            vec![repeating.id]
        );
    }

    fn course_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE courses (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                teacher TEXT NOT NULL DEFAULT '',
                location TEXT NOT NULL DEFAULT '',
                weekday INTEGER NOT NULL,
                start_period INTEGER NOT NULL,
                period_count INTEGER NOT NULL DEFAULT 2,
                start_week INTEGER NOT NULL DEFAULT 1,
                end_week INTEGER NOT NULL DEFAULT 18,
                color_index INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn course_conflicts_require_week_and_period_overlap() {
        let conn = course_test_db();
        let first = save_course(&conn, 0, "高等数学", "陈老师", "A101", 1, 1, 2, 1, 16, 0).unwrap();

        let conflict =
            save_course(&conn, 0, "大学英语", "李老师", "B202", 1, 2, 2, 1, 16, 1).unwrap_err();
        assert!(conflict.to_string().contains("高等数学"));

        save_course(&conn, 0, "大学英语", "李老师", "B202", 1, 2, 2, 17, 18, 1).unwrap();
        save_course(
            &conn,
            first.id,
            "高等数学",
            "陈老师",
            "A102",
            1,
            1,
            2,
            1,
            16,
            2,
        )
        .unwrap();
        assert_eq!(list_courses(&conn).unwrap().len(), 2);
    }

    #[test]
    fn course_json_schema_is_self_describing_and_old_backups_still_parse() {
        let schema = CourseJsonSchema::default();
        assert_eq!(schema.schema_id, "timehub.course/v1");
        assert_eq!(schema.weekday_values.len(), 7);
        assert!(schema.fields.iter().any(|field| field.name == "weekday"));
        assert!(schema
            .fields
            .iter()
            .any(|field| field.name == "period_count"));

        let old_backup = r#"{
            "format_version": 1,
            "exported_at": "2026-09-02 09:00:00",
            "calendars": [],
            "events": [],
            "todos": [],
            "notes": [],
            "habits": [],
            "habit_logs": []
        }"#;
        let parsed: LocalBackup = serde_json::from_str(old_backup).unwrap();
        assert!(parsed.courses.is_empty());
        assert_eq!(parsed.course_schema.schema_id, "timehub.course/v1");
    }
}
