//! Provider-independent account metadata. Credentials remain in vendor storage.
use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Claude,
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
    Cursor,
    Grok,
    Copilot,
    Antigravity,
    Glm,
    #[serde(rename = "commandcode")]
    CommandCode,
    Qoder,
    #[serde(rename = "deepseek")]
    DeepseekHarness,
}

impl ProviderKind {
    pub const ALL: [Self; 11] = [
        Self::Claude,
        Self::Codex,
        Self::OpenCode,
        Self::Cursor,
        Self::Grok,
        Self::Copilot,
        Self::Antigravity,
        Self::Glm,
        Self::CommandCode,
        Self::Qoder,
        Self::DeepseekHarness,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Cursor => "cursor",
            Self::Grok => "grok",
            Self::Copilot => "copilot",
            Self::Antigravity => "antigravity",
            Self::Glm => "glm",
            Self::CommandCode => "commandcode",
            Self::Qoder => "qoder",
            Self::DeepseekHarness => "deepseek",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CredentialSource {
    Default,
    ConfigDir {
        path: PathBuf,
    },
    EnvVar {
        name: String,
    },
    /// A reference to Credential Manager, never the token itself.
    Secret {
        key: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountOrigin {
    Detected,
    Manual,
    Imported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub provider: ProviderKind,
    pub label: String,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub enabled: bool,
    pub source: CredentialSource,
    pub origin: AccountOrigin,
}

pub fn primary(provider: ProviderKind) -> Account {
    Account {
        id: provider.id().into(),
        provider,
        label: provider.id().into(),
        email: None,
        plan: None,
        enabled: true,
        source: CredentialSource::Default,
        origin: AccountOrigin::Detected,
    }
}

/// Preserves legacy primary IDs and slots. Idempotent, with no filesystem or token reads.
/// During the transition Claude's existing editor remains authoritative for its labels.
pub fn migrate(cfg: &mut Config, home: &Path) {
    for provider in ProviderKind::ALL {
        if !cfg.accounts.iter().any(|a| a.id == provider.id()) {
            cfg.accounts.push(primary(provider));
        }
    }
    for profile in &cfg.claude_profiles {
        if !valid_profile_id(&profile.id) {
            continue;
        }
        let id = if profile.id == "default" {
            "claude".to_string()
        } else {
            format!("claude-{}", profile.id)
        };
        if let Some(account) = cfg.accounts.iter_mut().find(|a| a.id == id) {
            account.label = profile.name.clone();
            account.enabled = profile.enabled;
        } else {
            let mut account = primary(ProviderKind::Claude);
            account.id = id;
            account.label = profile.name.clone();
            account.enabled = profile.enabled;
            account.source = CredentialSource::ConfigDir {
                path: home.join(format!(".claude-{}", profile.id)),
            };
            account.origin = AccountOrigin::Imported;
            cfg.accounts.push(account);
        }
    }
}

pub fn valid_profile_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub fn config_dir_at(account: &Account, home: &Path) -> Option<PathBuf> {
    match &account.source {
        CredentialSource::ConfigDir { path } => Some(path.clone()),
        CredentialSource::Default => match account.provider {
            ProviderKind::Claude => Some(home.join(".claude")),
            ProviderKind::Codex => Some(home.join(".codex")),
            ProviderKind::OpenCode => Some(home.join(".local/share/opencode")),
            _ => None,
        },
        _ => None,
    }
}

pub fn config_dir(account: &Account) -> Option<PathBuf> {
    config_dir_at(account, &dirs::home_dir()?)
}

pub fn checked_label(label: &str) -> Result<String, String> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 40 || label.chars().any(char::is_control) {
        Err("Use a name between 1 and 40 characters".into())
    } else { Ok(label.into()) }
}

pub fn next_id(accounts: &[Account], provider: ProviderKind, label: &str) -> String {
    let slug: String = label.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let slug = slug.split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-");
    let base = format!("{}-{}", provider.id(), if slug.is_empty() { "account" } else { &slug });
    let mut id = base.clone();
    let mut n = 2u64;
    while accounts.iter().any(|account| account.id == id) {
        id = format!("{base}-{n}"); n += 1;
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ClaudeProfileConfig, TraySlot};

    #[test]
    fn legacy_migration_preserves_labels_disabled_accounts_and_slots() {
        let mut cfg = Config::default();
        cfg.claude_profiles = vec![
            ClaudeProfileConfig {
                id: "default".into(),
                name: "Personal".into(),
                enabled: true,
            },
            ClaudeProfileConfig {
                id: "work".into(),
                name: "Trabajo".into(),
                enabled: false,
            },
        ];
        cfg.notch_slots = vec![TraySlot {
            provider: "codex".into(),
        }];
        migrate(&mut cfg, Path::new("fixture"));
        let personal = cfg.accounts.iter().find(|a| a.id == "claude").unwrap();
        assert_eq!(personal.label, "Personal");
        assert_eq!(personal.source, CredentialSource::Default);
        let work = cfg.accounts.iter().find(|a| a.id == "claude-work").unwrap();
        assert!(!work.enabled);
        assert_eq!(
            work.source,
            CredentialSource::ConfigDir {
                path: Path::new("fixture").join(".claude-work")
            }
        );
        assert_eq!(cfg.notch_slots[0].provider, "codex");
        let once = cfg.accounts.clone();
        migrate(&mut cfg, Path::new("fixture"));
        assert_eq!(once, cfg.accounts);
    }

    #[test]
    fn migration_does_not_override_new_account_metadata() {
        let mut cfg = Config::default();
        let mut codex = primary(ProviderKind::Codex);
        codex.label = "Mi Codex".into();
        codex.enabled = false;
        cfg.accounts.push(codex.clone());
        migrate(&mut cfg, Path::new("fixture"));
        assert_eq!(cfg.accounts.iter().find(|a| a.id == "codex"), Some(&codex));
    }

    #[test]
    fn credential_sources_accept_references_and_reject_token_fields() {
        let source = CredentialSource::Secret {
            key: "usoai/claude-work".into(),
        };
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(
            serde_json::from_str::<CredentialSource>(&json).unwrap(),
            source
        );
        assert!(serde_json::from_str::<CredentialSource>(
            r#"{"kind":"secret","key":"x","token":"must-not-be-stored"}"#
        )
        .is_err());
    }

    #[test]
    fn malicious_legacy_profiles_cannot_form_paths() {
        let mut cfg = Config::default();
        for id in ["../secret", "work/other", "work\\other", "", "."] {
            cfg.claude_profiles.push(ClaudeProfileConfig {
                id: id.into(),
                name: "x".into(),
                enabled: true,
            });
        }
        migrate(&mut cfg, Path::new("fixture"));
        assert_eq!(cfg.accounts.len(), ProviderKind::ALL.len());
    }
}
