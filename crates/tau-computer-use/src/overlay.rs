use tau_core::ToolResult;

pub use tau_computer_use_layout::ScreenPoint as OverlayPoint;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayMoveRequest {
    pub from: OverlayPoint,
    pub to: OverlayPoint,
    pub duration_ms: u64,
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Default)]
pub struct OverlaySession;

#[cfg(not(target_os = "macos"))]
impl OverlaySession {
    pub fn new() -> Self {
        Self
    }

    pub async fn show(&mut self, _: OverlayPoint) -> anyhow::Result<ToolResult> {
        Ok(ToolResult {
            content: "computer_use overlay is only supported on macOS".to_string(),
            is_error: true,
        })
    }

    pub async fn move_tau(&mut self, _: OverlayMoveRequest) -> anyhow::Result<ToolResult> {
        Ok(ToolResult {
            content: "computer_use overlay is only supported on macOS".to_string(),
            is_error: true,
        })
    }

    pub async fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Default)]
pub struct OverlaySession {
    child: Option<tokio::process::Child>,
    command_path: Option<std::path::PathBuf>,
}

#[cfg(target_os = "macos")]
impl OverlaySession {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn show(&mut self, point: OverlayPoint) -> anyhow::Result<ToolResult> {
        self.ensure_started(point).await?;
        self.write_command(&format!("show {},{}", point.x, point.y))
            .await?;
        Ok(ToolResult {
            content: format!(
                "showing tau mark at ({},{}) until computer_use turn ends",
                point.x, point.y
            ),
            is_error: false,
        })
    }

    pub async fn move_tau(&mut self, request: OverlayMoveRequest) -> anyhow::Result<ToolResult> {
        self.ensure_started(request.from).await?;
        self.write_command(&format!(
            "move {},{} {},{} {}",
            request.from.x, request.from.y, request.to.x, request.to.y, request.duration_ms
        ))
        .await?;
        Ok(ToolResult {
            content: format!(
                "moved tau mark linearly from ({},{}) to ({},{}) for {} ms; marker remains visible until computer_use turn ends",
                request.from.x, request.from.y, request.to.x, request.to.y, request.duration_ms
            ),
            is_error: false,
        })
    }

    pub async fn stop(&mut self) -> anyhow::Result<()> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        if let Some(path) = self.command_path.take() {
            let _ = tokio::fs::write(&path, "stop").await;
            let _ = tokio::time::timeout(std::time::Duration::from_millis(700), child.wait()).await;
            let _ = tokio::fs::remove_file(path).await;
        }
        if child.id().is_some() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        Ok(())
    }

    async fn ensure_started(&mut self, point: OverlayPoint) -> anyhow::Result<()> {
        if self.child.as_mut().and_then(|child| child.id()).is_some() {
            return Ok(());
        }
        let command_path = overlay_command_path();
        tokio::fs::write(&command_path, format!("show {},{}", point.x, point.y)).await?;

        use std::process::Stdio;
        use tokio::io::AsyncWriteExt;
        use tokio::process::Command;

        let mut child = Command::new("osascript")
            .arg("-l")
            .arg("JavaScript")
            .arg("-")
            .arg(command_path.to_string_lossy().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let mut stdin = child.stdin.take().expect("stdin piped");
        stdin.write_all(SESSION_OVERLAY_SCRIPT.as_bytes()).await?;
        drop(stdin);

        self.child = Some(child);
        self.command_path = Some(command_path);
        Ok(())
    }

    async fn write_command(&self, command: &str) -> anyhow::Result<()> {
        let Some(path) = &self.command_path else {
            anyhow::bail!("tau overlay session is not running");
        };
        tokio::fs::write(path, command).await?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn overlay_command_path() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "tau-computer-use-overlay-{}-{nanos}.cmd",
        std::process::id()
    ))
}

#[cfg(target_os = "macos")]
const SESSION_OVERLAY_SCRIPT: &str = r#"
ObjC.import('Cocoa')
ObjC.import('Foundation')

function numberOr(raw, fallback) {
  const parsed = Number(raw)
  return Number.isFinite(parsed) ? parsed : fallback
}

function nsColor(r, g, b, a) {
  return $.NSColor.colorWithCalibratedRedGreenBlueAlpha(r, g, b, a)
}

function makeShadow(blurRadius, offsetY, color) {
  const shadow = $.NSShadow.alloc.init
  shadow.setShadowBlurRadius(blurRadius)
  shadow.setShadowOffset($.NSMakeSize(0, offsetY))
  shadow.setShadowColor(color)
  return shadow
}

function attrs(fontSize, color, shadow) {
  const dict = $.NSMutableDictionary.alloc.init
  dict.setObjectForKey($.NSFont.boldSystemFontOfSize(fontSize), $.NSFontAttributeName)
  dict.setObjectForKey(color, $.NSForegroundColorAttributeName)
  if (shadow) {
    dict.setObjectForKey(shadow, $.NSShadowAttributeName)
  }
  return dict
}

function screenHeight() {
  return $.NSScreen.mainScreen.frame.size.height
}

function frameXFor(x) {
  return x - 24
}

function frameYFor(topY, frameHeight) {
  return screenHeight() - topY - frameHeight + 24
}

function drawTauMark() {
  const mark = $('τ')
  const base = $.NSMakePoint(15, 7)
  $.NSGraphicsContext.currentContext.saveGraphicsState
  mark.drawAtPointWithAttributes($.NSMakePoint(14, 7), attrs(29, nsColor(0.96, 0.46, 0.16, 0.28), makeShadow(24, 0, nsColor(1.0, 0.58, 0.20, 0.86))))
  mark.drawAtPointWithAttributes($.NSMakePoint(15, 7), attrs(28, nsColor(0.98, 0.62, 0.22, 0.36), makeShadow(15, 0, nsColor(0.95, 0.36, 0.10, 0.70))))
  mark.drawAtPointWithAttributes($.NSMakePoint(17, 6), attrs(28, nsColor(0.07, 0.025, 0.01, 0.88), makeShadow(2, -1, nsColor(0.04, 0.015, 0.006, 0.60))))
  mark.drawAtPointWithAttributes(base, attrs(28, nsColor(0.90, 0.33, 0.10, 1.0), makeShadow(6, 0, nsColor(1.0, 0.72, 0.30, 0.50))))
  mark.drawAtPointWithAttributes($.NSMakePoint(14, 12), attrs(15, nsColor(1.0, 0.84, 0.47, 0.72), makeShadow(4, 0, nsColor(1.0, 0.82, 0.40, 0.42))))
  mark.drawAtPointWithAttributes($.NSMakePoint(16, 10), attrs(22, nsColor(1.0, 0.73, 0.31, 0.38), null))
  $.NSGraphicsContext.currentContext.restoreGraphicsState
}

function readCommand(path) {
  const text = $.NSString.stringWithContentsOfFileEncodingError(path, $.NSUTF8StringEncoding, null)
  return text ? ObjC.unwrap(text).trim() : ''
}

function parsePoint(raw, fallbackX, fallbackY) {
  const parts = raw.split(',')
  return {
    x: numberOr(parts[0], fallbackX),
    y: numberOr(parts[1], fallbackY)
  }
}

function makeWindow(frameWidth, frameHeight) {
  const app = $.NSApplication.sharedApplication
  app.setActivationPolicy($.NSApplicationActivationPolicyAccessory)

  ObjC.registerSubclass({
    name: 'TauComputerUseSessionOverlayView',
    superclass: 'NSView',
    methods: {
      'drawRect:': function(_rect) {
        drawTauMark()
      }
    }
  })

  const window = $.NSWindow.alloc.initWithContentRectStyleMaskBackingDefer(
    $.NSMakeRect(0, 0, frameWidth, frameHeight),
    $.NSWindowStyleMaskBorderless,
    $.NSBackingStoreBuffered,
    false
  )
  window.setOpaque(false)
  window.setBackgroundColor($.NSColor.clearColor)
  window.setIgnoresMouseEvents(true)
  window.setLevel($.NSStatusWindowLevel)
  window.setCollectionBehavior(
    $.NSWindowCollectionBehaviorCanJoinAllSpaces |
    $.NSWindowCollectionBehaviorFullScreenAuxiliary |
    $.NSWindowCollectionBehaviorStationary
  )

  const view = $.TauComputerUseSessionOverlayView.alloc.initWithFrame($.NSMakeRect(0, 0, frameWidth, frameHeight))
  window.setContentView(view)
  window.orderFrontRegardless
  return window
}

function placeWindow(window, x, topY, frameHeight) {
  window.setFrameOrigin($.NSMakePoint(frameXFor(x), frameYFor(topY, frameHeight)))
}

function animateWindow(window, from, to, durationMs, frameHeight) {
  const start = Date.now()
  const end = Date.now() + Math.max(1, durationMs)
  while (Date.now() < end) {
    const progress = Math.min(1, Math.max(0, (Date.now() - start) / Math.max(1, durationMs)))
    const currentX = from.x + ((to.x - from.x) * progress)
    const currentY = from.y + ((to.y - from.y) * progress)
    placeWindow(window, currentX, currentY, frameHeight)
    $.NSRunLoop.currentRunLoop.runUntilDate($.NSDate.dateWithTimeIntervalSinceNow(0.025))
  }
  placeWindow(window, to.x, to.y, frameHeight)
}

function run(argv) {
  const commandPath = argv[0]
  const frameWidth = 48
  const frameHeight = 48
  const window = makeWindow(frameWidth, frameHeight)
  let current = { x: 160, y: 120 }
  let lastCommand = ''

  while (true) {
    const command = readCommand(commandPath)
    if (command !== '' && command !== lastCommand) {
      lastCommand = command
      if (command === 'stop') {
        window.orderOut(null)
        return
      }
      if (command.startsWith('show ')) {
        current = parsePoint(command.slice(5), current.x, current.y)
        placeWindow(window, current.x, current.y, frameHeight)
      } else if (command.startsWith('move ')) {
        const parts = command.split(' ')
        const from = parsePoint(parts[1] || '', current.x, current.y)
        const to = parsePoint(parts[2] || '', from.x, from.y)
        const durationMs = numberOr(parts[3], 700)
        animateWindow(window, from, to, durationMs, frameHeight)
        current = to
      }
    }
    $.NSRunLoop.currentRunLoop.runUntilDate($.NSDate.dateWithTimeIntervalSinceNow(0.05))
  }
}
"#;
