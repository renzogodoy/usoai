//! Independent polling and persisted retry state for Codex and OpenCode accounts.
use crate::{accounts::{Account, ProviderKind}, usage::UsageSnapshot, AppState};
use serde::{Deserialize, Serialize};
use std::{collections::{HashMap, HashSet}, path::PathBuf, sync::{Mutex, OnceLock, atomic::{AtomicU32, AtomicU64, Ordering}}, time::{Duration, SystemTime, UNIX_EPOCH}};
use tauri::{AppHandle, Emitter, Manager};

pub const POLL_MS: u64 = 300_000;

fn workers() -> &'static Mutex<HashMap<&'static str, std::thread::Thread>> {
    static WORKERS: OnceLock<Mutex<HashMap<&'static str, std::thread::Thread>>> = OnceLock::new();
    WORKERS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn reconnected() -> &'static Mutex<HashSet<String>> {
    static IDS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    IDS.get_or_init(|| Mutex::new(HashSet::new()))
}
pub fn request_refresh(provider: ProviderKind) {
    if let Some(worker) = workers().lock().unwrap().get(provider.id()) { worker.unpark(); }
}
pub fn credentials_changed(id: &str) { reconnected().lock().unwrap().insert(id.into()); }

#[derive(Default)]
pub struct Runtime {
    pub backoff_until: AtomicU64,
    pub native_retry_after: AtomicU64,
    pub consecutive_429: AtomicU32,
}

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    snapshot: UsageSnapshot,
    last_attempt_at: u64,
    native_retry_after: u64,
    consecutive_429: u32,
}

#[derive(Clone, Serialize)]
pub struct Update {
    pub account_id: String,
    pub provider: String,
    pub label: String,
    pub snapshot: UsageSnapshot,
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn path(id: &str) -> Option<PathBuf> {
    if !crate::accounts::valid_profile_id(id) { return None; }
    Some(crate::config::config_path().parent()?.join("accounts").join(format!("{id}.json")))
}

fn legacy_path(provider: ProviderKind) -> PathBuf {
    crate::config::config_path().with_file_name(if provider == ProviderKind::Codex { "codex.json" } else { "opencode-usage.json" })
}

fn load(account: &Account) -> Stored {
    if let Some(stored) = path(&account.id).and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok()) { return stored; }
    if account.id == account.provider.id() {
        if let Some(snapshot) = std::fs::read(legacy_path(account.provider)).ok()
            .and_then(|bytes| serde_json::from_slice::<UsageSnapshot>(&bytes).ok()) {
            return Stored { snapshot, ..Default::default() };
        }
    }
    Stored::default()
}

fn due(last_attempt_at: u64, backoff_until: u64, now: u64) -> bool {
    now >= backoff_until && (last_attempt_at == 0 || now.saturating_sub(last_attempt_at) >= POLL_MS)
}

fn merge(previous: &UsageSnapshot, mut next: UsageSnapshot, now: u64) -> UsageSnapshot {
    if next.backoff_until > now { next.status = "backoff".into(); }
    if next.windows.is_empty() && !previous.windows.is_empty()
        && !matches!(next.status.as_str(), "absent" | "none" | "needsAuth") {
        next.windows = previous.windows.clone();
        next.fetched_at = previous.fetched_at;
        if next.status != "backoff" { next.status = "stale".into(); }
    }
    next
}

fn publish(app: &AppHandle, account: &Account, snapshot: UsageSnapshot) {
    let st = app.state::<AppState>();
    st.account_usage.lock().unwrap().insert(account.id.clone(), snapshot.clone());
    if account.id == account.provider.id() {
        match account.provider {
            ProviderKind::Codex => *st.codex.lock().unwrap() = snapshot.clone(),
            ProviderKind::OpenCode => *st.opencode.lock().unwrap() = snapshot.clone(),
            _ => return,
        }
        let _ = app.emit(account.provider.id(), &snapshot);
    }
    let _ = app.emit("usage_update", Update { account_id: account.id.clone(), provider: account.provider.id().into(), label: account.label.clone(), snapshot });
    crate::tray::refresh_menu(app);
}

#[tauri::command]
pub fn refresh_account(app: AppHandle, id: String) -> Result<bool, String> {
    let account = app.state::<AppState>().cfg.lock().unwrap().accounts.iter()
        .find(|a| a.id == id && a.enabled && matches!(a.provider, ProviderKind::Codex | ProviderKind::OpenCode))
        .cloned().ok_or("Unknown or paused account")?;
    let saved = load(&account);
    Ok(due(saved.last_attempt_at, saved.snapshot.backoff_until, now_ms()))
}

pub fn forget(id: &str) {
    if let Some(path) = path(id) { let _ = std::fs::remove_file(path); }
}

pub fn start(app: AppHandle, provider: ProviderKind, fetch: fn(&Account, &Runtime) -> UsageSnapshot) {
    std::thread::spawn(move || {
        workers().lock().unwrap().insert(provider.id(), std::thread::current());
        let mut records: HashMap<String, (Runtime, Stored)> = HashMap::new();
        loop {
            let st = app.state::<AppState>();
            let accounts: Vec<_> = st.cfg.lock().unwrap().accounts.iter()
                .filter(|a| a.provider == provider && a.enabled && crate::accounts::valid_profile_id(&a.id)).cloned().collect();
            let ids: Vec<_> = accounts.iter().map(|a| a.id.clone()).collect();
            records.retain(|id, _| ids.contains(id));
            st.account_usage.lock().unwrap().retain(|id, _| !id.starts_with(provider.id()) || ids.contains(id));
            if !ids.iter().any(|id| id == provider.id()) {
                let mut legacy = match provider {
                    ProviderKind::Codex => st.codex.lock().unwrap(),
                    _ => st.opencode.lock().unwrap(),
                };
                if legacy.status != "absent" {
                    *legacy = UsageSnapshot { status: "absent".into(), ..Default::default() };
                    let _ = app.emit(provider.id(), &*legacy);
                }
            }
            for account in accounts {
                if provider == ProviderKind::Codex && crate::codex_login::get_codex_logins().get(&account.id).is_some_and(|login| login.busy) { continue; }
                let (runtime, saved) = records.entry(account.id.clone()).or_insert_with(|| {
                    let mut stored = load(&account);
                    if !stored.snapshot.windows.is_empty() { stored.snapshot.status = "stale".into(); }
                    if stored.snapshot.backoff_until > now_ms() { stored.snapshot.status = "backoff".into(); }
                    if stored.snapshot.status.is_empty() { stored.snapshot.status = "none".into(); }
                    let runtime = Runtime::default();
                    runtime.backoff_until.store(stored.snapshot.backoff_until, Ordering::Relaxed);
                    runtime.native_retry_after.store(stored.native_retry_after, Ordering::Relaxed);
                    runtime.consecutive_429.store(stored.consecutive_429, Ordering::Relaxed);
                    publish(&app, &account, stored.snapshot.clone());
                    (runtime, stored)
                });
                if reconnected().lock().unwrap().remove(&account.id) {
                    saved.last_attempt_at = 0;
                    runtime.native_retry_after.store(0, Ordering::Relaxed);
                }
                if !due(saved.last_attempt_at, runtime.backoff_until.load(Ordering::Relaxed), now_ms()) { continue; }
                saved.last_attempt_at = now_ms();
                // Persist the attempt floor before network or native-client I/O.
                let persisted = path(&account.id).zip(serde_json::to_vec(saved).ok())
                    .is_some_and(|(path, bytes)| crate::atomic_file::write(&path, &bytes).is_ok());
                if !persisted { crate::applog("accounts: unable to persist the retry state; request postponed"); continue; }
                let next = fetch(&account, runtime);
                // Do not publish a result for an account removed or paused during the request.
                if !st.cfg.lock().unwrap().accounts.iter().any(|a| a.id == account.id && a.enabled && a.source == account.source) { continue; }
                saved.snapshot = merge(&saved.snapshot, next, now_ms());
                saved.native_retry_after = runtime.native_retry_after.load(Ordering::Relaxed);
                saved.consecutive_429 = runtime.consecutive_429.load(Ordering::Relaxed);
                if let (Some(path), Ok(bytes)) = (path(&account.id), serde_json::to_vec(saved)) {
                    let _ = crate::atomic_file::write(&path, &bytes);
                }
                publish(&app, &account, saved.snapshot.clone());
            }
            std::thread::park_timeout(Duration::from_secs(1));
        }
    });
}

#[tauri::command]
pub fn get_account_usage(app: AppHandle) -> Vec<Update> {
    let st = app.state::<AppState>();
    let cfg = st.cfg.lock().unwrap();
    let snapshots = st.account_usage.lock().unwrap();
    cfg.accounts.iter().filter(|a| a.enabled).filter_map(|a| snapshots.get(&a.id).map(|snapshot| Update {
        account_id: a.id.clone(), provider: a.provider.id().into(), label: a.label.clone(), snapshot: snapshot.clone(),
    })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_accounts_backoff_does_not_block_another_or_reset_on_manual_refresh() {
        let blocked = Runtime::default();
        let other = Runtime::default();
        blocked.backoff_until.store(1_000_000, Ordering::Relaxed);
        assert!(!due(0, blocked.backoff_until.load(Ordering::Relaxed), 400_000));
        assert!(due(0, other.backoff_until.load(Ordering::Relaxed), 400_000));
        assert!(!due(390_000, 0, 400_000));
        assert!(due(100_000, 0, 400_000));
    }
    #[test]
    fn account_cache_names_cannot_escape_their_directory() {
        assert!(path("../auth").is_none());
        assert_ne!(path("codex-work"), path("codex-personal"));
    }
    #[test]
    fn network_failure_keeps_dated_data_but_signout_does_not() {
        let old = UsageSnapshot { status: "ok".into(), fetched_at: 100,
            windows: vec![crate::usage::LimitWindow::default()], ..Default::default() };
        let waiting = merge(&old, UsageSnapshot { backoff_until: 500, ..Default::default() }, 200);
        assert_eq!(waiting.status, "backoff");
        assert_eq!(waiting.fetched_at, 100);
        assert_eq!(waiting.windows.len(), 1);
        let rejected = merge(&old, UsageSnapshot { status: "needsAuth".into(), ..Default::default() }, 200);
        assert!(rejected.windows.is_empty());
    }
}
