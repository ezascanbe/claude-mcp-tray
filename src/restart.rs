//! Quitting and relaunching Claude Desktop so it reloads its MCP config.

#![allow(unused_imports)]

use anyhow::{anyhow, Result};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
pub fn restart_claude() -> Result<()> {
    use std::path::{Path, PathBuf};
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind};

    /// Desktop app processes only: the Claude Code CLI is also `claude.exe`.
    fn is_desktop(exe: &Path) -> bool {
        let p = exe.to_string_lossy().to_lowercase();
        p.ends_with("claude.exe")
            && (p.contains("\\windowsapps\\claude_") || p.contains("\\anthropicclaude\\"))
    }

    /// `...\WindowsApps\Claude_2.1.0.0_x64__abc123\app\claude.exe` -> `Claude_abc123!Claude`
    fn msix_app_id(exe: &Path) -> Option<String> {
        exe.ancestors().find_map(|dir| {
            let name = dir.file_name()?.to_string_lossy().into_owned();
            let (pkg, publisher) = name.split_once("__")?;
            let pkg_name = pkg.split('_').next()?;
            (pkg_name.eq_ignore_ascii_case("claude"))
                .then(|| format!("{pkg_name}_{publisher}!{pkg_name}"))
        })
    }

    fn installed_launch_target() -> Option<Launch> {
        let local = dirs::data_local_dir()?;
        let squirrel = local.join("AnthropicClaude").join("claude.exe");
        if squirrel.exists() {
            return Some(Launch::Exe(squirrel));
        }
        std::fs::read_dir(local.join("Packages")).ok()?.flatten().find_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let publisher = name.strip_prefix("Claude_")?;
            Some(Launch::AppId(format!("Claude_{publisher}!Claude")))
        })
    }

    enum Launch {
        Exe(PathBuf),
        AppId(String),
    }

    let refresh = ProcessRefreshKind::new().with_exe(UpdateKind::Always);
    let mut sys = System::new_with_specifics(RefreshKind::new().with_processes(refresh));

    let mut target = None;
    for proc_ in sys.processes().values() {
        let Some(exe) = proc_.exe() else { continue };
        if !is_desktop(exe) {
            continue;
        }
        if target.is_none() {
            target = Some(match msix_app_id(exe) {
                Some(id) => Launch::AppId(id),
                // Squirrel installs: launch via the stable root stub, not the versioned exe.
                None => exe
                    .ancestors()
                    .find(|d| d.file_name().is_some_and(|n| n.eq_ignore_ascii_case("AnthropicClaude")))
                    .map(|root| Launch::Exe(root.join("claude.exe")))
                    .unwrap_or_else(|| Launch::Exe(exe.to_path_buf())),
            });
        }
        proc_.kill();
    }

    // Closing the window only minimises Claude to the tray, so we terminate it and wait.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
        let alive = sys.processes().values().filter(|p| p.exe().is_some_and(is_desktop)).count();
        if alive == 0 {
            break;
        }
        if Instant::now() > deadline {
            return Err(anyhow!("Claude did not exit; please close it manually"));
        }
        thread::sleep(Duration::from_millis(250));
    }
    thread::sleep(Duration::from_millis(800));

    let target = target
        .or_else(installed_launch_target)
        .ok_or_else(|| anyhow!("could not find a Claude Desktop installation to launch"))?;
    match target {
        Launch::Exe(path) => Command::new(path).spawn()?,
        Launch::AppId(id) => Command::new("explorer.exe").arg(format!("shell:AppsFolder\\{id}")).spawn()?,
    };
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn restart_claude() -> Result<()> {
    fn running() -> bool {
        Command::new("pgrep")
            .args(["-x", "Claude"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    if running() {
        let _ = Command::new("osascript")
            .args(["-e", "tell application id \"com.anthropic.claudefordesktop\" to quit"])
            .status();
        let deadline = Instant::now() + Duration::from_secs(15);
        while running() {
            if Instant::now() > deadline {
                let _ = Command::new("pkill").args(["-x", "Claude"]).status();
                thread::sleep(Duration::from_secs(1));
                if running() {
                    return Err(anyhow!("Claude did not quit; please quit it manually"));
                }
                break;
            }
            thread::sleep(Duration::from_millis(250));
        }
        thread::sleep(Duration::from_millis(500));
    }
    let status = Command::new("open").args(["-a", "Claude"]).status()?;
    if !status.success() {
        return Err(anyhow!("could not relaunch Claude"));
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn restart_claude() -> Result<()> {
    let _ = Command::new("pkill").args(["-x", "claude-desktop"]).status();
    thread::sleep(Duration::from_secs(1));
    Command::new("claude-desktop").spawn()?;
    Ok(())
}
