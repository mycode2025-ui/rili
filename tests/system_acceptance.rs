//! Acceptance tests use fresh on-disk databases; never the user's data directory.
use chrono::NaiveDate;
use rili::{db, natural};

fn date(value: &str) -> NaiveDate {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
}

#[test]
fn local_lifecycle_backup_and_sync_source_isolation() {
    let folder = std::env::temp_dir().join(format!(
        "timehub-qa-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::env::set_var("TIMEHUB_NATIVE_SMOKE", "1");
    std::env::set_var("TIMEHUB_SMOKE_DATA_DIR", &folder);
    let mut conn = db::open().unwrap();
    let today = date("2026-09-06");
    let event = db::create_event(
        &conn,
        db::NewEvent {
            title: "QA 日程🙂",
            date: today,
            time: Some("10:30"),
            note: "多行\n说明",
            repeat_rule: "daily",
            reminder_offsets: "0,30",
            category: "event",
            calendar_id: 1,
        },
    )
    .unwrap();
    assert_eq!(
        db::list_event_occurrences(&conn, today, date("2026-09-08"))
            .unwrap()
            .len(),
        3
    );
    db::delete_event_occurrence(&conn, event.id, date("2026-09-07")).unwrap();
    assert_eq!(
        db::list_event_occurrences(&conn, today, date("2026-09-08"))
            .unwrap()
            .len(),
        2
    );
    let todo = db::create_todo(&conn, "QA 待办", Some(today), 1).unwrap();
    assert!(db::toggle_todo(&conn, todo.id).unwrap().done);
    assert!(!db::toggle_todo(&conn, todo.id).unwrap().done);
    let note = db::create_note(&conn, "QA 便签", "甲\n乙🙂").unwrap();
    db::update_note(&conn, note.id, "QA 新标题", "").unwrap();
    assert!(!db::search(&conn, "QA", 100).unwrap().is_empty());
    db::save_course(&conn, 0, "QA 课程", "老师", "教室", 1, 1, 2, 1, 16, 0).unwrap();
    assert!(db::save_course(&conn, 0, "冲突课程", "", "", 1, 2, 1, 1, 16, 0).is_err());
    assert!(db::save_course(&conn, 0, "越界课程", "", "", 8, 1, 1, 1, 16, 0).is_err());
    let habit = db::create_habit(&conn, "QA 阅读").unwrap();
    db::toggle_habit_log(&conn, habit.id, today).unwrap();
    let calendar = db::create_calendar(&conn, "QA 订阅", "#3366ff").unwrap();
    let subscription =
        db::create_subscription(&conn, "QA", "https://example.com/qa.ics", calendar.id).unwrap();
    let imported = db::SubscribedEvent {
        subscription_id: subscription.id,
        calendar_id: calendar.id,
        external_uid: "qa-1",
        title: "QA 远端",
        date: today,
        time: None,
        duration_minutes: 60,
        note: "",
        repeat_rule: "none",
    };
    let mirror = db::upsert_subscribed_event(&conn, imported).unwrap();
    assert_eq!(
        db::upsert_subscribed_event(&conn, imported).unwrap(),
        mirror
    );
    db::delete_subscription(&conn, subscription.id).unwrap();
    assert!(db::get_event(&conn, mirror).unwrap().is_none());
    assert!(db::get_event(&conn, event.id).unwrap().is_some());
    let backup = db::export_backup(&conn).unwrap();
    let mut unsupported = backup.clone();
    unsupported.format_version = 999;
    assert!(db::import_backup(&mut conn, &unsupported).is_err());
    assert_eq!(db::list_notes(&conn).unwrap().len(), 1);
    drop(conn);
    let conn = db::open().unwrap();
    assert_eq!(db::list_notes(&conn).unwrap()[0].title, "QA 新标题");
    assert_eq!(db::list_notes(&conn).unwrap()[0].content, "");
    drop(conn);

    // Restore into a genuinely empty database to test more than JSON parsing.
    std::env::set_var("TIMEHUB_SMOKE_DATA_DIR", folder.join("restore"));
    let mut restored = db::open().unwrap();
    let stats = db::import_backup(&mut restored, &backup).unwrap();
    assert_eq!(
        (
            stats.events,
            stats.todos,
            stats.notes,
            stats.courses,
            stats.habits,
            stats.habit_logs
        ),
        (1, 1, 1, 1, 1, 1)
    );
    let occurrences = db::list_event_occurrences(&restored, today, date("2026-09-08")).unwrap();
    assert_eq!(
        occurrences.len(),
        2,
        "QA-BACKUP-01: 恢复备份不能复活已经删除的单次日程"
    );
    let mut old_json = serde_json::to_value(&backup).unwrap();
    old_json.as_object_mut().unwrap().remove("event_exceptions");
    let legacy: db::LocalBackup = serde_json::from_value(old_json).unwrap();
    assert!(legacy.event_exceptions.is_empty(), "旧版备份仍应兼容");
    let mut cutoff_backup = backup.clone();
    cutoff_backup
        .event_exceptions
        .push(db::BackupEventException {
            event_id: event.id,
            occurrence_date: "from:2026-09-08".into(),
        });
    db::import_backup(&mut restored, &cutoff_backup).unwrap();
    assert_eq!(
        db::list_event_occurrences(&restored, today, date("2026-09-08"))
            .unwrap()
            .len(),
        3,
        "重新生成 ID 后单次例外和后续截止仍应关联正确"
    );
}

#[test]
fn natural_language_minutes_are_not_taken_from_the_hour() {
    let draft = natural::parse("明天下午三点十五分开会", date("2026-09-06")).unwrap();
    assert_eq!(draft.time, "15:15", "QA-NLP-01");
}

#[test]
fn natural_language_half_hour_is_preserved() {
    let draft = natural::parse("明天下午两点半开会", date("2026-09-06")).unwrap();
    assert_eq!(draft.time, "14:30", "QA-NLP-02");
}

#[test]
fn invalid_explicit_date_must_not_silently_become_today() {
    assert!(
        natural::parse("2026-02-30 开会", date("2026-09-06")).is_err(),
        "QA-NLP-03: 非法显式日期不应静默按今天创建"
    );
}

#[test]
fn title_is_not_eaten_by_a_later_reminder() {
    let draft = natural::parse("明天下午两点开会提前十分钟提醒", date("2026-09-06")).unwrap();
    assert_eq!(draft.title, "开会", "QA-NLP-04");
}

#[test]
fn utc_calendar_time_is_converted_to_local_time() {
    let events = rili::ics::parse_ics("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:qa-utc\r\nSUMMARY:UTC meeting\r\nDTSTART:20260906T180000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n").unwrap();
    // TimeHub currently runs in Asia/Shanghai. Verify both time and date rollover.
    let expected = chrono::DateTime::parse_from_rfc3339("2026-09-06T18:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Local);
    assert_eq!(
        events[0].date.to_string(),
        expected.format("%Y-%m-%d").to_string(),
        "QA-ICS-01: UTC 日期跨天错误"
    );
    assert_eq!(
        events[0].time.as_deref(),
        Some(expected.format("%H:%M").to_string().as_str())
    );
}

#[test]
fn date_calculator_boundaries_and_signed_intervals() {
    let leap = date("2024-02-28");
    assert_eq!(
        rili::date_calc::add_calendar_days(leap, 1).unwrap(),
        date("2024-02-29")
    );
    assert_eq!(
        rili::date_calc::add_calendar_days(leap, 2).unwrap(),
        date("2024-03-01")
    );
    assert!(rili::date_calc::add_calendar_days(leap, i64::MAX).is_err());
    assert!(rili::date_calc::add_workdays(leap, i64::MIN).is_err());
    let forward = rili::date_calc::diff(date("2026-09-01"), date("2026-10-08"));
    let reverse = rili::date_calc::diff(date("2026-10-08"), date("2026-09-01"));
    assert_eq!(forward.calendar_days, -reverse.calendar_days);
    assert_eq!(forward.workdays, -reverse.workdays);
}

#[cfg(windows)]
#[test]
fn credential_roundtrip_and_malformed_ciphertext_rejection() {
    let sample = "qa-only-非真实凭据🙂";
    let encrypted = rili::secret_store::protect(sample).unwrap();
    assert!(!encrypted.contains(sample));
    assert_eq!(rili::secret_store::unprotect(&encrypted).unwrap(), sample);
    assert!(rili::secret_store::unprotect("plaintext").is_err());
    assert!(rili::secret_store::unprotect("dpapi:not-valid-base64!").is_err());
}
