use super::*;

// ------------------------------ Course timetable ------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Course {
    pub id: i64,
    pub title: String,
    pub teacher: String,
    pub location: String,
    pub weekday: i64,
    pub start_period: i64,
    pub period_count: i64,
    pub start_week: i64,
    pub end_week: i64,
    pub color_index: i64,
    pub created_at: String,
    pub updated_at: String,
}

const COURSE_COLUMNS: &str = "id, title, teacher, location, weekday, start_period, period_count, start_week, end_week, color_index, created_at, updated_at";

fn row_to_course(row: &rusqlite::Row) -> rusqlite::Result<Course> {
    Ok(Course {
        id: row.get(0)?,
        title: row.get(1)?,
        teacher: row.get(2)?,
        location: row.get(3)?,
        weekday: row.get(4)?,
        start_period: row.get(5)?,
        period_count: row.get(6)?,
        start_week: row.get(7)?,
        end_week: row.get(8)?,
        color_index: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

pub fn list_courses(conn: &Connection) -> Result<Vec<Course>> {
    let sql =
        format!("SELECT {COURSE_COLUMNS} FROM courses ORDER BY weekday, start_period, title, id");
    let mut stmt = conn.prepare(&sql)?;
    let courses = stmt
        .query_map([], row_to_course)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(courses)
}

pub fn get_course(conn: &Connection, id: i64) -> Result<Option<Course>> {
    let sql = format!("SELECT {COURSE_COLUMNS} FROM courses WHERE id = ?1");
    Ok(conn
        .query_row(&sql, params![id], row_to_course)
        .optional()?)
}

#[allow(clippy::too_many_arguments)]
pub fn save_course(
    conn: &Connection,
    id: i64,
    title: &str,
    teacher: &str,
    location: &str,
    weekday: i64,
    start_period: i64,
    period_count: i64,
    start_week: i64,
    end_week: i64,
    color_index: i64,
) -> Result<Course> {
    let title = title.trim();
    anyhow::ensure!(!title.is_empty(), "请填写课程名称");
    anyhow::ensure!((1..=7).contains(&weekday), "星期设置无效");
    anyhow::ensure!((1..=12).contains(&start_period), "开始节次应为 1–12");
    anyhow::ensure!((1..=6).contains(&period_count), "连续节数应为 1–6");
    anyhow::ensure!(
        start_period + period_count - 1 <= 12,
        "课程不能超过第 12 节"
    );
    anyhow::ensure!((1..=30).contains(&start_week), "开始周次应为 1–30");
    anyhow::ensure!((1..=30).contains(&end_week), "结束周次应为 1–30");
    anyhow::ensure!(start_week <= end_week, "结束周不能早于开始周");

    let new_period_end = start_period + period_count - 1;
    for other in list_courses(conn)?
        .into_iter()
        .filter(|course| course.id != id)
    {
        let other_period_end = other.start_period + other.period_count - 1;
        let weeks_overlap = start_week <= other.end_week && other.start_week <= end_week;
        let periods_overlap =
            start_period <= other_period_end && other.start_period <= new_period_end;
        if weekday == other.weekday && weeks_overlap && periods_overlap {
            anyhow::bail!(
                "与“{}”（第 {}–{} 节，第 {}–{} 周）冲突",
                other.title,
                other.start_period,
                other_period_end,
                other.start_week,
                other.end_week
            );
        }
    }

    let ts = now();
    if id > 0 {
        anyhow::ensure!(get_course(conn, id)?.is_some(), "课程不存在");
        conn.execute(
            "UPDATE courses SET title = ?1, teacher = ?2, location = ?3, weekday = ?4, start_period = ?5, period_count = ?6, start_week = ?7, end_week = ?8, color_index = ?9, updated_at = ?10 WHERE id = ?11",
            params![title, teacher.trim(), location.trim(), weekday, start_period, period_count, start_week, end_week, color_index.clamp(0, 7), ts, id],
        )?;
        get_course(conn, id)?.context("更新后的课程读取失败")
    } else {
        conn.execute(
            "INSERT INTO courses (title, teacher, location, weekday, start_period, period_count, start_week, end_week, color_index, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![title, teacher.trim(), location.trim(), weekday, start_period, period_count, start_week, end_week, color_index.clamp(0, 7), ts],
        )?;
        get_course(conn, conn.last_insert_rowid())?.context("刚插入的课程读取失败")
    }
}

pub fn delete_course(conn: &Connection, id: i64) -> Result<usize> {
    Ok(conn.execute("DELETE FROM courses WHERE id = ?1", params![id])?)
}
