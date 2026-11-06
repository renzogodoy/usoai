//! The accounts UI receives metadata and usage only, never credential values.
use crate::{accounts::{self, Account, AccountOrigin, CredentialSource, ProviderKind}, config::{Config, ClaudeProfileConfig}, AppState};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize)]
pub struct AccountView {
    #[serde(flatten)]
    account: Account,
    connected: bool,
    has_secret: bool,
    status: String,
    fetched_at: u64,
    windows: Vec<crate::usage::LimitWindow>,
}

#[derive(Clone, Serialize)]
pub struct Candidate { pub provider: ProviderKind, pub path: PathBuf, pub label: String }

fn supported(provider: ProviderKind) -> bool {
    matches!(provider, ProviderKind::Claude | ProviderKind::Codex | ProviderKind::OpenCode)
}

fn canonical(path: &Path) -> String {
    let text = path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned();
    if cfg!(windows) { text.to_lowercase() } else { text }
}

fn has_credential(provider: ProviderKind, path: &Path) -> bool {
    match provider {
        ProviderKind::Claude => [".credentials.json", "credentials.json"].iter().any(|n| path.join(n).is_file()),
        ProviderKind::Codex => path.join("auth.json").is_file(),
        ProviderKind::OpenCode => path.join("auth.json").is_file() || path.join("opencode.db").is_file(),
        _ => false,
    }
}

fn registered(cfg: &Config, home: &Path, provider: ProviderKind, path: &Path) -> bool {
    cfg.accounts.iter().filter(|a| a.provider == provider).any(|a| accounts::config_dir_at(a, home)
        .is_some_and(|dir| canonical(&dir) == canonical(path)))
}

pub fn discover_at(home: &Path, cfg: &Config, environment: &[(ProviderKind, PathBuf)]) -> Vec<Candidate> {
    let mut found = Vec::new();
    let mut offer = |provider: ProviderKind, path: PathBuf| {
        if !has_credential(provider, &path) || registered(cfg, home, provider, &path)
            || cfg.excluded_account_dirs.iter().any(|dir| canonical(dir) == canonical(&path))
            || found.iter().any(|c: &Candidate| c.provider == provider && canonical(&c.path) == canonical(&path)) { return; }
        let label = path.file_name().unwrap_or_default().to_string_lossy().trim_start_matches('.').to_string();
        found.push(Candidate { provider, path, label });
    };
    for (provider, root, prefix) in [
        (ProviderKind::Claude, home.to_path_buf(), ".claude-"),
        (ProviderKind::Codex, home.to_path_buf(), ".codex-"),
        (ProviderKind::OpenCode, home.join(".local/share"), "opencode-"),
    ] {
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.take(512).flatten() {
                if entry.file_name().to_string_lossy().starts_with(prefix) && entry.path().is_dir() { offer(provider, entry.path()); }
            }
        }
    }
    for (provider, path) in environment { offer(*provider, path.clone()); }
    found.sort_by(|a, b| a.provider.id().cmp(b.provider.id()).then(a.path.cmp(&b.path)));
    found
}

fn sync_claude(cfg: &mut Config, account: &Account) {
    if account.provider != ProviderKind::Claude { return; }
    let legacy_id = if account.id == "claude" { "default" } else { account.id.strip_prefix("claude-").unwrap_or(&account.id) };
    if let Some(profile) = cfg.claude_profiles.iter_mut().find(|p| p.id == legacy_id) {
        profile.name = account.label.clone(); profile.enabled = account.enabled;
    } else {
        cfg.claude_profiles.push(ClaudeProfileConfig { id: legacy_id.into(), name: account.label.clone(), enabled: account.enabled });
    }
}

fn changed(app: &AppHandle) {
    crate::usage::request_refresh();
    let _ = app.emit("accounts_changed", ());
    let _ = app.emit("claude_accounts_changed", ());
}

#[tauri::command]
pub fn list_accounts(app: AppHandle) -> Vec<AccountView> {
    let st = app.state::<AppState>();
    let cfg = st.cfg.lock().unwrap().clone();
    let snapshots = st.account_usage.lock().unwrap().clone();
    let claude = st.usage.lock().unwrap().clone();
    cfg.accounts.into_iter().filter(|a| supported(a.provider)).map(|account| {
        let has_secret = matches!(account.source, CredentialSource::Secret { .. }) && crate::secrets::read(&account.id).ok().flatten().is_some();
        let connected = match &account.source {
            CredentialSource::Secret { .. } => has_secret,
            CredentialSource::EnvVar { name } => std::env::var(name).is_ok_and(|value| !value.trim().is_empty()),
            _ => accounts::config_dir(&account).is_some_and(|path| has_credential(account.provider, &path)),
        };
        let snapshot = snapshots.get(&account.id);
        let claude_id = if account.id == "claude" { "default" } else { account.id.strip_prefix("claude-").unwrap_or("") };
        let claude_snapshot = if account.provider == ProviderKind::Claude { claude.accounts.iter().find(|a| a.id == claude_id) } else { None };
        AccountView { status: snapshot.map(|s| s.status.clone()).or_else(|| claude_snapshot.map(|s| s.status.clone()))
            .unwrap_or_else(|| if connected { "none" } else { "needsAuth" }.into()),
            fetched_at: snapshot.map(|s| s.fetched_at).or_else(|| claude_snapshot.map(|s| s.fetched_at)).unwrap_or(0),
            windows: snapshot.map(|s| s.windows.clone()).or_else(|| claude_snapshot.map(|s| s.windows.clone())).unwrap_or_default(),
            account, connected, has_secret }
    }).collect()
}

#[tauri::command]
pub fn detect_accounts(app: AppHandle) -> Result<Vec<Candidate>, String> {
    let home = dirs::home_dir().ok_or("Home directory unavailable")?;
    let cfg = app.state::<AppState>().cfg.lock().unwrap().clone();
    let mut env = Vec::new();
    for (name, provider) in [("CLAUDE_CONFIG_DIR", ProviderKind::Claude), ("CODEX_HOME", ProviderKind::Codex), ("XDG_DATA_HOME", ProviderKind::OpenCode)] {
        if let Some(value) = std::env::var_os(name).filter(|value| !value.is_empty()) {
            let path = PathBuf::from(value);
            env.push((provider, if name == "XDG_DATA_HOME" { path.join("opencode") } else { path }));
        }
    }
    Ok(discover_at(&home, &cfg, &env))
}

#[tauri::command]
pub fn add_account(app: AppHandle, provider: ProviderKind, label: String, method: String,
    path: Option<PathBuf>, credential: Option<String>, variable: Option<String>) -> Result<String, String> {
    if !supported(provider) { return Err("This provider does not support additional accounts yet".into()); }
    let label = accounts::checked_label(&label)?;
    let home = dirs::home_dir().ok_or("Home directory unavailable")?;
    let st = app.state::<AppState>();
    let mut cfg = st.cfg.lock().unwrap();
    let mut next = cfg.clone();
    let id = accounts::next_id(&next.accounts, provider, &label);
    let mut created_dir = None;
    let source = match method.as_str() {
        "folder" => {
            let path = path.ok_or("Choose an existing profile folder")?;
            if !path.is_absolute() || !has_credential(provider, &path) { return Err("The folder has no credential for this provider".into()); }
            if registered(&next, &home, provider, &path) { return Err("This folder is already registered".into()); }
            next.excluded_account_dirs.retain(|dir| canonical(dir) != canonical(&path));
            CredentialSource::ConfigDir { path: path.canonicalize().map_err(|_| "Unable to open the profile folder")? }
        }
        "login" if matches!(provider, ProviderKind::Claude | ProviderKind::Codex) => {
            let slug = id.strip_prefix(&format!("{}-", provider.id())).ok_or("Invalid account ID")?;
            let path = home.join(format!(".{}-{slug}", provider.id()));
            std::fs::create_dir(&path).map_err(|_| "The profile folder already exists or could not be created")?;
            created_dir = Some(path.clone());
            CredentialSource::ConfigDir { path }
        }
        "token" if provider == ProviderKind::OpenCode => {
            let value = credential.as_deref().ok_or("Enter an OpenCode Go API key")?;
            crate::secrets::set(&id, value.trim())?;
            CredentialSource::Secret { key: format!("usoai/{id}") }
        }
        "env" if provider == ProviderKind::OpenCode => {
            let name = variable.unwrap_or_default();
            if name.is_empty() || name.len() > 128 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return Err("Enter a valid environment variable name".into());
            }
            CredentialSource::EnvVar { name }
        }
        _ => return Err("Unsupported connection method".into()),
    };
    let account = Account { id: id.clone(), provider, label, email: None, plan: None, enabled: true,
        source, origin: AccountOrigin::Manual };
    sync_claude(&mut next, &account);
    next.accounts.push(account.clone());
    if let Err(error) = crate::config::save_checked(&next) {
        if matches!(account.source, CredentialSource::Secret { .. }) { let _ = crate::secrets::remove(&id); }
        if let Some(path) = created_dir { let _ = std::fs::remove_dir(path); }
        return Err(error);
    }
    *cfg = next;
    drop(cfg);
    changed(&app);
    Ok(id)
}

#[tauri::command]
pub fn update_account(app: AppHandle, id: String, label: String, enabled: bool) -> Result<(), String> {
    let label = accounts::checked_label(&label)?;
    let st = app.state::<AppState>();
    let mut cfg = st.cfg.lock().unwrap();
    let mut next = cfg.clone();
    let account = next.accounts.iter_mut().find(|a| a.id == id).ok_or("Unknown account")?;
    account.label = label; account.enabled = enabled;
    let account = account.clone();
    sync_claude(&mut next, &account);
    crate::config::save_checked(&next)?;
    *cfg = next;
    drop(cfg);
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn remove_account(app: AppHandle, id: String) -> Result<(), String> {
    let st = app.state::<AppState>();
    let mut cfg = st.cfg.lock().unwrap();
    let mut next = cfg.clone();
    let account = next.accounts.iter().find(|a| a.id == id).cloned().ok_or("Unknown account")?;
    if account.id == account.provider.id() { return Err("Pause the default account instead of removing it".into()); }
    if let Some(path) = accounts::config_dir(&account) { next.excluded_account_dirs.push(path); }
    next.accounts.retain(|a| a.id != id);
    if account.provider == ProviderKind::Claude {
        next.claude_profiles.retain(|p| Some(p.id.as_str()) != id.strip_prefix("claude-"));
    }
    // Remove our own secret before forgetting its reference. Vendor files are never removed.
    let old_secret = if matches!(account.source, CredentialSource::Secret { .. }) { crate::secrets::read(&id)? } else { None };
    if old_secret.is_some() { crate::secrets::remove(&id)?; }
    if let Err(error) = crate::config::save_checked(&next) {
        if let Some(value) = old_secret { crate::secrets::set(&id, &value).map_err(|_| "Settings save failed and the account key needs to be entered again")?; }
        return Err(error);
    }
    *cfg = next;
    drop(cfg);
    st.account_usage.lock().unwrap().remove(&id);
    crate::account_usage::forget(&id);
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn connect_account(app: AppHandle, id: String) -> Result<(), String> {
    let account = app.state::<AppState>().cfg.lock().unwrap().accounts.iter().find(|a| a.id == id).cloned().ok_or("Unknown account")?;
    let path = accounts::config_dir(&account).ok_or("This account uses a key or environment variable")?;
    match account.provider {
        ProviderKind::Claude => crate::claude_auth::start_profile_login(&path, if id == "claude" { "default" } else { id.strip_prefix("claude-").ok_or("Invalid Claude account ID")? }),
        ProviderKind::Codex => crate::codex_login::start(&path, &id),
        _ => Err("Sign in through OpenCode and select its profile folder".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_is_read_only_bounded_and_deduplicates_registered_paths() {
        let home = std::env::temp_dir().join(format!("usoai-discovery-{}-{}", std::process::id(), crate::account_usage::now_ms()));
        let profile = home.join(".codex-work");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join("auth.json"), b"fake-fixture-never-a-real-token").unwrap();
        let mut cfg = Config::default();
        accounts::migrate(&mut cfg, &home);
        let env = vec![(ProviderKind::Codex, profile.clone())];
        assert_eq!(discover_at(&home, &cfg, &env).len(), 1);
        cfg.excluded_account_dirs.push(profile.clone());
        assert!(discover_at(&home, &cfg, &env).is_empty());
        cfg.excluded_account_dirs.clear();
        let mut account = accounts::primary(ProviderKind::Codex);
        account.id = "codex-work".into(); account.source = CredentialSource::ConfigDir { path: profile.clone() };
        cfg.accounts.push(account);
        assert!(discover_at(&home, &cfg, &env).is_empty());
        assert_eq!(std::fs::read(profile.join("auth.json")).unwrap(), b"fake-fixture-never-a-real-token");
        std::fs::remove_file(profile.join("auth.json")).unwrap();
        std::fs::remove_dir(profile).unwrap();
        std::fs::remove_dir(home).unwrap();
    }
}
