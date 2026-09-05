use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter, WindowEvent};
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

slint::slint! {
    import { NotesView } from "../ui/notes-view.slint";
    export component NoteHarness inherits Window {
        width: 720px; height: 560px;
        in-out property <int> note-id <=> editor.selected-id;
        in-out property <string> title-text <=> editor.draft-title;
        in-out property <string> body-text <=> editor.draft-content;
        out property <string> save-status <=> editor.save-status;
        callback persist(int, string, string) -> bool;
        callback create(string, string) -> int;
        public function flush() { editor.auto-save-current(); }
        public function new-note() { editor.start-new-note(); }
        public function focus-body() { editor.focus-body(); }
        public function focus-title() { editor.focus-title(); }
        public function undo() { editor.undo-active(); }
        public function redo() { editor.redo-active(); }
        editor := NotesView {
            compact-layout: true;
            update-note(id, title, content) => { return root.persist(id, title, content); }
            add-note(title, content) => { return root.create(title, content); }
        }
    }
}

struct TestPlatform;
impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
    }
}

#[test]
fn notes_clear_multiline_autosave_and_failure_guard() {
    slint::platform::set_platform(Box::new(TestPlatform)).unwrap();
    let ui = NoteHarness::new().unwrap();
    let saved = Rc::new(RefCell::new((String::new(), String::new())));
    let fail = Rc::new(Cell::new(false));
    let creates = Rc::new(Cell::new(0));
    let (sink, failure) = (saved.clone(), fail.clone());
    ui.on_persist(move |_, title, body| {
        if failure.get() {
            return false;
        }
        *sink.borrow_mut() = (title.into(), body.into());
        true
    });
    let count = creates.clone();
    ui.on_create(move |_, _| {
        count.set(count.get() + 1);
        7
    });
    ui.set_note_id(7);
    ui.set_title_text("旧标题".into());
    ui.set_body_text("旧内容".into());
    ui.invoke_flush();
    ui.set_title_text("".into());
    ui.set_body_text("".into());
    ui.invoke_flush();
    assert_eq!(*saved.borrow(), (String::new(), String::new()));
    assert_eq!(ui.get_save_status(), "已保存");

    fail.set(true);
    ui.set_body_text("不能丢失的草稿".into());
    ui.invoke_flush();
    ui.invoke_new_note();
    assert_eq!(ui.get_body_text(), "不能丢失的草稿");
    assert!(ui.get_save_status().contains("失败"));
    fail.set(false);
    ui.invoke_flush();
    ui.invoke_new_note();
    assert_eq!(creates.get(), 0, "新建空草稿不能立即产生数据库垃圾");

    ui.set_note_id(7);
    ui.show().unwrap();
    ui.invoke_focus_body();
    for text in ["第一行", "\n", "第二行🙂"] {
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: text.into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: text.into() });
    }
    assert_eq!(ui.get_body_text(), "第一行\n第二行🙂");
    assert_eq!(saved.borrow().1, "第一行\n第二行🙂");
    let completed = ui.get_body_text();
    ui.invoke_undo();
    assert_ne!(ui.get_body_text(), completed);
    ui.invoke_redo();
    assert_eq!(ui.get_body_text(), completed);
    assert_eq!(saved.borrow().1, completed.as_str());

    ui.invoke_focus_title();
    slint::platform::update_timers_and_animations();
    for text in ["标题", "补充"] {
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: text.into() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text: text.into() });
    }
    let title = ui.get_title_text();
    ui.invoke_undo();
    assert_ne!(ui.get_title_text(), title, "标题撤销不能误操作正文");
    assert_eq!(ui.get_body_text(), completed);
    ui.invoke_redo();
    assert_eq!(ui.get_title_text(), title);
    assert_eq!(saved.borrow().0, title.as_str());

    ui.invoke_focus_body();
    slint::platform::update_timers_and_animations();
    ui.invoke_undo();
    assert_ne!(ui.get_body_text(), completed, "切回正文后应撤销正文");
    assert_eq!(ui.get_title_text(), title);
    ui.invoke_redo();
    assert_eq!(ui.get_body_text(), completed);
}
