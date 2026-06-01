use tau_computer_use_layout::{parse_frame_csv, parse_point_csv, ScreenFrame};

#[test]
fn parses_negative_frames_and_centers() {
    let frame = parse_frame_csv("-3982,-1410,1146,1410").unwrap();
    assert_eq!(frame, ScreenFrame::new(-3982, -1410, 1146, 1410));
    assert_eq!(frame.top_left().x, -3982);
    assert_eq!(frame.center().x, -3409);
    assert_eq!(frame.center().y, -705);
    assert!(frame.contains(frame.center()));
    assert!(!frame.contains(tau_computer_use_layout::ScreenPoint::new(50, 50)));
}

#[test]
fn tolerates_legacy_applescript_list_commas() {
    let frame = parse_frame_csv("-3982, ,, -1410, ,, 1146, ,, 1410").unwrap();
    assert_eq!(frame, ScreenFrame::new(-3982, -1410, 1146, 1410));

    let point = parse_point_csv("-3409, ,, -705").unwrap();
    assert_eq!(point.x, -3409);
    assert_eq!(point.y, -705);
}

#[test]
fn reports_bad_coordinates_with_layout_label() {
    let error = parse_point_csv("-3409, nope").unwrap_err();
    assert!(error.to_string().contains("invalid y in point"));
}
