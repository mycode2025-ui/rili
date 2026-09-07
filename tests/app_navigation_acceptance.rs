use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::ComponentHandle;
use std::rc::Rc;

slint::slint! {
    import { AppWindow } from "../ui/app.slint";
    export component NavigationHarness inherits AppWindow {
        width: 960px; height: 640px;
        out property <int> active-page: 0;
        out property <bool> settings-visible: root.settings-open;
        out property <bool> search-visible: root.search-open;
        out property <bool> editor-visible: root.new-event-open;
        public function reset-editor-for-test() { root.new-event-open = false; }
        view-mode: root.active-page;
        set-view-mode(mode) => { root.active-page = mode; }
    }
}

struct TestPlatform;
impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
    }
}

fn click(ui: &NavigationHarness, x: f32, y: f32) {
    let position = slint::LogicalPosition::new(x, y);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
    slint::platform::update_timers_and_animations();
}

#[test]
fn main_navigation_and_search_accept_mouse_input_at_minimum_size() {
    slint::platform::set_platform(Box::new(TestPlatform)).unwrap();
    let ui = NavigationHarness::new().unwrap();
    ui.show().unwrap();
    for (y, expected) in [
        (128., 2),
        (164., 0),
        (198., 12),
        (234., 4),
        (270., 11),
        (306., 8),
        (342., 6),
    ] {
        click(&ui, 70., y);
        assert_eq!(ui.get_active_page(), expected, "主导航点击 y={y}");
    }
    click(&ui, 70., 76.);
    assert!(ui.get_editor_visible(), "主窗口新建日程入口");
    ui.invoke_reset_editor_for_test();
    click(&ui, 300., 24.);
    assert!(ui.get_search_visible(), "全局搜索入口");
    // Escape should close the search without mutating any database.
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Escape.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Escape.into(),
    });
    let escape_closes_search = !ui.get_search_visible();
    click(&ui, 900., 500.);
    assert!(!ui.get_search_visible(), "点击搜索外部关闭");
    click(&ui, 70., 378.);
    assert!(ui.get_settings_visible(), "设置入口");
    assert!(escape_closes_search, "QA-UI-02: Esc 关闭搜索");
}
