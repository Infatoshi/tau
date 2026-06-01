use tau_computer_use_layout::ScreenFrame;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedImage {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

impl CapturedImage {
    pub fn png(bytes: Vec<u8>) -> Self {
        Self {
            mime_type: "image/png".to_string(),
            bytes,
        }
    }
}

pub fn screencapture_args(region: Option<ScreenFrame>, path: &str) -> Vec<String> {
    let mut args = vec!["-x".to_string(), "-t".to_string(), "png".to_string()];
    if let Some(region) = region {
        args.push("-R".to_string());
        args.push(region.screencapture_region());
    }
    args.push(path.to_string());
    args
}

#[cfg(target_os = "macos")]
pub async fn capture_png(region: Option<ScreenFrame>) -> anyhow::Result<CapturedImage> {
    use tokio::process::Command;

    let tempdir = tempfile::tempdir()?;
    let path = tempdir.path().join("capture.png");
    let path_text = path.to_string_lossy().to_string();
    let output = Command::new("screencapture")
        .args(screencapture_args(region, &path_text))
        .output()
        .await?;
    if !output.status.success() {
        anyhow::bail!(
            "screencapture failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(CapturedImage::png(tokio::fs::read(path).await?))
}

#[cfg(not(target_os = "macos"))]
pub async fn capture_png(_: Option<ScreenFrame>) -> anyhow::Result<CapturedImage> {
    anyhow::bail!("desktop capture is only supported on macOS")
}
