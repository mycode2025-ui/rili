use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;

slint::slint! {
    import { CalendarView, CalendarDay } from "../ui/calendar-view.slint";
    import { FocusTouchArea } from "../ui/widgets.slint";
    export component ClickHarness inherits Window {
        width: 700px; height: 720px;
        in property <[CalendarDay]> days;
        out property <int> opened: 0;
        out property <string> occurrence;
        out property <int> selected: 0;
        out property <int> presses: 0;
        in-out property <bool> active: true;
        out property <length> hit-width: hit.width;
        out property <length> hit-height: hit.height;
        public function focus-control() { hit.focus(); }
        CalendarView {
            x: 0px; y: 0px; width: 700px; height: 636px;
            days: root.days;
            open-event(id, date) => { root.opened = id; root.occurrence = date; }
            select-day(day) => { root.selected = day; }
        }
        Rectangle {
            x: 10px; y: 650px; width: 300px; height: 50px;
            hit := FocusTouchArea { enabled: root.active; clicked => { root.presses += 1; } }
        }
    }
}

struct TestPlatform;
impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
    }
}

fn click(ui: &ClickHarness, x: f32, y: f32) {
    let position = slint::LogicalPosition::new(x, y);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

#[test]
fn calendar_tags_dates_and_default_hit_area_receive_input() {
    slint::platform::set_platform(Box::new(TestPlatform)).unwrap();
    let ui = ClickHarness::new().unwrap();
    let mut days = vec![CalendarDay::default(); 42];
    for (i, day) in days.iter_mut().enumerate() {
        day.day = (i + 1) as i32;
        day.in_current_month = true;
    }
    days[0].date = "2026-09-01".into();
    days[0].event_tags = ModelRc::new(VecModel::from(vec![EventTag {
        id: 42,
        title: "测试会议".into(),
        color: slint::Color::from_rgb_u8(40, 100, 220),
    }]));
    ui.set_days(ModelRc::new(VecModel::from(days)));
    ui.show().unwrap();
    assert_eq!(ui.get_hit_width(), 300.0);
    assert_eq!(ui.get_hit_height(), 50.0);
    click(&ui, 50.0, 79.0);
    assert_eq!(ui.get_opened(), 42, "鼠标点击标签必须传递真实日程 ID");
    assert_eq!(ui.get_occurrence(), "2026-09-01");
    assert_eq!(ui.get_selected(), 0, "标签点击不能被日期背景吞掉");
    click(&ui, 150.0, 120.0);
    assert_eq!(ui.get_selected(), 2);
    click(&ui, 30.0, 675.0);
    assert_eq!(ui.get_presses(), 1);
    ui.invoke_focus_control();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: " ".into() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: " ".into() });
    assert_eq!(ui.get_presses(), 2);
    ui.set_active(false);
    click(&ui, 30.0, 675.0);
    assert_eq!(ui.get_presses(), 2, "禁用控件不可触发");
}
