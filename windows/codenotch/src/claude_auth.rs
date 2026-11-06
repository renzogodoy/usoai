//! User-initiated sign-in through the standalone Claude Code CLI. OAuth stays in
//! the CLI: no codes, tokens, browser URLs or credential writes cross widget IPC.
use serde::Serialize;
use std::{process::{Child, Command}, sync::{Mutex, OnceLock}, time::{Duration, Instant}};

#[derive(Clone, Default, Serialize)]
pub struct ProfileAuthState { pub busy: bool, pub message: String }
#[derive(Default)]
struct Registry { profiles: std::collections::HashMap<String, ProfileAuthState>, last_profile: Option<String> }
fn registry() -> std::sync::MutexGuard<'static, Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default())).lock().unwrap_or_else(|e| e.into_inner())
}
#[derive(Serialize)]
pub struct AuthState {
    pub busy: bool, pub message: String, pub profile_id: Option<String>,
    pub accounts: std::collections::HashMap<String, ProfileAuthState>,
}
pub fn state() -> AuthState {
    let data = registry();
    AuthState { busy: data.profiles.values().any(|s| s.busy),
        message: data.last_profile.as_ref().and_then(|id| data.profiles.get(id)).map(|s| s.message.clone()).unwrap_or_default(),
        profile_id: data.last_profile.clone(), accounts: data.profiles.clone() }
}
pub fn state_for(profile_id: &str) -> ProfileAuthState { registry().profiles.get(profile_id).cloned().unwrap_or_default() }
fn set_message(profile_id: &str, message: &str) {
    let mut data = registry();
    data.last_profile = Some(profile_id.into());
    data.profiles.entry(profile_id.into()).or_default().message = message.into();
}
pub struct AuthGuard { profile_id: String }
#[cfg(test)]
pub fn try_acquire() -> Option<AuthGuard> { try_acquire_for("default") }
pub fn try_acquire_for(profile_id: &str) -> Option<AuthGuard> {
    let mut data = registry();
    let state = data.profiles.entry(profile_id.into()).or_default();
    if state.busy { return None; }
    state.busy = true;
    Some(AuthGuard { profile_id: profile_id.into() })
}
impl Drop for AuthGuard {
    fn drop(&mut self) { if let Some(state) = registry().profiles.get_mut(&self.profile_id) { state.busy = false; } }
}
pub fn usage_succeeded(profile_id: &str) {
    if let Some(state) = registry().profiles.get_mut(profile_id) { if !state.busy { state.message.clear(); } }
}

// No interpolated shell input: even paths containing apostrophes arrive in env.
#[cfg(windows)]
const LOGIN_SCRIPT: &str = "$Host.UI.RawUI.WindowTitle = 'Codenotch - Claude sign-in'; Write-Host 'Complete sign-in in your browser. Paste any code in this window.'; & $env:CODENOTCH_CLAUDE_CLI auth login --claudeai; $loginResult = $LASTEXITCODE; if ($loginResult -eq 0) { Write-Host 'Sign-in complete. Codenotch will refresh automatically.'; Start-Sleep -Seconds 2 } else { Write-Host 'Sign-in failed or cancelled. Retry from Codenotch.'; Start-Sleep -Seconds 8 }; exit $loginResult";

/// The same sign-in, driven by a POSIX shell inside a terminal window. The CLI path
/// travels in the environment here too, so a path with quotes in it is never parsed.
#[cfg(not(windows))]
const LOGIN_SH: &str = "echo 'Complete sign-in in your browser. Paste any code in this window.'; \"$CODENOTCH_CLAUDE_CLI\" auth login --claudeai; r=$?; if [ $r -eq 0 ]; then echo 'Sign-in complete. Codenotch will refresh automatically.'; sleep 2; else echo 'Sign-in failed or cancelled. Retry from Codenotch.'; sleep 8; fi; exit $r";

/// A terminal emulator that can run a command, in the order a desktop is likely to have one.
/// `-e` is understood by all of these; the Debian alternative comes first so the user's own
/// choice wins.
#[cfg(not(windows))]
fn terminal_emulator() -> Option<std::path::PathBuf> {
    const CANDIDATES: [&str; 8] = [
        "x-terminal-emulator",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "kgx",
        "alacritty",
        "kitty",
        "xterm",
    ];
    let path = std::env::var_os("PATH")?;
    for name in CANDIDATES {
        for dir in std::env::split_paths(&path) {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn login_command(cli: &std::path::Path, profile_dir: &std::path::Path) -> Result<Command, String> {
    let term = terminal_emulator().ok_or("No terminal emulator found. Run `claude auth login` yourself.")?;
    let mut cmd = Command::new(term);
    cmd.args(["-e", "sh", "-c", LOGIN_SH]).env("CODENOTCH_CLAUDE_CLI", cli);
    cmd.current_dir(dirs::home_dir().ok_or("Home directory unavailable.")?);
    for (key, _) in std::env::vars_os() {
        let k = key.to_string_lossy();
        if k == "CLAUDECODE" || k.starts_with("CLAUDE_CODE_")
            || matches!(k.as_ref(), "CLAUDE_CONFIG_DIR" | "ANTHROPIC_API_KEY" | "ANTHROPIC_AUTH_TOKEN") {
            cmd.env_remove(&key);
        }
    }
    cmd.env("CLAUDE_CONFIG_DIR", profile_dir);
    Ok(cmd)
}

#[cfg(windows)]
fn login_command(cli: &std::path::Path, profile_dir: &std::path::Path) -> Result<Command, String> {
    let root = std::env::var_os("SystemRoot").ok_or("Windows directory unavailable.")?;
    let mut cmd = Command::new(std::path::PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe"));
    cmd.args(["-NoLogo", "-NoProfile", "-Command", LOGIN_SCRIPT]).env("CODENOTCH_CLAUDE_CLI", cli);
    cmd.current_dir(dirs::home_dir().ok_or("Home directory unavailable.")?);
    // A widget launched inside a Claude session must not inherit that session's
    // auth or take the headless refresh-token login branch instead of the browser.
    for (key, _) in std::env::vars_os() {
        let k = key.to_string_lossy();
        if k == "CLAUDECODE" || k.starts_with("CLAUDE_CODE_")
            || matches!(k.as_ref(), "CLAUDE_CONFIG_DIR" | "ANTHROPIC_API_KEY" | "ANTHROPIC_AUTH_TOKEN") {
            cmd.env_remove(&key);
        }
    }
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0010); // visible console only after a user's click
    }
    cmd.env("CLAUDE_CONFIG_DIR", profile_dir);
    Ok(cmd)
}

pub(crate) fn terminate(child: &mut Child) {
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        if let Some(root) = std::env::var_os("SystemRoot") {
            // A .cmd CLI may have node children: terminate only this owned tree.
            if let Ok(mut killer) = Command::new(std::path::PathBuf::from(root).join("System32/taskkill.exe"))
                .args(["/PID", &child.id().to_string(), "/T", "/F"])
                .creation_flags(0x0800_0000).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
                let deadline = Instant::now() + Duration::from_secs(1);
                while matches!(killer.try_wait(), Ok(None)) && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(25)); }
                let _ = killer.kill(); let _ = killer.wait();
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn wait_child(child: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(250)),
            _ => { terminate(child); return false; }
        }
    }
}

pub fn start_login() -> Result<(), String> {
    let home = dirs::home_dir().ok_or("Home directory unavailable")?;
    start_profile_login(&home.join(".claude"), "default")
}

pub fn start_profile_login(profile_dir: &std::path::Path, profile_id: &str) -> Result<(), String> {
    let cli = crate::usage::find_cli().ok_or("Claude Code CLI not found. Install the standalone CLI first.")?;
    let guard = try_acquire_for(profile_id).ok_or("Claude sign-in or renewal is already running.")?;
    let mut cmd = login_command(&cli, profile_dir)?;
    let mut child = cmd.spawn().map_err(|_| "Unable to open Claude sign-in window.")?;
    set_message(profile_id, "Complete sign-in in the browser or terminal window.");
    let profile_id = profile_id.to_string();
    std::thread::spawn(move || {
        let ok = wait_child(&mut child, Duration::from_secs(15 * 60));
        set_message(&profile_id, if ok { "Sign-in complete. Refreshing usage..." } else {
            "Sign-in cancelled, failed or timed out. Try again."
        });
        drop(guard);
        if ok { crate::usage::request_account_refresh(&profile_id); }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gate_excludes_login_and_renewal_and_releases_on_drop() {
        let guard = try_acquire().unwrap();
        assert!(try_acquire().is_none());
        assert!(state_for("default").busy);
        drop(guard);
        assert!(!state_for("default").busy);
        assert!(try_acquire().is_some());
        set_message("work", "Sign-in complete. Refreshing usage...");
        usage_succeeded("default");
        assert!(!state().message.is_empty(), "an unrelated account cannot acknowledge this account's sign-in");
        usage_succeeded("work");
        assert!(state().message.is_empty());
        registry().last_profile = None;
    }
    #[test]
    fn login_and_renewal_are_isolated_by_profile() {
        let work = try_acquire_for("test-work").unwrap();
        assert!(try_acquire_for("test-work").is_none());
        let personal = try_acquire_for("test-personal").unwrap();
        assert!(state_for("test-work").busy);
        drop(work);
        assert!(!state_for("test-work").busy);
        assert!(state_for("test-personal").busy);
        drop(personal);
    }
    #[test]
    #[cfg(windows)]
    fn paths_are_data_not_shell_source() {
        let path = std::path::Path::new(r"C:\fixture with spaces\O'Brien\claude.cmd");
        let profile = std::path::Path::new(r"C:\Users\fixture\.claude-work");
        let cmd = login_command(path, profile).unwrap();
        assert!(cmd.get_envs().any(|(k,v)| k == "CLAUDE_CONFIG_DIR" && v == Some(profile.as_os_str())));
        assert!(cmd.get_args().all(|arg| !arg.to_string_lossy().contains("O'Brien")));
        assert!(cmd.get_envs().any(|(k,v)| k == "CODENOTCH_CLAUDE_CLI" && v == Some(path.as_os_str())));
    }
    #[test]
    #[cfg(windows)]
    fn failed_exit_and_timeout_are_reaped() {
        use std::os::windows::process::CommandExt;
        let root = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap());
        let where_exe = root.join("System32/where.exe");
        let mut child = Command::new(&where_exe)
            .arg("codenotch-no-such-command-394.exe").creation_flags(0x0800_0000)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        assert!(!wait_child(&mut child, Duration::from_secs(5)));
        let mut child = Command::new(&where_exe)
            .arg("cmd.exe").creation_flags(0x0800_0000)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        assert!(wait_child(&mut child, Duration::from_secs(5)));
        let mut child = Command::new(root.join("System32/ping.exe"))
            .args(["-n", "30", "127.0.0.1"]).creation_flags(0x0800_0000)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let start = Instant::now();
        assert!(!wait_child(&mut child, Duration::from_millis(300)));
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(child.try_wait().unwrap().is_some());
    }
}
