use tau_computer_use_layout::ScreenFrame;
use tau_desktop_capture::{screencapture_args, CapturedImage};

#[test]
fn builds_fullscreen_capture_args() {
    assert_eq!(
        screencapture_args(None, "/tmp/capture.png"),
        ["-x", "-t", "png", "/tmp/capture.png"]
    );
}

#[test]
fn builds_region_capture_args_with_negative_coordinates() {
    assert_eq!(
        screencapture_args(
            Some(ScreenFrame::new(-3982, -1410, 1146, 1410)),
            "/tmp/c.png"
        ),
        [
            "-x",
            "-t",
            "png",
            "-R",
            "-3982,-1410,1146,1410",
            "/tmp/c.png"
        ]
    );
}

#[test]
fn captured_png_has_png_mime_type() {
    let image = CapturedImage::png(vec![1, 2, 3]);
    assert_eq!(image.mime_type, "image/png");
    assert_eq!(image.bytes, vec![1, 2, 3]);
}
