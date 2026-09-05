use rili::{db, integrations};
use rusqlite::Connection;

#[test]
fn empty_unicode_note_roundtrip_and_missing_note_failure() {
    let path = std::env::temp_dir().join(format!(
        "timehub-note-test-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE notes(id INTEGER PRIMARY KEY, title TEXT, content TEXT, created_at TEXT, updated_at TEXT)").unwrap();
    let note = db::create_note(&conn, "中文🙂", "第一行\n第二行").unwrap();
    db::update_note(&conn, note.id, "", "").unwrap();
    drop(conn);
    let conn = Connection::open(&path).unwrap();
    let notes = db::list_notes(&conn).unwrap();
    assert_eq!(notes[0].content, "");
    assert_eq!(notes[0].title, "");
    assert!(db::update_note(&conn, 9999, "丢失", "不能报告成功").is_err());
    drop(conn);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn habit_deletion_rolls_back_logs_if_record_delete_fails() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE habits(id INTEGER PRIMARY KEY); CREATE TABLE habit_logs(habit_id INTEGER); INSERT INTO habits VALUES(1); INSERT INTO habit_logs VALUES(1); CREATE TRIGGER prevent_delete BEFORE DELETE ON habits BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
    assert!(db::delete_habit(&conn, 1).is_err());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM habit_logs", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        1
    );
    conn.execute_batch("DROP TRIGGER prevent_delete").unwrap();
    assert_eq!(db::delete_habit(&conn, 1).unwrap(), 1);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM habit_logs", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        0
    );
}

#[test]
fn copied_sync_errors_do_not_contain_subscription_url_secrets() {
    let result = integrations::safe_error(
        "连接失败：https://user:secret@example.com/private/path?token=secret status code 400",
    );
    assert!(!result.contains("secret"));
    assert!(!result.contains("private"));
    assert!(result.contains("400"));
}

#[test]
fn editing_connection_preserves_identity_and_saved_credentials() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE subscriptions(id INTEGER PRIMARY KEY, name TEXT, url TEXT UNIQUE, calendar_id INTEGER, enabled INTEGER, last_sync TEXT, last_error TEXT, source_type TEXT, username TEXT, secret TEXT, created_at TEXT); INSERT INTO subscriptions VALUES(1, '旧名称', 'https://example.com/dav', 9, 1, NULL, NULL, 'caldav', 'account', 'encrypted-test-value', '2026-09-05');").unwrap();
    db::edit_subscription(&conn, 1, "新名称", "").unwrap();
    let subscription = db::get_subscription(&conn, 1).unwrap().unwrap();
    assert_eq!(subscription.name, "新名称");
    assert_eq!(subscription.secret, "encrypted-test-value");
    assert_eq!(subscription.url, "https://example.com/dav");
    assert_eq!(subscription.calendar_id, 9);
    assert_eq!(subscription.username, "account");
    db::set_subscription_result(&conn, 1, Some("2026-09-05 08:00:00"), None).unwrap();
    db::set_subscription_result(&conn, 1, None, Some("连接失败")).unwrap();
    assert_eq!(
        db::get_subscription(&conn, 1)
            .unwrap()
            .unwrap()
            .last_sync
            .as_deref(),
        Some("2026-09-05 08:00:00")
    );
    assert!(db::edit_subscription(&conn, 1, "  ", "").is_err());
    assert_eq!(
        db::get_subscription(&conn, 1).unwrap().unwrap().name,
        "新名称"
    );
}
