use serde_json::json;
use tau_computer_use::visuals::tau_clicker_svg;
use tau_computer_use::ComputerUseTool;
use tau_core::Tool;
use tau_tools::{PermissionedTool, SandboxMode};
use tokio_util::sync::CancellationToken;

#[test]
fn schema_exposes_core_actions() {
    let schema = ComputerUseTool::default().schema();
    assert_eq!(schema.name, "computer_use");
    assert!(schema.description.contains("macOS"));

    let actions = schema.input_schema["properties"]["action"]["enum"]
        .as_array()
        .unwrap();
    for action in [
        "list_apps",
        "focus_app",
        "get_app_state",
        "click",
        "scroll",
        "drag",
        "move_tau",
        "type_text",
        "paste_text",
        "press_key",
        "set_value",
        "mark_app",
        "show_tau",
    ] {
        assert!(actions.iter().any(|value| value == action));
    }
    assert!(schema.input_schema["properties"]["physical_input"].is_object());
    assert!(schema.input_schema["properties"]["coordinate_space"].is_object());
    assert!(schema.description.contains("visual overlay"));
    assert!(schema
        .description
        .contains("raw coordinate mouse clicks and scrolls are disabled"));
    assert!(schema.description.contains("paste_text"));
    assert!(schema.description.contains("locked window"));
    assert!(schema.description.contains("frontmost"));
    assert!(schema.description.contains("revoked"));
    assert!(schema.description.contains("turn ends"));
}

#[test]
fn tau_clicker_svg_is_transparent_asset() {
    let svg = tau_clicker_svg();
    assert!(svg.starts_with("<svg "));
    assert!(svg.contains("τ"));
    assert!(svg.contains("tau-rust"));
    assert!(svg.contains("tau-glow"));
    assert!(svg.contains("tau-shine"));
    assert!(svg.contains("font-size=\"29\""));
    assert!(!svg.contains("<path"));
    assert!(!svg.contains("<circle"));
    assert!(!svg.contains("background"));
}

#[tokio::test]
async fn rejects_unknown_action_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(json!({"action": "dance"}), CancellationToken::new())
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("unknown computer_use action"));
}

#[tokio::test]
async fn missing_action_returns_tool_error_instead_of_deserialize_error() {
    let result = ComputerUseTool::default()
        .execute(json!({}), CancellationToken::new())
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("action required"));
}

#[tokio::test]
async fn recovers_provider_arg_tag_embedded_in_object_key() {
    let result = ComputerUseTool::default()
        .execute(
            json!({"press_key\n<arg_key>key": "Escape"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(!result.content.contains("action required"));
    assert!(result.content.contains("active focus_app window lock"));
}

#[tokio::test]
async fn show_tau_requires_point_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(json!({"action": "show_tau"}), CancellationToken::new())
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("requires x and y"));
}

#[tokio::test]
async fn focus_app_requires_app_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(json!({"action": "focus_app"}), CancellationToken::new())
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("requires app"));
}

#[tokio::test]
async fn element_index_click_requires_app_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(
            json!({"action": "click", "element_index": 1}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("element_index requires app"));
}

#[tokio::test]
async fn coordinate_click_rejects_bad_coordinate_space_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(
            json!({
                "action": "click",
                "x": 10,
                "y": 10,
                "physical_input": true,
                "coordinate_space": "galaxy"
            }),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result
        .content
        .contains("coordinate_space must be app or screen"));
}

#[tokio::test]
async fn coordinate_click_is_disabled_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(
            json!({"action": "click", "x": 10, "y": 10}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("raw coordinate click is disabled"));
}

#[tokio::test]
async fn app_coordinate_click_is_disabled_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(
            json!({
                "action": "click",
                "app": "Finder",
                "x": 10,
                "y": 10,
                "physical_input": true
            }),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("raw coordinate click is disabled"));
}

#[tokio::test]
async fn screen_coordinate_click_is_disabled_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(
            json!({
                "action": "click",
                "coordinate_space": "screen",
                "physical_input": true,
                "x": 1290,
                "y": 90
            }),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("raw coordinate click is disabled"));
}

#[tokio::test]
async fn scroll_requires_direction_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(
            json!({"action": "scroll", "app": "Finder"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("scroll requires direction"));
}

#[tokio::test]
async fn coordinate_scroll_is_disabled_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(
            json!({"action": "scroll", "direction": "down", "x": 10, "y": 10}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("raw coordinate scroll is disabled"));
}

#[tokio::test]
async fn physical_scroll_is_disabled_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(
            json!({"action": "scroll", "direction": "down", "physical_input": true}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("raw coordinate scroll is disabled"));
}

#[tokio::test]
async fn drag_requires_points_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(
            json!({"action": "drag", "app": "Finder"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("drag requires"));
}

#[tokio::test]
async fn press_key_requires_key_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(
            json!({"action": "press_key", "app": "Finder"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("press_key requires key"));
}

#[tokio::test]
async fn paste_text_requires_text_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(
            json!({"action": "paste_text", "app": "Finder"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("paste_text requires text"));
}

#[tokio::test]
async fn input_requires_focus_app_window_lock_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(
            json!({"action": "type_text", "app": "Finder", "text": "hello"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("call focus_app first"));
}

#[tokio::test]
async fn press_key_rejects_unknown_modifier_before_touching_os() {
    let result = ComputerUseTool::default()
        .execute(
            json!({"action": "press_key", "app": "Finder", "key": "hyper+k"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("unsupported key modifier"));
}

#[test]
fn drag_is_visual_only_not_physical_mouse_drag() {
    let source = include_str!("../src/lib.rs");
    assert!(!source.contains("kCGEventLeftMouseDragged"));
    assert!(!source.contains("kCGEventRightMouseDragged"));
    assert!(!source.contains("kCGEventOtherMouseDragged"));
    assert!(!source.contains("dragBetween"));
    assert!(!source.contains("click at {"));
    assert!(source.contains("visual-only tau movement"));
}

#[test]
fn tau_marker_motion_is_linear_and_precedes_input() {
    let overlay = include_str!("../src/overlay.rs");
    assert!(overlay.contains("moved tau mark linearly"));
    assert!(overlay.contains("marker remains visible until computer_use turn ends"));
    assert!(overlay.contains("SESSION_OVERLAY_SCRIPT"));
    assert!(overlay.contains("command === 'stop'"));
    assert!(overlay.contains("const frameWidth = 48"));
    assert!(overlay.contains("attrs(28"));
    assert!(overlay.contains("$.NSShadowAttributeName"));
    assert!(!overlay.contains("progress * progress"));
    assert!(!overlay.contains("const OVERLAY_SCRIPT"));
    assert!(!overlay.contains("TauComputerUseOverlayView"));

    let source = include_str!("../src/lib.rs");
    assert!(!source.contains("run_pointer_action"));
    assert!(!source.contains("POINTER_SCRIPT"));
    assert!(!source.contains("CGEventCreateMouseEvent"));
    assert!(!source.contains("CGEventCreateScrollWheelEvent"));
    assert!(!source.contains("tokio::join!(marker, pointer)"));
}

#[test]
fn focus_app_shows_tau_marker_at_app_center() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("self.run_marked_focus_app(app).await"));
    assert!(source.contains("\"frontmost_window_identity\".to_string()"));
    assert!(source.contains("focus_app did not lock"));
    assert!(source.contains("\"window_frame\".to_string(), app"));
    assert!(source.contains("window_center_from_tool_result"));
    assert!(source.contains("show_session_tau(point)"));
    assert!(source.contains("self.overlay_session.lock().await.stop().await"));
}

#[test]
fn macos_bridge_prefers_standard_windows_over_auxiliary_popovers() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("on primaryWindow(procRef, appRef)"));
    assert!(source.contains("AXMain"));
    assert!(source.contains("if descText is \"standard window\" then return winRef"));
    assert!(source.contains("set rootElement to my primaryWindow(procRef, appRef)"));
}

#[test]
fn macos_bridge_surfaces_browser_address_bar_value_in_header() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("set urlText to my addressBarValue(rootElement, 0, 8)"));
    assert!(source.contains("headerText & \" url=\" & urlText"));
    assert!(source.contains("descText is \"Address and search bar\""));
}

#[test]
fn focus_app_activates_native_app_before_sending_keys() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("tell application id bundleText to activate"));
    assert!(source.contains("set frontmost of procRef to true"));
    assert!(source.contains("delay 0.2"));
}

#[test]
fn input_actions_are_guarded_by_locked_frontmost_window() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("window_lock"));
    assert!(source.contains("require_input_lock"));
    assert!(source.contains("frontmost_window_identity"));
    assert!(source.contains("on ensureFrontmostWindow(expectedIdentity)"));
    assert!(source.contains("my ensureFrontmostWindow(expectedIdentity)"));
    assert!(!source.contains("on typeText(appRef, textToType)\n  my focusApp(appRef)"));
    assert!(!source.contains("on pasteText(appRef, textToPaste)\n  my focusApp(appRef)"));
    assert!(!source.contains("on pressKey(appRef, keyText, keyCodeText, commandFlag, controlFlag, optionFlag, shiftFlag, displayText)\n  my focusApp(appRef)"));
}

#[test]
fn show_tau_and_mark_app_use_persistent_overlay_session() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("\"show_tau\" =>"));
    assert!(source.contains("self.show_session_tau(point).await"));
    assert!(!source.contains("overlay::show_tau"));
    assert!(!source.contains("OverlayRequest"));
}

#[tokio::test]
async fn blocked_without_yolo() {
    let result = PermissionedTool::new(ComputerUseTool::default(), SandboxMode::ReadOnly)
        .execute(json!({"action": "list_apps"}), CancellationToken::new())
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("sandbox_mode"));
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "requires a live macOS GUI session and System Events access"]
async fn list_apps_smoke() {
    let result = ComputerUseTool::default()
        .execute(json!({"action": "list_apps"}), CancellationToken::new())
        .await
        .unwrap();
    assert!(!result.is_error, "{}", result.content);
    assert!(result.content.lines().next().is_some());
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "requires TAU_COMPUTER_USE_APP plus a live macOS GUI session"]
async fn get_app_state_smoke() {
    let app = std::env::var("TAU_COMPUTER_USE_APP")
        .expect("set TAU_COMPUTER_USE_APP to a running app with a window");
    let result = ComputerUseTool::default()
        .execute(
            json!({"action": "get_app_state", "app": app, "max_depth": 1}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error, "{}", result.content);
    assert!(result.content.contains("App="));
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "requires a live macOS GUI session"]
async fn show_tau_smoke() {
    let tool = ComputerUseTool::default();
    let result = tool
        .execute(
            json!({
                "action": "show_tau",
                "x": 160,
                "y": 120,
                "duration_ms": 250
            }),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error, "{}", result.content);
    assert!(result.content.contains("showing tau mark"));
    tool.cleanup().await.unwrap();
}
