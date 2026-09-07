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
    let responses = [
        event.to_owned(),
        event.to_owned(),
        cancelled,
        event.to_owned(),
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n".to_owned(),
        event.to_owned(),
        "<html>Sign in required</html>".to_owned(),
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
    server.join().unwrap();
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
}
