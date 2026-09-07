//! Real Rust controller + Slint models, using an isolated database and software
//! renderer. Timings are diagnostic; deterministic model assertions gate CI.
use crate::*;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::Rgb8Pixel;

struct TestPlatform(Rc<RefCell<Vec<Rc<MinimalSoftwareWindow>>>>);
impl Platform for TestPlatform {
    fn create_window_adapter(
        &self,
    ) -> std::result::Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        self.0.borrow_mut().push(window.clone());
        Ok(window)
    }
}

fn state(conn: Connection, date: NaiveDate) -> AppState {
    AppState {
        conn,
        year: date.year(),
        month: date.month(),
        selected_day: date.day(),
        week_starts_sunday: false,
        show_week_numbers: false,
        view_mode: 0,
        todo_board_mode: 1,
        calculator_start: date.to_string(),
        calculator_end: date.to_string(),
        calculator_offset: "10".into(),
        calculator_result: String::new(),
        stopwatch_elapsed_secs: 0,
        stopwatch_started_at: None,
        pomodoro_remaining_secs: 1500,
        pomodoro_total_secs: 1500,
        pomodoro_end_at: None,
        focus_task: String::new(),
        focus_round: 1,
        search_query: String::new(),
        shift_start_date: date.to_string(),
        shift_end_date: date.to_string(),
        shift_sequence: String::new(),
        shift_result: String::new(),
        ai_input: String::new(),
        ai_draft: String::new(),
        ai_draft_title: String::new(),
        ai_draft_date: String::new(),
        ai_draft_time: String::new(),
        ai_draft_reminder: String::new(),
        default_event_reminder: "15".into(),
        subscription_name: String::new(),
        subscription_url: String::new(),
        week_anchor: date,
        timeline_anchor: date,
        course_term_start: date,
        course_week: 1,
    }
}

#[test]
fn navigation_keeps_unrelated_models_and_measures_real_controller() {
    let windows = Rc::new(RefCell::new(Vec::new()));
    slint::platform::set_platform(Box::new(TestPlatform(windows.clone()))).unwrap();
    let folder = std::env::temp_dir().join(format!(
        "timehub-navigation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::env::set_var("TIMEHUB_NATIVE_SMOKE", "1");
    std::env::set_var("TIMEHUB_SMOKE_DATA_DIR", &folder);
    let conn = db::open().unwrap();
    let date = Local::now().date_naive();
    for i in 0..100 {
        db::create_event(
            &conn,
            db::NewEvent {
                title: &format!("性能测试会议 {i}"),
                date,
                time: Some("10:00"),
                note: "",
                repeat_rule: "daily",
                reminder_offsets: "",
                category: "event",
                calendar_id: 1,
            },
        )
        .unwrap();
    }
    let state = Rc::new(RefCell::new(state(conn, date)));
    let ui = AppWindow::new().unwrap();
    let widget = WidgetWindow::new().unwrap();
    let desktop = Rc::new(DesktopWidgetWindows::new().unwrap());
    DESKTOP_WIDGET_WINDOWS.with(|slot| *slot.borrow_mut() = Some(desktop));
    refresh_all(&ui, &widget, &state);
    controllers::schedule::register_schedule_callbacks(&ui, &widget, &state);
    ui.window().set_size(slint::LogicalSize::new(960., 640.));
    ui.show().unwrap();
    let mut pixels = vec![Rgb8Pixel::default(); 960 * 640];
    let paint = |pixels: &mut [Rgb8Pixel]| {
        slint::platform::update_timers_and_animations();
        windows.borrow()[0].draw_if_needed(|renderer| {
            renderer.render(pixels, 960);
        });
    };
    paint(&mut pixels);
    for mode in [4, 6, 8, 11, 12] {
        let calendar = ui.get_days();
        let todos = ui.get_board_todo_items();
        let notes = ui.get_notes();
        let start = Instant::now();
        ui.invoke_set_view_mode(mode);
        let callback = start.elapsed();
        paint(&mut pixels);
        let frame = start.elapsed();
        assert_eq!(ui.get_view_mode(), mode);
        assert!(
            ui.get_days() == calendar,
            "navigation rebuilt calendar for {mode}"
        );
        assert!(
            ui.get_board_todo_items() == todos,
            "navigation rebuilt todos for {mode}"
        );
        assert!(
            ui.get_notes() == notes,
            "navigation rebuilt notes for {mode}"
        );
        let baseline = Instant::now();
        refresh_all(&ui, &widget, &state);
        let full = baseline.elapsed();
        eprintln!(
            "NAV mode={mode} callback_us={} first_frame_us={} full_refresh_us={}",
            callback.as_micros(),
            frame.as_micros(),
            full.as_micros()
        );
    }
    // A write is still reflected on return: resident models are maintained by
    // mutation callbacks, not dropped indiscriminately to make navigation fast.
    let note = db::create_note(&state.borrow().conn, "新便签", "实时内容").unwrap();
    refresh_notes(&ui, &widget, &state);
    ui.invoke_set_view_mode(11);
    assert!(ui.get_notes().iter().any(|item| item.id == note.id as i32));
    ui.invoke_set_view_mode(3);
    assert_eq!(ui.get_timeline_days().row_count(), 3);
    ui.invoke_set_view_mode(4);
    ui.invoke_set_view_mode(2);
    assert_eq!(ui.get_timeline_days().row_count(), 1);
    ui.hide().unwrap();
    DESKTOP_WIDGET_WINDOWS.with(|slot| *slot.borrow_mut() = None);
}
