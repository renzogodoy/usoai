//! User-triggered official CLI login, with independent gates for each profile.
use serde::Serialize;
use std::{collections::HashMap, path::Path, process::Command, sync::{Mutex, OnceLock}, time::{Duration, Instant}};

#[derive(Clone, Default, Serialize)]
pub struct LoginState { pub busy: bool, pub message: String }

fn states() -> &'static Mutex<HashMap<String, LoginState>> {
    static STATES: OnceLock<Mutex<HashMap<String, LoginState>>> = OnceLock::new();
    STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[tauri::command]
pub fn get_codex_logins() -> HashMap<String, LoginState> { states().lock().unwrap().clone() }

struct Gate(String);
impl Gate {
    fn acquire(id: &str) -> Result<Self, String> {
        let mut states = states().lock().unwrap();
        if states.get(id).is_some_and(|state| state.busy) { return Err("This account is already signing in".into()); }
        states.insert(id.into(), LoginState { busy: true, message: String::new() });
        Ok(Self(id.into()))
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        if let Some(state) = states().lock().unwrap().get_mut(&self.0) { state.busy = false; }
    }
}

#[cfg(windows)]
fn command(cli: &Path, dir: &Path) -> Result<Command, String> {
    use std::os::windows::process::CommandExt;
    let root = std::env::var_os("SystemRoot").ok_or("Windows directory unavailable")?;
    let mut command = Command::new(std::path::PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe"));
    command.args(["-NoLogo", "-NoProfile", "-Command",
        "& $env:USOAI_CODEX_CLI -c 'cli_auth_credentials_store=\"file\"' login; exit $LASTEXITCODE"]);
    command.env("USOAI_CODEX_CLI", cli).env("CODEX_HOME", dir)
        .env_remove("OPENAI_API_KEY").env_remove("OPENAI_BASE_URL");
    command.creation_flags(0x00000010);
    Ok(command)
}

#[cfg(not(windows))]
fn command(_: &Path, _: &Path) -> Result<Command, String> { Err("Interactive Codex login requires Windows".into()) }

pub fn start(dir: &Path, id: &str) -> Result<(), String> {
    let cli = crate::codex::login_executable().ok_or("Install the official Codex CLI to connect an account")?;
    let gate = Gate::acquire(id)?;
    let mut child = command(&cli, dir)?.spawn().map_err(|_| "Unable to open the Codex sign-in window")?;
    states().lock().unwrap().get_mut(id).unwrap().message = "Complete sign-in in the browser or terminal window.".into();
    let id = id.to_string();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(900);
        let ok = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.success(),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(250)),
                _ => { crate::claude_auth::terminate(&mut child); break false; }
            }
        };
        if let Some(state) = states().lock().unwrap().get_mut(&id) {
            state.message = if ok { "Sign-in complete. Waiting for the next usage reading." } else { "Sign-in cancelled, failed or timed out. Try again." }.into();
        }
        drop(gate);
        if ok { crate::account_usage::credentials_changed(&id); }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_gate_blocks_only_the_same_account_and_releases_on_error() {
        let a = Gate::acquire("codex-test-a").unwrap();
        assert!(Gate::acquire("codex-test-a").is_err());
        let b = Gate::acquire("codex-test-b").unwrap();
        drop(a);
        assert!(Gate::acquire("codex-test-a").is_ok());
        drop(b);
    }
    #[test]
    #[cfg(windows)]
    fn profile_paths_are_passed_as_data_not_powershell_source() {
        let dir = Path::new(r"C:\Users\fixture with ' quotes\.codex-work");
        let cli = Path::new(r"C:\Program Files\Codex\codex.exe");
        let command = command(cli, dir).unwrap();
        assert!(command.get_envs().any(|(name, value)| name == "CODEX_HOME" && value == Some(dir.as_os_str())));
        assert!(command.get_args().all(|arg| !arg.to_string_lossy().contains("fixture with")));
    }
}
