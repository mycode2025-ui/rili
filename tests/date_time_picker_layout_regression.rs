#[test]
fn minute_mode_reserves_footer_space_before_the_popup_is_shown() {
    let source = include_str!("../ui/date-time-picker.slint");
    assert!(
        source.contains("height: root.compact ? 314px : 324px;"),
        "时间弹窗必须在 show() 前固定预留分钟微调行和底部操作栏的完整高度"
    );
    assert!(
        !source.contains("root.selecting-hour ? 300px : 324px"),
        "PopupWindow 显示后不可再依赖 selecting-hour 改变原生窗口高度"
    );
}
