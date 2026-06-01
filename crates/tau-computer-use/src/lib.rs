use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tau_computer_use_layout::{parse_frame_csv, parse_point_csv, ScreenFrame, ScreenPoint};
use tau_core::{Tool, ToolResult};
use tau_llm::ToolSchema;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub mod overlay;
pub mod visuals;

const DEFAULT_TREE_DEPTH: u8 = 6;
const MAX_TREE_DEPTH: u8 = 10;
const MAX_MARK_DURATION_MS: u64 = 30_000;
const DEFAULT_DRAG_DURATION_MS: u64 = 700;
const MAX_SCROLL_PAGES: f64 = 10.0;

#[cfg(target_os = "macos")]
const OUTPUT_LIMIT: usize = 100 * 1024;

#[derive(Debug, Clone)]
pub struct ComputerUseTool {
    overlay_session: Arc<Mutex<overlay::OverlaySession>>,
    window_lock: Arc<Mutex<Option<WindowLock>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowLock {
    app: String,
    identity: String,
}

impl Default for ComputerUseTool {
    fn default() -> Self {
        Self {
            overlay_session: Arc::new(Mutex::new(overlay::OverlaySession::new())),
            window_lock: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl Tool for ComputerUseTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "computer_use".to_string(),
            description: "Inspect and control visible macOS apps through accessibility: list_apps, focus_app, get_app_state, click, scroll, drag, move_tau, type_text, paste_text, press_key, set_value, mark_app, and show_tau. focus_app shows the tau marker, binds computer_use to that focused app window, and keeps the marker visible until the computer-use turn ends. Input actions require that locked window to remain frontmost; if the user changes focus, input is blocked instead of re-focusing. paste_text uses the clipboard with best-effort restore. Tau marker movement is a visual overlay only. Raw coordinate click/scroll requires physical_input=true. When app is supplied, x/y are app-window coordinates unless coordinate_space=screen. This exposes UI structure and explicit input events, not private app APIs or image understanding.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list_apps", "focus_app", "get_app_state", "click", "scroll", "drag", "move_tau", "type_text", "paste_text", "press_key", "set_value", "mark_app", "show_tau"]
                    },
                    "app": {
                        "type": "string",
                        "description": "App process name or bundle identifier, for example Discord or com.hnc.Discord. Required for app inspection, app-relative targets, element_index actions, and all input actions."
                    },
                    "element_index": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Element index from get_app_state."
                    },
                    "x": {
                        "type": "integer",
                        "description": "X coordinate for physical_input click/scroll or show_tau. For click/scroll with app supplied, defaults to app-window-relative coordinates."
                    },
                    "y": {
                        "type": "integer",
                        "description": "Y coordinate for physical_input click/scroll or show_tau. For click/scroll with app supplied, defaults to app-window-relative coordinates."
                    },
                    "coordinate_space": {
                        "type": "string",
                        "enum": ["app", "screen"],
                        "description": "Coordinate space for physical_input click/scroll x/y. Defaults to app when app is supplied, otherwise screen. Screen coordinates with app supplied must be inside that app window."
                    },
                    "from_x": {
                        "type": "integer",
                        "description": "Starting screen x coordinate for visual-only drag/move_tau."
                    },
                    "from_y": {
                        "type": "integer",
                        "description": "Starting screen y coordinate for visual-only drag/move_tau."
                    },
                    "to_x": {
                        "type": "integer",
                        "description": "Ending screen x coordinate for visual-only drag/move_tau."
                    },
                    "to_y": {
                        "type": "integer",
                        "description": "Ending screen y coordinate for visual-only drag/move_tau."
                    },
                    "duration_ms": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 30000,
                        "description": "Tau marker movement duration in milliseconds for drag/move_tau. show_tau and mark_app keep the marker visible until computer_use cleanup."
                    },
                    "click_count": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 10
                    },
                    "button": {
                        "type": "string",
                        "enum": ["left", "right", "middle"],
                        "description": "Mouse button for physical_input coordinate click. Element-index clicks use accessibility."
                    },
                    "physical_input": {
                        "type": "boolean",
                        "description": "Required to send raw coordinate mouse or scroll events. Defaults to false. Raw coordinate input also requires a focus_app window lock. Prefer element_index click and visual-only move_tau/drag."
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["up", "down", "left", "right"],
                        "description": "Scroll direction."
                    },
                    "pages": {
                        "type": "number",
                        "minimum": 0.1,
                        "maximum": 10,
                        "description": "Approximate pages to scroll. Defaults to 1."
                    },
                    "text": {
                        "type": "string",
                        "description": "Text to type or paste into the focused control."
                    },
                    "key": {
                        "type": "string",
                        "description": "Key or key combination to press in the focused app, for example command+k, cmd+f, Escape, Return, Tab, Up, Down, Left, or Right."
                    },
                    "value": {
                        "type": "string",
                        "description": "Value to assign with set_value."
                    },
                    "max_depth": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 10,
                        "description": "Maximum accessibility-tree depth for get_app_state. Defaults to 6."
                    }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(&self, input: Value, _: CancellationToken) -> anyhow::Result<ToolResult> {
        let mut args: ComputerUseArgs = serde_json::from_value(input)?;
        args.normalize_provider_arg_tags();
        if args.action.is_empty() {
            return Ok(tool_error("computer_use action required"));
        }
        match args.action.as_str() {
            "list_apps" => run_action(vec!["list_apps".to_string()]).await,
            "focus_app" => {
                let app = match args.require_app() {
                    Ok(app) => app,
                    Err(result) => return Ok(result),
                };
                self.run_marked_focus_app(app).await
            }
            "get_app_state" => {
                let app = match args.require_app() {
                    Ok(app) => app,
                    Err(result) => return Ok(result),
                };
                let max_depth = args
                    .max_depth
                    .unwrap_or(DEFAULT_TREE_DEPTH)
                    .min(MAX_TREE_DEPTH);
                run_action(vec![
                    "get_app_state".to_string(),
                    app,
                    max_depth.to_string(),
                ])
                .await
            }
            "click" => {
                let app = args.optional_app();
                let click_count = args.click_count.unwrap_or(1).clamp(1, 10);
                let button = match args.pointer_button() {
                    Ok(button) => button,
                    Err(result) => return Ok(result),
                };
                if let Some(element_index) = args.element_index {
                    let Some(app) = app else {
                        return Ok(tool_error("click with element_index requires app"));
                    };
                    let expected_identity = match self.require_input_lock(&app).await? {
                        Ok(identity) => identity,
                        Err(result) => return Ok(result),
                    };
                    let point = match args.target_point(Some(&app), false, "click").await? {
                        Ok(point) => point,
                        Err(result) => return Ok(result),
                    };
                    return self
                        .run_marked_ax_action(
                            point,
                            vec![
                                "click_element".to_string(),
                                app,
                                element_index.to_string(),
                                click_count.to_string(),
                                expected_identity,
                            ],
                        )
                        .await;
                }
                if !args.has_coordinates() {
                    return Ok(tool_error(
                        "click requires either element_index or both x and y",
                    ));
                }
                if !args.physical_input {
                    return Ok(tool_error(
                        "coordinate click sends real mouse events; set physical_input=true, or prefer element_index or focus_app",
                    ));
                }
                if let Err(result) = args.coordinate_space(app.is_some()) {
                    return Ok(result);
                }
                let Some(app) = app else {
                    return Ok(tool_error(
                        "coordinate click requires app and an active focus_app window lock",
                    ));
                };
                let _expected_identity = match self.require_input_lock(&app).await? {
                    Ok(identity) => identity,
                    Err(result) => return Ok(result),
                };
                let point = match args.target_point(Some(&app), false, "click").await? {
                    Ok(point) => point,
                    Err(result) => return Ok(result),
                };
                self.run_marked_pointer(
                    point,
                    vec![
                        "click".to_string(),
                        point.x.to_string(),
                        point.y.to_string(),
                        click_count.to_string(),
                        button,
                    ],
                )
                .await
            }
            "scroll" => {
                let app = args.optional_app();
                let direction = match args.scroll_direction() {
                    Ok(direction) => direction,
                    Err(result) => return Ok(result),
                };
                if !args.physical_input {
                    return Ok(tool_error(
                        "scroll sends real scroll events; set physical_input=true after choosing a safe target",
                    ));
                }
                let Some(app) = app else {
                    return Ok(tool_error(
                        "scroll requires app and an active focus_app window lock",
                    ));
                };
                let _expected_identity = match self.require_input_lock(&app).await? {
                    Ok(identity) => identity,
                    Err(result) => return Ok(result),
                };
                let point = match args.target_point(Some(&app), true, "scroll").await? {
                    Ok(point) => point,
                    Err(result) => return Ok(result),
                };
                self.run_marked_pointer(
                    point,
                    vec![
                        "scroll".to_string(),
                        point.x.to_string(),
                        point.y.to_string(),
                        direction,
                        args.scroll_pages().to_string(),
                    ],
                )
                .await
            }
            "drag" | "move_tau" => {
                let (from, to) = match args.drag_points() {
                    Ok(points) => points,
                    Err(result) => return Ok(result),
                };
                let result = self
                    .move_session_tau(overlay::OverlayMoveRequest {
                        from,
                        to,
                        duration_ms: args.drag_duration_ms(),
                    })
                    .await?;
                if result.is_error {
                    Ok(result)
                } else {
                    Ok(ToolResult {
                        content: format!(
                            "{}\nvisual-only tau movement; no mouse drag was sent",
                            result.content.trim()
                        ),
                        is_error: false,
                    })
                }
            }
            "type_text" => {
                let app = match args.require_app() {
                    Ok(app) => app,
                    Err(result) => return Ok(result),
                };
                let Some(text) = args.text else {
                    return Ok(tool_error("type_text requires text"));
                };
                let expected_identity = match self.require_input_lock(&app).await? {
                    Ok(identity) => identity,
                    Err(result) => return Ok(result),
                };
                run_action(vec!["type_text".to_string(), app, text, expected_identity]).await
            }
            "paste_text" => {
                let app = match args.require_app() {
                    Ok(app) => app,
                    Err(result) => return Ok(result),
                };
                let Some(text) = args.text else {
                    return Ok(tool_error("paste_text requires text"));
                };
                let expected_identity = match self.require_input_lock(&app).await? {
                    Ok(identity) => identity,
                    Err(result) => return Ok(result),
                };
                run_action(vec!["paste_text".to_string(), app, text, expected_identity]).await
            }
            "press_key" => {
                let app = match args.require_app() {
                    Ok(app) => app,
                    Err(result) => return Ok(result),
                };
                let Some(key) = args.key.as_deref() else {
                    return Ok(tool_error("press_key requires key"));
                };
                let key = match parse_key_spec(key) {
                    Ok(key) => key,
                    Err(result) => return Ok(result),
                };
                let expected_identity = match self.require_input_lock(&app).await? {
                    Ok(identity) => identity,
                    Err(result) => return Ok(result),
                };
                run_action(vec![
                    "press_key".to_string(),
                    app,
                    key.key_text,
                    key.key_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(String::new),
                    bool_flag(key.command),
                    bool_flag(key.control),
                    bool_flag(key.option),
                    bool_flag(key.shift),
                    key.display,
                    expected_identity,
                ])
                .await
            }
            "set_value" => {
                let app = match args.require_app() {
                    Ok(app) => app,
                    Err(result) => return Ok(result),
                };
                let Some(element_index) = args.element_index else {
                    return Ok(tool_error("set_value requires element_index"));
                };
                let Some(value) = args.value else {
                    return Ok(tool_error("set_value requires value"));
                };
                let expected_identity = match self.require_input_lock(&app).await? {
                    Ok(identity) => identity,
                    Err(result) => return Ok(result),
                };
                run_action(vec![
                    "set_value".to_string(),
                    app,
                    element_index.to_string(),
                    value,
                    expected_identity,
                ])
                .await
            }
            "mark_app" => {
                let app = match args.require_app() {
                    Ok(app) => app,
                    Err(result) => return Ok(result),
                };
                let frame = run_action(vec!["window_frame".to_string(), app.clone()]).await?;
                if frame.is_error {
                    return Ok(frame);
                }
                let point = match parse_frame_top_left(&frame.content) {
                    Ok(point) => point,
                    Err(result) => return Ok(result),
                };
                self.show_session_tau(point).await
            }
            "show_tau" => {
                let point = match args.require_tau_point() {
                    Ok(point) => point,
                    Err(result) => return Ok(result),
                };
                self.show_session_tau(point).await
            }
            action => Ok(tool_error(&format!(
                "unknown computer_use action: {action}"
            ))),
        }
    }

    async fn cleanup(&self) -> anyhow::Result<()> {
        *self.window_lock.lock().await = None;
        self.overlay_session.lock().await.stop().await
    }
}

#[derive(Deserialize)]
struct ComputerUseArgs {
    #[serde(default)]
    action: String,
    app: Option<String>,
    element_index: Option<u32>,
    x: Option<i64>,
    y: Option<i64>,
    coordinate_space: Option<String>,
    from_x: Option<i64>,
    from_y: Option<i64>,
    to_x: Option<i64>,
    to_y: Option<i64>,
    duration_ms: Option<u64>,
    click_count: Option<u8>,
    button: Option<String>,
    #[serde(default)]
    physical_input: bool,
    direction: Option<String>,
    pages: Option<f64>,
    text: Option<String>,
    key: Option<String>,
    value: Option<String>,
    max_depth: Option<u8>,
}

impl ComputerUseArgs {
    fn optional_app(&self) -> Option<String> {
        self.app
            .as_ref()
            .map(|app| app.trim())
            .filter(|app| !app.is_empty())
            .map(ToOwned::to_owned)
    }

    fn normalize_provider_arg_tags(&mut self) {
        if !self.action.contains("<arg_key>") {
            self.action = self.action.trim().to_string();
            return;
        }
        let raw = self.action.clone();
        if let Some(action) = raw.split("<arg_key>").next() {
            self.action = action.trim().to_string();
        }
        for (key, value) in tagged_args(&raw) {
            match key.as_str() {
                "app" if self.app.is_none() => self.app = Some(value),
                "key" if self.key.is_none() => self.key = Some(value),
                "text" if self.text.is_none() => self.text = Some(value),
                "value" if self.value.is_none() => self.value = Some(value),
                "coordinate_space" if self.coordinate_space.is_none() => {
                    self.coordinate_space = Some(value)
                }
                "direction" if self.direction.is_none() => self.direction = Some(value),
                "element_index" if self.element_index.is_none() => {
                    self.element_index = value.parse().ok()
                }
                "x" if self.x.is_none() => self.x = value.parse().ok(),
                "y" if self.y.is_none() => self.y = value.parse().ok(),
                "from_x" if self.from_x.is_none() => self.from_x = value.parse().ok(),
                "from_y" if self.from_y.is_none() => self.from_y = value.parse().ok(),
                "to_x" if self.to_x.is_none() => self.to_x = value.parse().ok(),
                "to_y" if self.to_y.is_none() => self.to_y = value.parse().ok(),
                "duration_ms" if self.duration_ms.is_none() => {
                    self.duration_ms = value.parse().ok()
                }
                "click_count" if self.click_count.is_none() => {
                    self.click_count = value.parse().ok()
                }
                "button" if self.button.is_none() => self.button = Some(value),
                "physical_input" if !self.physical_input => {
                    self.physical_input = matches!(value.as_str(), "true" | "1")
                }
                "pages" if self.pages.is_none() => self.pages = value.parse().ok(),
                "max_depth" if self.max_depth.is_none() => self.max_depth = value.parse().ok(),
                _ => {}
            }
        }
    }

    fn require_app(&self) -> anyhow::Result<String, ToolResult> {
        self.optional_app()
            .ok_or_else(|| tool_error("computer_use action requires app"))
    }

    fn require_tau_point(&self) -> anyhow::Result<overlay::OverlayPoint, ToolResult> {
        let (Some(x), Some(y)) = (self.x, self.y) else {
            return Err(tool_error("show_tau requires x and y"));
        };
        Ok(overlay::OverlayPoint::new(x, y))
    }

    fn has_coordinates(&self) -> bool {
        self.x.is_some() && self.y.is_some()
    }

    async fn target_point(
        &self,
        app: Option<&str>,
        default_to_window_center: bool,
        action: &str,
    ) -> anyhow::Result<Result<overlay::OverlayPoint, ToolResult>> {
        if let Some(element_index) = self.element_index {
            let Some(app) = app else {
                return Ok(Err(tool_error(&format!(
                    "{action} with element_index requires app"
                ))));
            };
            let result = run_action(vec![
                "element_center".to_string(),
                app.to_string(),
                element_index.to_string(),
            ])
            .await?;
            return Ok(point_from_tool_result(result));
        }
        if let (Some(x), Some(y)) = (self.x, self.y) {
            let point = ScreenPoint::new(x, y);
            return match self.resolve_coordinate_point(app, point).await? {
                Ok(point) => Ok(Ok(point)),
                Err(result) => Ok(Err(result)),
            };
        }
        if default_to_window_center {
            let Some(app) = app else {
                return Ok(Err(tool_error(&format!(
                    "{action} without coordinates requires app"
                ))));
            };
            let frame = run_action(vec!["window_frame".to_string(), app.to_string()]).await?;
            return Ok(window_center_from_tool_result(frame));
        }
        Ok(Err(tool_error(&format!(
            "{action} requires either element_index or both x and y"
        ))))
    }

    fn drag_duration_ms(&self) -> u64 {
        self.duration_ms
            .unwrap_or(DEFAULT_DRAG_DURATION_MS)
            .clamp(1, MAX_MARK_DURATION_MS)
    }

    fn pointer_button(&self) -> anyhow::Result<String, ToolResult> {
        let button = self.button.as_deref().unwrap_or("left");
        match button {
            "left" | "right" | "middle" => Ok(button.to_string()),
            _ => Err(tool_error("click button must be left, right, or middle")),
        }
    }

    fn scroll_direction(&self) -> anyhow::Result<String, ToolResult> {
        let Some(direction) = self.direction.as_deref() else {
            return Err(tool_error("scroll requires direction"));
        };
        match direction {
            "up" | "down" | "left" | "right" => Ok(direction.to_string()),
            _ => Err(tool_error(
                "scroll direction must be up, down, left, or right",
            )),
        }
    }

    fn scroll_pages(&self) -> f64 {
        self.pages.unwrap_or(1.0).clamp(0.1, MAX_SCROLL_PAGES)
    }

    fn coordinate_space(&self, has_app: bool) -> anyhow::Result<CoordinateSpace, ToolResult> {
        match self.coordinate_space.as_deref() {
            Some("app") => Ok(CoordinateSpace::App),
            Some("screen") => Ok(CoordinateSpace::Screen),
            Some(_) => Err(tool_error("coordinate_space must be app or screen")),
            None if has_app => Ok(CoordinateSpace::App),
            None => Ok(CoordinateSpace::Screen),
        }
    }

    async fn resolve_coordinate_point(
        &self,
        app: Option<&str>,
        point: ScreenPoint,
    ) -> anyhow::Result<Result<ScreenPoint, ToolResult>> {
        let space = match self.coordinate_space(app.is_some()) {
            Ok(space) => space,
            Err(result) => return Ok(Err(result)),
        };
        let Some(app) = app else {
            return Ok(match space {
                CoordinateSpace::Screen => Ok(point),
                CoordinateSpace::App => Err(tool_error("coordinate_space=app requires app")),
            });
        };
        let frame = run_action(vec!["window_frame".to_string(), app.to_string()]).await?;
        if frame.is_error {
            return Ok(Err(frame));
        }
        let frame = match parse_frame(&frame.content) {
            Ok(frame) => frame,
            Err(result) => return Ok(Err(result)),
        };
        Ok(resolve_point_in_frame(frame, point, space))
    }

    fn drag_points(
        &self,
    ) -> anyhow::Result<(overlay::OverlayPoint, overlay::OverlayPoint), ToolResult> {
        let (Some(from_x), Some(from_y), Some(to_x), Some(to_y)) =
            (self.from_x, self.from_y, self.to_x, self.to_y)
        else {
            return Err(tool_error("drag requires from_x, from_y, to_x, and to_y"));
        };
        Ok((
            overlay::OverlayPoint::new(from_x, from_y),
            overlay::OverlayPoint::new(to_x, to_y),
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoordinateSpace {
    App,
    Screen,
}

fn tool_error(message: &str) -> ToolResult {
    ToolResult {
        content: message.to_string(),
        is_error: true,
    }
}

fn parse_frame_top_left(content: &str) -> anyhow::Result<overlay::OverlayPoint, ToolResult> {
    Ok(parse_frame(content)?.top_left())
}

fn parse_frame(content: &str) -> anyhow::Result<ScreenFrame, ToolResult> {
    parse_frame_csv(content).map_err(|err| tool_error(&err.to_string()))
}

fn parse_point(content: &str) -> anyhow::Result<overlay::OverlayPoint, ToolResult> {
    parse_point_csv(content).map_err(|err| tool_error(&err.to_string()))
}

fn resolve_point_in_frame(
    frame: ScreenFrame,
    point: ScreenPoint,
    space: CoordinateSpace,
) -> Result<ScreenPoint, ToolResult> {
    match space {
        CoordinateSpace::App => {
            if point.x < 0 || point.y < 0 || point.x >= frame.width || point.y >= frame.height {
                return Err(tool_error(&format!(
                    "app-relative coordinate ({},{}) is outside app window frame ({},{},{},{})",
                    point.x, point.y, frame.x, frame.y, frame.width, frame.height
                )));
            }
            Ok(ScreenPoint::new(frame.x + point.x, frame.y + point.y))
        }
        CoordinateSpace::Screen => {
            if !frame.contains(point) {
                return Err(tool_error(&format!(
                    "screen coordinate ({},{}) is outside app window frame ({},{},{},{}); use app-relative x/y or omit app",
                    point.x, point.y, frame.x, frame.y, frame.width, frame.height
                )));
            }
            Ok(point)
        }
    }
}

#[derive(Debug)]
struct KeySpec {
    key_text: String,
    key_code: Option<u16>,
    command: bool,
    control: bool,
    option: bool,
    shift: bool,
    display: String,
}

fn parse_key_spec(raw: &str) -> anyhow::Result<KeySpec, ToolResult> {
    let parts = raw
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let Some((base, modifiers)) = parts.split_last() else {
        return Err(tool_error("press_key requires key"));
    };

    let mut key = KeySpec {
        key_text: String::new(),
        key_code: None,
        command: false,
        control: false,
        option: false,
        shift: false,
        display: raw.trim().to_string(),
    };

    for modifier in modifiers {
        match modifier.to_ascii_lowercase().as_str() {
            "cmd" | "command" | "super" | "meta" => key.command = true,
            "ctrl" | "control" => key.control = true,
            "alt" | "option" => key.option = true,
            "shift" => key.shift = true,
            unknown => return Err(tool_error(&format!("unsupported key modifier: {unknown}"))),
        }
    }

    let normalized = base.to_ascii_lowercase();
    match normalized.as_str() {
        "return" | "enter" => {
            key.key_text = "Return".to_string();
            key.key_code = Some(36);
        }
        "tab" => {
            key.key_text = "Tab".to_string();
            key.key_code = Some(48);
        }
        "escape" | "esc" => {
            key.key_text = "Escape".to_string();
            key.key_code = Some(53);
        }
        "delete" | "backspace" => {
            key.key_text = "Delete".to_string();
            key.key_code = Some(51);
        }
        "forwarddelete" | "forward-delete" => {
            key.key_text = "ForwardDelete".to_string();
            key.key_code = Some(117);
        }
        "space" => key.key_text = " ".to_string(),
        "up" => {
            key.key_text = "Up".to_string();
            key.key_code = Some(126);
        }
        "down" => {
            key.key_text = "Down".to_string();
            key.key_code = Some(125);
        }
        "left" => {
            key.key_text = "Left".to_string();
            key.key_code = Some(123);
        }
        "right" => {
            key.key_text = "Right".to_string();
            key.key_code = Some(124);
        }
        "home" => {
            key.key_text = "Home".to_string();
            key.key_code = Some(115);
        }
        "end" => {
            key.key_text = "End".to_string();
            key.key_code = Some(119);
        }
        "pageup" | "page-up" => {
            key.key_text = "PageUp".to_string();
            key.key_code = Some(116);
        }
        "pagedown" | "page-down" => {
            key.key_text = "PageDown".to_string();
            key.key_code = Some(121);
        }
        _ if base.chars().count() == 1 => key.key_text = base.to_string(),
        _ => return Err(tool_error(&format!("unsupported key: {}", base.trim()))),
    }

    Ok(key)
}

fn bool_flag(value: bool) -> String {
    if value { "1" } else { "0" }.to_string()
}

fn tagged_args(raw: &str) -> Vec<(String, String)> {
    let mut args = Vec::new();
    let mut rest = raw;
    while let Some(key_start) = rest.find("<arg_key>") {
        rest = &rest[(key_start + "<arg_key>".len())..];
        let Some(key_end) = rest.find("</arg_key>") else {
            break;
        };
        let key = rest[..key_end].trim().to_string();
        rest = &rest[(key_end + "</arg_key>".len())..];
        let Some(value_start) = rest.find("<arg_value>") else {
            continue;
        };
        rest = &rest[(value_start + "<arg_value>".len())..];
        let (value, next_rest) = if let Some(value_end) = rest.find("</arg_value>") {
            (
                rest[..value_end].trim().to_string(),
                &rest[(value_end + "</arg_value>".len())..],
            )
        } else if let Some(next_key) = rest.find("<arg_key>") {
            (rest[..next_key].trim().to_string(), &rest[next_key..])
        } else {
            (rest.trim().to_string(), "")
        };
        rest = next_rest;
        args.push((key, value));
    }
    args
}

fn point_from_tool_result(result: ToolResult) -> Result<overlay::OverlayPoint, ToolResult> {
    if result.is_error {
        return Err(result);
    }
    parse_point(&result.content)
}

fn window_center_from_tool_result(result: ToolResult) -> Result<overlay::OverlayPoint, ToolResult> {
    if result.is_error {
        return Err(result);
    }
    parse_frame(&result.content).map(|frame| frame.center())
}

impl ComputerUseTool {
    async fn run_marked_focus_app(&self, app: String) -> anyhow::Result<ToolResult> {
        let focus_result = run_action(vec!["focus_app".to_string(), app.clone()]).await?;
        if focus_result.is_error {
            return Ok(focus_result);
        }
        let target_identity = run_action(vec!["window_identity".to_string(), app.clone()]).await?;
        if target_identity.is_error {
            return Ok(target_identity);
        }
        let target_identity = target_identity.content.trim().to_string();
        let frontmost_identity = run_action(vec!["frontmost_window_identity".to_string()]).await?;
        if frontmost_identity.is_error {
            return Ok(frontmost_identity);
        }
        let frontmost_identity = frontmost_identity.content.trim().to_string();
        if frontmost_identity != target_identity {
            *self.window_lock.lock().await = None;
            return Ok(tool_error(&format!(
                "focus_app did not lock {app}: frontmost window changed to {frontmost_identity}"
            )));
        }
        let identity = target_identity;
        *self.window_lock.lock().await = Some(WindowLock {
            app: app.clone(),
            identity: identity.clone(),
        });
        let frame_result = run_action(vec!["window_frame".to_string(), app]).await?;
        let point = match window_center_from_tool_result(frame_result) {
            Ok(point) => point,
            Err(result) => return Ok(result),
        };
        let marker_result = self.show_session_tau(point).await?;
        if marker_result.is_error {
            return Ok(marker_result);
        }
        Ok(ToolResult {
            content: format!(
                "{}\nlocked computer_use to frontmost window: {}\n{}",
                focus_result.content.trim(),
                identity,
                marker_result.content.trim()
            ),
            is_error: false,
        })
    }

    async fn require_input_lock(&self, app: &str) -> anyhow::Result<Result<String, ToolResult>> {
        let Some(lock) = self.window_lock.lock().await.clone() else {
            return Ok(Err(tool_error(
                "computer_use input blocked: call focus_app first to lock a target window",
            )));
        };

        let target = run_action(vec!["window_identity".to_string(), app.to_string()]).await?;
        if target.is_error {
            return Ok(Err(target));
        }
        let target_identity = target.content.trim();
        if target_identity != lock.identity {
            return Ok(Err(tool_error(&format!(
                "computer_use input blocked: app/window changed from locked target {} ({}) to {} ({})",
                lock.app, lock.identity, app, target_identity
            ))));
        }

        let frontmost = run_action(vec!["frontmost_window_identity".to_string()]).await?;
        if frontmost.is_error {
            return Ok(Err(frontmost));
        }
        let frontmost_identity = frontmost.content.trim();
        if frontmost_identity != lock.identity {
            return Ok(Err(tool_error(&format!(
                "computer_use input blocked: frontmost window changed; locked target is {} ({}) but frontmost is {}",
                lock.app, lock.identity, frontmost_identity
            ))));
        }

        Ok(Ok(lock.identity))
    }

    async fn run_marked_pointer(
        &self,
        point: overlay::OverlayPoint,
        pointer_args: Vec<String>,
    ) -> anyhow::Result<ToolResult> {
        let marker_result = self.show_session_tau(point).await?;
        if marker_result.is_error {
            return Ok(marker_result);
        }
        let pointer_result = run_pointer_action(pointer_args).await?;
        if pointer_result.is_error {
            return Ok(pointer_result);
        }
        Ok(join_tool_results(marker_result, pointer_result))
    }

    async fn run_marked_ax_action(
        &self,
        point: overlay::OverlayPoint,
        action_args: Vec<String>,
    ) -> anyhow::Result<ToolResult> {
        let marker_result = self.show_session_tau(point).await?;
        if marker_result.is_error {
            return Ok(marker_result);
        }
        let action_result = run_action(action_args).await?;
        if action_result.is_error {
            return Ok(action_result);
        }
        Ok(join_tool_results(marker_result, action_result))
    }

    async fn show_session_tau(&self, point: overlay::OverlayPoint) -> anyhow::Result<ToolResult> {
        self.overlay_session.lock().await.show(point).await
    }

    async fn move_session_tau(
        &self,
        request: overlay::OverlayMoveRequest,
    ) -> anyhow::Result<ToolResult> {
        self.overlay_session.lock().await.move_tau(request).await
    }
}

fn join_tool_results(first: ToolResult, second: ToolResult) -> ToolResult {
    ToolResult {
        content: format!("{}\n{}", first.content.trim(), second.content.trim()),
        is_error: false,
    }
}

#[cfg(not(target_os = "macos"))]
async fn run_action(_: Vec<String>) -> anyhow::Result<ToolResult> {
    Ok(tool_error(
        "computer_use is only supported on macOS accessibility",
    ))
}

#[cfg(target_os = "macos")]
async fn run_action(args: Vec<String>) -> anyhow::Result<ToolResult> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    let mut child = Command::new("osascript")
        .arg("-")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("stdin piped");
    stdin.write_all(COMPUTER_USE_SCRIPT.as_bytes()).await?;
    drop(stdin);

    let output = child.wait_with_output().await?;
    let is_error = !output.status.success();
    let content = output_text(output.stdout, output.stderr, is_error);

    Ok(ToolResult { content, is_error })
}

#[cfg(not(target_os = "macos"))]
async fn run_pointer_action(_: Vec<String>) -> anyhow::Result<ToolResult> {
    Ok(tool_error(
        "computer_use pointer actions are only supported on macOS",
    ))
}

#[cfg(target_os = "macos")]
async fn run_pointer_action(args: Vec<String>) -> anyhow::Result<ToolResult> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    let mut child = Command::new("osascript")
        .arg("-l")
        .arg("JavaScript")
        .arg("-")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("stdin piped");
    stdin.write_all(POINTER_SCRIPT.as_bytes()).await?;
    drop(stdin);

    let output = child.wait_with_output().await?;
    let is_error = !output.status.success();
    let content = output_text(output.stdout, output.stderr, is_error);

    Ok(ToolResult { content, is_error })
}

#[cfg(target_os = "macos")]
fn output_text(stdout: Vec<u8>, stderr: Vec<u8>, is_error: bool) -> String {
    let mut output = if is_error && !stderr.is_empty() {
        let mut bytes = stderr;
        if !stdout.is_empty() {
            bytes.extend_from_slice(b"\n");
            bytes.extend_from_slice(&stdout);
        }
        bytes
    } else {
        stdout
    };

    if output.is_empty() {
        return if is_error {
            "computer_use failed".to_string()
        } else {
            "ok".to_string()
        };
    }

    let total = output.len();
    output.truncate(output.len().min(OUTPUT_LIMIT));
    let mut content = String::from_utf8_lossy(&output).to_string();
    if total > OUTPUT_LIMIT {
        content.push_str(&format!("\n[output truncated, {total} bytes total]"));
    }
    if is_error {
        format!("computer_use failed: {}", content.trim())
    } else {
        content
    }
}

#[cfg(target_os = "macos")]
const COMPUTER_USE_SCRIPT: &str = r#"
on run argv
  set actionName to item 1 of argv
  if actionName is "list_apps" then return my listApps()
  if actionName is "get_app_state" then return my getAppState(item 2 of argv, item 3 of argv as integer)
  if actionName is "focus_app" then return my focusOnly(item 2 of argv)
  if actionName is "element_center" then return my elementCenter(item 2 of argv, item 3 of argv as integer)
  if actionName is "click_element" then return my clickElement(item 2 of argv, item 3 of argv as integer, item 4 of argv as integer, item 5 of argv)
  if actionName is "type_text" then return my typeText(item 2 of argv, item 3 of argv, item 4 of argv)
  if actionName is "paste_text" then return my pasteText(item 2 of argv, item 3 of argv, item 4 of argv)
  if actionName is "press_key" then return my pressKey(item 2 of argv, item 3 of argv, item 4 of argv, item 5 of argv, item 6 of argv, item 7 of argv, item 8 of argv, item 9 of argv, item 10 of argv)
  if actionName is "set_value" then return my setValue(item 2 of argv, item 3 of argv as integer, item 4 of argv, item 5 of argv)
  if actionName is "window_frame" then return my windowFrame(item 2 of argv)
  if actionName is "window_identity" then return my windowIdentity(item 2 of argv)
  if actionName is "frontmost_window_identity" then return my frontmostWindowIdentity()
  error "unknown action: " & actionName
end run

on cleanText(rawValue)
  try
    if rawValue is missing value then return ""
    set textValue to rawValue as text
    if textValue is "missing value" then return ""
    return textValue
  on error
    return ""
  end try
end cleanText

on csvPoint(xCoord, yCoord)
  return ((xCoord as integer) as text) & "," & ((yCoord as integer) as text)
end csvPoint

on csvFrame(xCoord, yCoord, widthValue, heightValue)
  return ((xCoord as integer) as text) & "," & ((yCoord as integer) as text) & "," & ((widthValue as integer) as text) & "," & ((heightValue as integer) as text)
end csvFrame

on appProcess(appRef)
  tell application "System Events"
    if exists process appRef then return process appRef
    try
      return first process whose bundle identifier is appRef
    on error
      error "app is not running: " & appRef
    end try
  end tell
end appProcess

on focusApp(appRef)
  set appNameText to ""
  set bundleText to ""
  tell application "System Events"
    set procRef to my appProcess(appRef)
    try
      set appNameText to my cleanText(name of procRef)
    end try
    try
      set bundleText to my cleanText(bundle identifier of procRef)
    end try
  end tell
  if bundleText is not "" then
    try
      tell application id bundleText to activate
    end try
  else if appNameText is not "" then
    try
      tell application appNameText to activate
    end try
  end if
  tell application "System Events"
    set frontmost of procRef to true
  end tell
  delay 0.2
  return procRef
end focusApp

on focusOnly(appRef)
  my focusApp(appRef)
  return "focused " & appRef
end focusOnly

on listApps()
  set outText to ""
  tell application "System Events"
    repeat with procRef in (processes whose background only is false)
      set titleText to my cleanText(name of procRef)
      set bundleText to ""
      set frontText to ""
      try
        set bundleText to my cleanText(bundle identifier of procRef)
      end try
      try
        if frontmost of procRef then set frontText to " frontmost"
      end try
      set outText to outText & titleText
      if bundleText is not "" then set outText to outText & " (" & bundleText & ")"
      set outText to outText & frontText & linefeed
    end repeat
  end tell
  return outText
end listApps

on getAppState(appRef, maxDepth)
  set procRef to my appProcess(appRef)
  set rootElement to my primaryWindow(procRef, appRef)
  tell application "System Events"
    set appName to my cleanText(name of procRef)
    set bundleText to ""
    set pidText to ""
    try
      set bundleText to my cleanText(bundle identifier of procRef)
    end try
    try
      set pidText to unix id of procRef as text
    end try
  end tell
  set resultTuple to my describeElement(rootElement, 0, maxDepth, 0)
  set headerText to "App=" & appName
  if bundleText is not "" then set headerText to headerText & " bundle_id=" & bundleText
  if pidText is not "" then set headerText to headerText & " pid=" & pidText
  set urlText to my addressBarValue(rootElement, 0, 8)
  if urlText is not "" then set headerText to headerText & " url=" & urlText
  return headerText & linefeed & item 1 of resultTuple
end getAppState

on windowFrame(appRef)
  set procRef to my appProcess(appRef)
  tell application "System Events"
    set rootElement to my primaryWindow(procRef, appRef)
    set p to position of rootElement
    set s to size of rootElement
    return my csvFrame(item 1 of p, item 2 of p, item 1 of s, item 2 of s)
  end tell
end windowFrame

on windowIdentity(appRef)
  set procRef to my appProcess(appRef)
  set rootElement to my primaryWindow(procRef, appRef)
  return my identityForWindow(procRef, rootElement)
end windowIdentity

on frontmostWindowIdentity()
  tell application "System Events"
    try
      set procRef to first process whose frontmost is true
    on error
      error "no frontmost app"
    end try
  end tell
  set rootElement to my primaryWindow(procRef, "frontmost app")
  return my identityForWindow(procRef, rootElement)
end frontmostWindowIdentity

on identityForWindow(procRef, rootElement)
  tell application "System Events"
    set bundleText to ""
    set pidText to ""
    try
      set bundleText to my cleanText(bundle identifier of procRef)
    end try
    try
      set pidText to unix id of procRef as text
    end try
    set p to position of rootElement
    set s to size of rootElement
    return bundleText & "|" & pidText & "|" & my csvFrame(item 1 of p, item 2 of p, item 1 of s, item 2 of s)
  end tell
end identityForWindow

on ensureFrontmostWindow(expectedIdentity)
  set currentIdentity to my frontmostWindowIdentity()
  if currentIdentity is not expectedIdentity then error "input blocked: frontmost window changed; expected " & expectedIdentity & " but got " & currentIdentity
end ensureFrontmostWindow

on primaryWindow(procRef, appRef)
  tell application "System Events"
    if not (exists window 1 of procRef) then error "app has no windows: " & appRef
    repeat with winRef in (windows of procRef)
      try
        if (value of attribute "AXMain" of winRef) is true then return winRef
      end try
    end repeat
    repeat with winRef in (windows of procRef)
      set descText to ""
      try
        set descText to my cleanText(description of winRef)
      end try
      if descText is "standard window" then return winRef
    end repeat
    return window 1 of procRef
  end tell
end primaryWindow

on addressBarValue(elementRef, depth, maxDepth)
  if depth > maxDepth then return ""

  set descText to ""
  set valueText to ""
  tell application "System Events"
    try
      set descText to my cleanText(description of elementRef)
    end try
    try
      set valueText to my cleanText(value of elementRef)
    end try
  end tell

  if descText is "Address and search bar" and valueText is not "" then return valueText

  tell application "System Events"
    try
      set elementChildren to UI elements of elementRef
    on error
      return ""
    end try
  end tell
  repeat with childRef in elementChildren
    set foundText to my addressBarValue(childRef, depth + 1, maxDepth)
    if foundText is not "" then return foundText
  end repeat
  return ""
end addressBarValue

on describeElement(elementRef, depth, maxDepth, idx)
  set i to idx + 1
  set indentText to ""
  repeat depth times
    set indentText to indentText & "  "
  end repeat

  set roleText to ""
  set titleText to ""
  set descText to ""
  set valueText to ""
  set frameText to ""

  tell application "System Events"
    try
      set roleText to my cleanText(role of elementRef)
    end try
    try
      set titleText to my cleanText(name of elementRef)
    end try
    try
      set descText to my cleanText(description of elementRef)
    end try
    try
      set valueText to my cleanText(value of elementRef)
    end try
    try
      set p to position of elementRef
      set s to size of elementRef
      set frameText to " frame=(" & (item 1 of p as integer) & "," & (item 2 of p as integer) & "," & (item 1 of s as integer) & "," & (item 2 of s as integer) & ")"
    end try
  end tell

  set lineText to indentText & i & " " & roleText
  if titleText is not "" then set lineText to lineText & " name=" & (quoted form of titleText)
  if descText is not "" then set lineText to lineText & " description=" & (quoted form of descText)
  if valueText is not "" then set lineText to lineText & " value=" & (quoted form of valueText)
  set lineText to lineText & frameText & linefeed

  set outText to lineText
  if depth < maxDepth then
    try
      tell application "System Events"
        set elementChildren to UI elements of elementRef
      end tell
      repeat with childRef in elementChildren
        set childResult to my describeElement(childRef, (depth + 1), maxDepth, i)
        set outText to outText & item 1 of childResult
        set i to item 2 of childResult
      end repeat
    end try
  end if
  return {outText, i}
end describeElement

on elementCenter(appRef, targetIndex)
  set procRef to my appProcess(appRef)
  set rootElement to my primaryWindow(procRef, appRef)
  set foundResult to my centerForIndex(rootElement, targetIndex, 0)
  if item 1 of foundResult is false then error "element_index not found: " & targetIndex
  return my csvPoint(item 2 of foundResult, item 3 of foundResult)
end elementCenter

on clickElement(appRef, targetIndex, clickCount, expectedIdentity)
  my ensureFrontmostWindow(expectedIdentity)
  set procRef to my appProcess(appRef)
  set rootElement to my primaryWindow(procRef, appRef)
  set foundResult to my clickIndex(rootElement, targetIndex, clickCount, 0)
  if item 1 of foundResult is false then error "element_index not found: " & targetIndex
  set xCoord to item 2 of foundResult
  set yCoord to item 3 of foundResult
  return "clicked element " & targetIndex & " at (" & xCoord & "," & yCoord & ") via accessibility"
end clickElement

on clickIndex(elementRef, targetIndex, clickCount, idx)
  set i to idx + 1
  if i is targetIndex then
    tell application "System Events"
      try
        set p to position of elementRef
        set s to size of elementRef
      on error
        error "element_index has no frame: " & targetIndex
      end try
      repeat clickCount times
        click elementRef
        delay 0.05
      end repeat
    end tell
    set xCoord to ((item 1 of p) + ((item 1 of s) / 2)) as integer
    set yCoord to ((item 2 of p) + ((item 2 of s) / 2)) as integer
    return {true, xCoord, yCoord, i}
  end if

  try
    tell application "System Events"
      set elementChildren to UI elements of elementRef
    end tell
    repeat with childRef in elementChildren
      set childResult to my clickIndex(childRef, targetIndex, clickCount, i)
      if item 1 of childResult is true then return childResult
      set i to item 4 of childResult
    end repeat
  end try
  return {false, 0, 0, i}
end clickIndex

on centerForIndex(elementRef, targetIndex, idx)
  set i to idx + 1
  if i is targetIndex then
    tell application "System Events"
      try
        set p to position of elementRef
        set s to size of elementRef
      on error
        error "element_index has no frame: " & targetIndex
      end try
    end tell
    set xCoord to ((item 1 of p) + ((item 1 of s) / 2)) as integer
    set yCoord to ((item 2 of p) + ((item 2 of s) / 2)) as integer
    return {true, xCoord, yCoord, i}
  end if

  try
    tell application "System Events"
      set elementChildren to UI elements of elementRef
    end tell
    repeat with childRef in elementChildren
      set childResult to my centerForIndex(childRef, targetIndex, i)
      if item 1 of childResult is true then return childResult
      set i to item 4 of childResult
    end repeat
  end try
  return {false, 0, 0, i}
end centerForIndex

on typeText(appRef, textToType, expectedIdentity)
  my appProcess(appRef)
  my ensureFrontmostWindow(expectedIdentity)
  tell application "System Events"
    keystroke textToType
  end tell
  return "typed " & (length of textToType as text) & " character(s)"
end typeText

on pasteText(appRef, textToPaste, expectedIdentity)
  my appProcess(appRef)
  my ensureFrontmostWindow(expectedIdentity)
  set previousClipboard to missing value
  try
    set previousClipboard to the clipboard
  end try
  set the clipboard to textToPaste
  my ensureFrontmostWindow(expectedIdentity)
  tell application "System Events"
    keystroke "v" using {command down}
  end tell
  delay 0.05
  try
    if previousClipboard is not missing value then set the clipboard to previousClipboard
  end try
  return "pasted " & (length of textToPaste as text) & " character(s)"
end pasteText

on keyModifiers(commandFlag, controlFlag, optionFlag, shiftFlag)
  set modifierKeys to {}
  if commandFlag is "1" then set end of modifierKeys to command down
  if controlFlag is "1" then set end of modifierKeys to control down
  if optionFlag is "1" then set end of modifierKeys to option down
  if shiftFlag is "1" then set end of modifierKeys to shift down
  return modifierKeys
end keyModifiers

on pressKey(appRef, keyText, keyCodeText, commandFlag, controlFlag, optionFlag, shiftFlag, displayText, expectedIdentity)
  my appProcess(appRef)
  my ensureFrontmostWindow(expectedIdentity)
  set modifierKeys to my keyModifiers(commandFlag, controlFlag, optionFlag, shiftFlag)
  tell application "System Events"
    if keyCodeText is not "" then
      if (count of modifierKeys) is 0 then
        key code (keyCodeText as integer)
      else
        key code (keyCodeText as integer) using modifierKeys
      end if
    else
      if (count of modifierKeys) is 0 then
        keystroke keyText
      else
        keystroke keyText using modifierKeys
      end if
    end if
  end tell
  return "pressed " & displayText
end pressKey

on setValue(appRef, targetIndex, newValue, expectedIdentity)
  my ensureFrontmostWindow(expectedIdentity)
  set procRef to my appProcess(appRef)
  set rootElement to my primaryWindow(procRef, appRef)
  set resultTuple to my setValueForIndex(rootElement, targetIndex, newValue, 0)
  if item 1 of resultTuple is false then error "element_index not found: " & targetIndex
  return "set value for element " & targetIndex
end setValue

on setValueForIndex(elementRef, targetIndex, newValue, idx)
  set i to idx + 1
  if i is targetIndex then
    tell application "System Events"
      set value of elementRef to newValue
    end tell
    return {true, i}
  end if

  try
    tell application "System Events"
      set elementChildren to UI elements of elementRef
    end tell
    repeat with childRef in elementChildren
      set childResult to my setValueForIndex(childRef, targetIndex, newValue, i)
      if item 1 of childResult is true then return childResult
      set i to item 2 of childResult
    end repeat
  end try
  return {false, i}
end setValueForIndex
"#;

#[cfg(target_os = "macos")]
const POINTER_SCRIPT: &str = r#"
ObjC.import('CoreGraphics')
ObjC.import('Foundation')

function numberArg(argv, index, fallback) {
  const parsed = Number(argv[index])
  return Number.isFinite(parsed) ? parsed : fallback
}

function sleep(seconds) {
  $.NSThread.sleepForTimeInterval(seconds)
}

function point(x, y) {
  return $.CGPointMake(x, y)
}

function postMouse(eventType, x, y, button) {
  const event = $.CGEventCreateMouseEvent(null, eventType, point(x, y), button)
  $.CGEventPost($.kCGHIDEventTap, event)
}

function postScroll(x, y, direction, pages) {
  const lines = Math.max(1, Math.round(pages * 10))
  let vertical = 0
  let horizontal = 0
  if (direction === 'up') vertical = lines
  if (direction === 'down') vertical = -lines
  if (direction === 'left') horizontal = lines
  if (direction === 'right') horizontal = -lines
  const event = $.CGEventCreateScrollWheelEvent(null, $.kCGScrollEventUnitLine, 2, vertical, horizontal)
  $.CGEventSetLocation(event, point(x, y))
  $.CGEventPost($.kCGHIDEventTap, event)
}

function buttonSpec(buttonName) {
  if (buttonName === 'right') {
    return { button: $.kCGMouseButtonRight, down: $.kCGEventRightMouseDown, up: $.kCGEventRightMouseUp }
  }
  if (buttonName === 'middle') {
    return { button: $.kCGMouseButtonCenter, down: $.kCGEventOtherMouseDown, up: $.kCGEventOtherMouseUp }
  }
  return { button: $.kCGMouseButtonLeft, down: $.kCGEventLeftMouseDown, up: $.kCGEventLeftMouseUp }
}

function clickAt(x, y, count, buttonName) {
  const spec = buttonSpec(buttonName)
  postMouse($.kCGEventMouseMoved, x, y, spec.button)
  sleep(0.03)
  for (let i = 0; i < count; i++) {
    postMouse(spec.down, x, y, spec.button)
    sleep(0.04)
    postMouse(spec.up, x, y, spec.button)
    sleep(0.05)
  }
}

function run(argv) {
  const actionName = argv[0]
  if (actionName === 'click') {
    const x = numberArg(argv, 1, 0)
    const y = numberArg(argv, 2, 0)
    const count = Math.max(1, Math.min(10, Math.round(numberArg(argv, 3, 1))))
    const button = argv[4] || 'left'
    clickAt(x, y, count, button)
    return 'clicked at (' + x + ',' + y + ')'
  }
  if (actionName === 'scroll') {
    const x = numberArg(argv, 1, 0)
    const y = numberArg(argv, 2, 0)
    const direction = argv[3] || 'down'
    const pages = Math.max(0.1, Math.min(10, numberArg(argv, 4, 1)))
    postScroll(x, y, direction, pages)
    return 'scrolled ' + direction + ' at (' + x + ',' + y + ') by ' + pages + ' page(s)'
  }
  throw new Error('unknown pointer action: ' + actionName)
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_negative_accessibility_frame() {
        let frame = parse_frame("-3982,-1410,1146,1410").unwrap();
        assert_eq!(frame.x, -3982);
        assert_eq!(frame.y, -1410);
        assert_eq!(frame.width, 1146);
        assert_eq!(frame.height, 1410);
        assert_eq!(frame.center().x, -3409);
        assert_eq!(frame.center().y, -705);
    }

    #[test]
    fn parses_legacy_applescript_list_csv() {
        let frame = parse_frame("-3982, ,, -1410, ,, 1146, ,, 1410").unwrap();
        assert_eq!(frame.x, -3982);
        assert_eq!(frame.y, -1410);
        assert_eq!(frame.width, 1146);
        assert_eq!(frame.height, 1410);

        let point = parse_point("-3409, ,, -705").unwrap();
        assert_eq!(point.x, -3409);
        assert_eq!(point.y, -705);
    }

    #[test]
    fn rejects_malformed_accessibility_coordinates() {
        let result = parse_point("-3409, nope").unwrap_err();
        assert!(result.is_error);
        assert!(result.content.contains("invalid y in point"));
    }

    #[test]
    fn resolves_app_relative_coordinates_against_window_frame() {
        let frame = ScreenFrame::new(-2835, -1410, 1146, 1410);
        let point =
            resolve_point_in_frame(frame, ScreenPoint::new(50, 50), CoordinateSpace::App).unwrap();
        assert_eq!(point, ScreenPoint::new(-2785, -1360));
    }

    #[test]
    fn rejects_screen_coordinates_outside_supplied_app_frame() {
        let frame = ScreenFrame::new(-2835, -1410, 1146, 1410);
        let result =
            resolve_point_in_frame(frame, ScreenPoint::new(50, 50), CoordinateSpace::Screen)
                .unwrap_err();
        assert!(result.is_error);
        assert!(result.content.contains("outside app window frame"));
    }

    #[test]
    fn parses_key_specs() {
        let key = parse_key_spec("command+k").unwrap();
        assert_eq!(key.key_text, "k");
        assert_eq!(key.key_code, None);
        assert!(key.command);
        assert!(!key.control);

        let key = parse_key_spec("ctrl+shift+Return").unwrap();
        assert_eq!(key.key_text, "Return");
        assert_eq!(key.key_code, Some(36));
        assert!(key.control);
        assert!(key.shift);
    }

    #[test]
    fn rejects_unsupported_key_specs() {
        let result = parse_key_spec("hyper+k").unwrap_err();
        assert!(result.content.contains("unsupported key modifier"));

        let result = parse_key_spec("command+LaunchMissiles").unwrap_err();
        assert!(result.content.contains("unsupported key"));
    }

    #[test]
    fn normalizes_provider_arg_tags_embedded_in_action() {
        let mut args = ComputerUseArgs {
            action: "focus_app\n<arg_key>app</arg_key>\n<arg_value>Google Chrome</arg_value>"
                .to_string(),
            app: None,
            element_index: None,
            x: None,
            y: None,
            coordinate_space: None,
            from_x: None,
            from_y: None,
            to_x: None,
            to_y: None,
            duration_ms: None,
            click_count: None,
            button: None,
            physical_input: false,
            direction: None,
            pages: None,
            text: None,
            key: None,
            value: None,
            max_depth: None,
        };

        args.normalize_provider_arg_tags();

        assert_eq!(args.action, "focus_app");
        assert_eq!(args.app.as_deref(), Some("Google Chrome"));
    }

    #[test]
    fn normalizes_unclosed_provider_arg_value_tag() {
        let mut args = ComputerUseArgs {
            action: "focus_app\n<arg_key>app</arg_key>\n<arg_value>Google Chrome".to_string(),
            app: None,
            element_index: None,
            x: None,
            y: None,
            coordinate_space: None,
            from_x: None,
            from_y: None,
            to_x: None,
            to_y: None,
            duration_ms: None,
            click_count: None,
            button: None,
            physical_input: false,
            direction: None,
            pages: None,
            text: None,
            key: None,
            value: None,
            max_depth: None,
        };

        args.normalize_provider_arg_tags();

        assert_eq!(args.action, "focus_app");
        assert_eq!(args.app.as_deref(), Some("Google Chrome"));
    }
}
