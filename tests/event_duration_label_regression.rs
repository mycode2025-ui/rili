use std::fs;

#[test]
fn event_duration_is_named_as_a_duration_and_has_room_for_the_full_label() {
    let source = fs::read_to_string("ui/new-event-dialog.slint")
        .expect("ui/new-event-dialog.slint should be readable");

    assert!(
        source
            .matches("\"时长 \" + root.draft-duration + \"分钟\"")
            .count()
            >= 2,
        "both regular and compact editors should explain that the value is an event duration"
    );
    assert!(
        source.contains("duration-box := FormBox { width: 124px;"),
        "the regular duration control must be wide enough to avoid eliding values such as 90 minutes"
    );
    assert!(
        !source.contains("text: \"持续 \" + root.draft-duration"),
        "the ambiguous '持续' label must not return"
    );
}
