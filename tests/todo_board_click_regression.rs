use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;

slint::slint! {
    import { KanbanView, TodoBoardItem } from "../ui/todo-board.slint";
    export component TodoClickHarness inherits Window {
        width: 960px; height: 640px;
        in property <[TodoBoardItem]> pending;
        in property <[TodoBoardItem]> doing;
        in property <[TodoBoardItem]> done;
        out property <int> moved-id: 0;
        out property <string> moved-status: "";
        out property <int> opened-id: 0;
        KanbanView {
            width: 960px; height: 640px;
            todo-items: root.pending; doing-items: root.doing; done-items: root.done;
            move-to(id, status) => { root.moved-id = id; root.moved-status = status; }
            open-item(id) => { root.opened-id = id; }
        }
    }
}

struct TestPlatform;
impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
    }
}

fn click(ui: &TodoClickHarness, x: f32, y: f32) {
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

fn item(id: i32, status: &str) -> TodoBoardItem {
    TodoBoardItem {
        id,
        title: format!("任务 {id}").into(),
        due_text: "".into(),
        is_urgent: false,
        status: status.into(),
    }
}

#[test]
fn checkbox_completes_but_title_opens_details_and_done_restores() {
    slint::platform::set_platform(Box::new(TestPlatform)).unwrap();
    let ui = TodoClickHarness::new().unwrap();
    ui.set_pending(ModelRc::new(VecModel::from(vec![item(1, "todo")])));
    ui.set_doing(ModelRc::new(VecModel::from(vec![item(2, "doing")])));
    ui.set_done(ModelRc::new(VecModel::from(vec![item(3, "done")])));
    ui.show().unwrap();

    click(&ui, 120.0, 74.0);
    assert_eq!(ui.get_opened_id(), 1);
    assert_eq!(ui.get_moved_id(), 0, "点标题不能意外完成任务");

    click(&ui, 45.0, 74.0);
    assert_eq!(ui.get_moved_id(), 1);
    assert_eq!(ui.get_moved_status(), "done");

    click(&ui, 675.0, 74.0);
    assert_eq!(ui.get_moved_id(), 3);
    assert_eq!(ui.get_moved_status(), "restore");
}
