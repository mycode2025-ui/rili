use crate::*;

pub(crate) struct WidgetControllerContext<'a> {
    pub(crate) ui: &'a AppWindow,
    pub(crate) widget: &'a WidgetWindow,
    pub(crate) quick_panel: &'a QuickPanelWindow,
    pub(crate) desktop_widgets: &'a Rc<DesktopWidgetWindows>,
    pub(crate) state: &'a Rc<RefCell<AppState>>,
    pub(crate) visibility: &'a Rc<RefCell<DesktopWidgetVisibility>>,
    pub(crate) click_through: &'a Rc<Cell<bool>>,
    pub(crate) shown: &'a Rc<Cell<bool>>,
}

pub(crate) mod appearance;
pub(crate) mod assistant;
pub(crate) mod course;
pub(crate) mod data_actions;
pub(crate) mod desktop_cards;
pub(crate) mod integration;
pub(crate) mod planning;
pub(crate) mod quick_panel;
pub(crate) mod schedule;
pub(crate) mod settings;
pub(crate) mod timeline;
pub(crate) mod tools;
pub(crate) mod widget_bridge;
pub(crate) mod widget_content;
pub(crate) mod widget_manager;
