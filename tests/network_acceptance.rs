//! Loopback-only HTTP fixture. No real account or external service is contacted.
use rili::{db, integrations};
use std::io::{Read, Write};
use std::net::TcpListener;

#[test]
fn sync_snapshot_removal_and_invalid_response_are_not_reported_as_success() {
    let folder = std::env::temp_dir().join(format!(
        "timehub-network-qa-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::env::set_var("TIMEHUB_NATIVE_SMOKE", "1");
    std::env::set_var("TIMEHUB_SMOKE_DATA_DIR", &folder);
    let conn = db::open().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/calendar.ics", listener.local_addr().unwrap());
    let calendar = db::create_calendar(&conn, "QA remote", "#3366ff").unwrap();
    let subscription = db::create_subscription(&conn, "QA remote", &url, calendar.id).unwrap();
    let event = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:qa-remote\r\nDTSTART:20260906T100000\r\nSUMMARY:QA meeting\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let cancelled = event.replace(
        "SUMMARY:QA meeting",
        "SUMMARY:QA meeting\r\nSTATUS:CANCELLED",
    );
    let series = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:qa-series\r\nDTSTART;VALUE=DATE:20260930\r\nRRULE:FREQ=WEEKLY;INTERVAL=15;BYDAY=WE;WKST=SU\r\nSUMMARY:Series\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:qa-series\r\nRECURRENCE-ID;VALUE=DATE:20270113\r\nDTSTART;VALUE=DATE:20270114\r\nSUMMARY:Moved\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let responses = [
        event.to_owned(),
        event.to_owned(),
        cancelled,
        event.to_owned(),
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n".to_owned(),
        event.to_owned(),
        "<html>Sign in required</html>".to_owned(),
        series.to_owned(),
        series.to_owned(),
        series.replace("20270114", "20270115"),
        series.replace(
            "DTSTART;VALUE=DATE:20270114",
            "DTSTART;VALUE=DATE:20270114\r\nSTATUS:CANCELLED",
        ),
        series.to_owned(),
        series.replace("INTERVAL=15", "COUNT=3"),
        series
            .split("BEGIN:VEVENT\r\nUID:qa-series\r\nRECURRENCE-ID")
            .next()
            .unwrap()
            .to_owned()
            + "END:VCALENDAR\r\n",
    ];
    let server = std::thread::spawn(move || {
        for body in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).unwrap();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/calendar\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
    });
    for expected in [1, 1, 0, 1] {
        integrations::sync_ics_subscription_with_result(&conn, &subscription).unwrap();
        assert_eq!(
            db::list_all_events(&conn).unwrap().len(),
            expected,
            "重复导入去重与显式取消事件"
        );
    }
    let local = db::create_event(
        &conn,
        db::NewEvent {
            title: "必须保留的本地事项",
            date: chrono::NaiveDate::from_ymd_opt(2026, 9, 6).unwrap(),
            time: None,
            note: "",
            repeat_rule: "none",
            reminder_offsets: "",
            category: "event",
            calendar_id: 1,
        },
    )
    .unwrap();
    integrations::sync_ics_subscription_with_result(&conn, &subscription).unwrap();
    let removed = db::subscription_summary(&conn, subscription.id).unwrap().1 == 0;
    integrations::sync_ics_subscription_with_result(&conn, &subscription).unwrap();
    let before_invalid = db::list_all_events(&conn).unwrap();
    let last_success = db::get_subscription(&conn, subscription.id)
        .unwrap()
        .unwrap()
        .last_sync;
    let invalid_rejected =
        integrations::sync_ics_subscription_with_result(&conn, &subscription).is_err();
    assert_eq!(
        db::list_all_events(&conn)
            .unwrap()
            .iter()
            .map(|event| event.id)
            .collect::<Vec<_>>(),
        before_invalid
            .iter()
            .map(|event| event.id)
            .collect::<Vec<_>>(),
        "HTML 响应不能删除已有的远端会议或本地事项"
    );
    assert!(
        db::get_event(&conn, local.id).unwrap().is_some(),
        "快照清理不能删除本地事项"
    );
    let failed = db::get_subscription(&conn, subscription.id)
        .unwrap()
        .unwrap();
    assert_eq!(failed.last_sync, last_success);
    assert!(failed.last_error.is_some());
    assert!(
        removed && invalid_rejected,
        "QA-SYNC-01/02: 远端删除后清理本地={removed}, 拒绝HTML伪日历={invalid_rejected}"
    );
    let start = chrono::NaiveDate::from_ymd_opt(2027, 1, 13).unwrap();
    let end = chrono::NaiveDate::from_ymd_opt(2027, 1, 15).unwrap();
    for expected_day in [14, 14, 15] {
        integrations::sync_ics_subscription_with_result(&conn, &subscription).unwrap();
        assert_eq!(
            db::list_all_events(&conn).unwrap().len(),
            3,
            "local + master + one override; no duplicates"
        );
        let occurrences = db::list_event_occurrences(&conn, start, end).unwrap();
        assert_eq!(occurrences.len(), 1, "original occurrence must be excluded");
        assert_eq!(
            occurrences[0].occurrence_date,
            chrono::NaiveDate::from_ymd_opt(2027, 1, expected_day)
                .unwrap()
                .to_string()
        );
    }
    integrations::sync_ics_subscription_with_result(&conn, &subscription).unwrap();
    assert!(
        db::list_event_occurrences(&conn, start, end)
            .unwrap()
            .is_empty(),
        "cancelled override excludes only its original occurrence"
    );
    integrations::sync_ics_subscription_with_result(&conn, &subscription).unwrap();
    let error = integrations::sync_ics_subscription_with_result(&conn, &subscription)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("COUNT=3"),
        "specific parsing cause is retained"
    );
    assert_eq!(
        db::list_event_occurrences(&conn, start, end).unwrap().len(),
        1,
        "invalid snapshot preserves existing data"
    );
    integrations::sync_ics_subscription_with_result(&conn, &subscription).unwrap();
    let restored = db::list_event_occurrences(&conn, start, end).unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(
        restored[0].occurrence_date,
        start.to_string(),
        "removed override restores original occurrence"
    );
    assert_eq!(
        db::list_all_events(&conn).unwrap().len(),
        2,
        "obsolete override is removed, local event remains"
    );
    server.join().unwrap();
}
