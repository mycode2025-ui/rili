use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::ComponentHandle;
use std::rc::Rc;

slint::slint! {
    import { DailyQuoteWidgetWindow } from "../ui/desktop-widgets.slint";
    export component DragHarness inherits DailyQuoteWidgetWindow {
        out property <int> drag-count: 0;
        out property <int> remove-count: 0;
        begin-window-drag => { root.drag-count += 1; }
        remove-widget => { root.remove-count += 1; }
    }
}

struct TestPlatform;

impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
    }
}

fn click(ui: &DragHarness, x: f32, y: f32) {
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
fn full_header_left_area_drags_without_covering_action_buttons() {
    slint::platform::set_platform(Box::new(TestPlatform)).unwrap();
    let ui = DragHarness::new().unwrap();
    ui.window().set_size(slint::LogicalSize::new(304., 190.));
    ui.show().unwrap();

    // Top-left padding and the lower part of the 48px header were both dead
    // zones before the regression fix.
    click(&ui, 4., 4.);
    click(&ui, 150., 44.);
    assert_eq!(ui.get_drag_count(), 2);

    // The first action button begins after the drag region. It must remain a
    // button instead of starting a window move.
    click(&ui, 220., 25.);
    assert_eq!(ui.get_drag_count(), 2);
    assert_eq!(ui.get_remove_count(), 1);
}
