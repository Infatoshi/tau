use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<()> {
    let repo_root = repo_root()?;
    let home = dirs::home_dir().ok_or_else(|| anyhow!("could not find home directory"))?;
    let bin_dir = home.join(".local/bin");
    let bin_path = bin_dir.join("tau");
    let release_bin = repo_root.join("target/release/tau");

    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&repo_root)
        .status()
        .context("failed to run cargo build --release")?;

    if !status.success() {
        return Err(anyhow!("cargo build --release failed"));
    }

    fs::create_dir_all(&bin_dir)
        .with_context(|| format!("failed to create {}", bin_dir.display()))?;
    replace_link(&release_bin, &bin_path)?;
    ensure_path(&home, &bin_dir)?;

    println!("installed tau at {}", bin_path.display());
    println!("open a new shell, then run: tau");
    Ok(())
}

fn repo_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("could not resolve repository root"))
}

#[cfg(unix)]
fn replace_link(source: &Path, target: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;

    if target.exists() || target.is_symlink() {
        fs::remove_file(target)
            .with_context(|| format!("failed to remove {}", target.display()))?;
    }

    symlink(source, target).with_context(|| {
        format!(
            "failed to link {} to {}",
            target.display(),
            source.display()
        )
    })
}

#[cfg(not(unix))]
fn replace_link(source: &Path, target: &Path) -> Result<()> {
    fs::copy(source, target).with_context(|| {
        format!(
            "failed to copy {} to {}",
            source.display(),
            target.display()
        )
    })?;
    Ok(())
}

fn ensure_path(home: &Path, bin_dir: &Path) -> Result<()> {
    let path = env::var_os("PATH").unwrap_or_default();
    if env::split_paths(&path).any(|entry| entry == bin_dir) {
        return Ok(());
    }

    let zshrc = home.join(".zshrc");
    let existing = fs::read_to_string(&zshrc).unwrap_or_default();
    if existing.contains("HOME/.local/bin") {
        return Ok(());
    }

    let mut next = existing;
    if !next.ends_with('\n') && !next.is_empty() {
        next.push('\n');
    }
    next.push_str("export PATH=\"$HOME/.local/bin:$PATH\"\n");
    fs::write(&zshrc, next).with_context(|| format!("failed to update {}", zshrc.display()))?;
    println!("added ~/.local/bin to {}", zshrc.display());

    Ok(())
}
