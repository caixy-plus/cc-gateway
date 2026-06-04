use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use chrono::Utc;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use tracing::info;

use crate::db;

pub static GLOBAL_PAIRING_MANAGER: Lazy<PairingManager> = Lazy::new(PairingManager::new);

#[derive(Debug, Clone)]
pub struct PendingPairing {
    pub pairing_code: String,
    pub platform: String,
    pub chat_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct ApprovedChat {
    pub platform: String,
    pub chat_id: String,
    pub approved_at: String,
    /// When `false`, access is suspended but the record is kept so the admin
    /// can re-enable it instantly without a new pairing handshake.
    pub enabled: bool,
}

pub struct PairingManager {
    /// Pending pairings keyed by pairing_code.
    pending: DashMap<String, PendingPairing>,
    /// Reverse index: (platform, chat_id) → pairing_code for dedup.
    by_chat: DashMap<(String, String), String>,
    /// Live per-platform `require_pairing` flag, updated from config at startup
    /// and whenever config is saved via the WebUI. Lets the toggle take effect
    /// without a daemon restart. Defaults to `true` when a platform is unset.
    require_pairing: DashMap<String, bool>,
    /// Explicitly approved chats, keyed by (platform, chat_id). Persisted to
    /// SQLite. A chat is "approved" only after an admin approves its pairing
    /// request in the WebUI — having a channel session is NOT enough.
    approved: DashMap<(String, String), ApprovedChat>,
}

impl PairingManager {
    fn new() -> Self {
        Self {
            pending: DashMap::new(),
            by_chat: DashMap::new(),
            require_pairing: DashMap::new(),
            approved: DashMap::new(),
        }
    }

    /// Update the live `require_pairing` flag for a platform.
    pub fn set_require_pairing(&self, platform: &str, required: bool) {
        self.require_pairing.insert(platform.to_string(), required);
    }

    /// Whether the given platform currently requires pairing approval.
    /// Defaults to `true` (secure-by-default) when the platform has no entry.
    pub fn require_pairing(&self, platform: &str) -> bool {
        self.require_pairing
            .get(platform)
            .map(|v| *v)
            .unwrap_or(true)
    }

    /// Load persisted pending pairings from SQLite on daemon startup.
    pub fn load_from_db(&self) {
        for (code, platform, chat_id, created_at_str) in db::load_all_pending_pairings() {
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| Utc::now());
            let entry = PendingPairing {
                pairing_code: code.clone(),
                platform: platform.clone(),
                chat_id: chat_id.clone(),
                created_at,
            };
            self.by_chat
                .insert((platform.clone(), chat_id.clone()), code.clone());
            self.pending.insert(code, entry);
        }
        for (platform, chat_id, approved_at, enabled) in db::load_all_approved_chats() {
            self.approved.insert(
                (platform.clone(), chat_id.clone()),
                ApprovedChat {
                    platform,
                    chat_id,
                    approved_at,
                    enabled,
                },
            );
        }
        info!(
            "Loaded {} pending pairings and {} approved chats from DB",
            self.pending.len(),
            self.approved.len()
        );
    }

    fn generate_code() -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let mut hasher = DefaultHasher::new();
        id.hash(&mut hasher);
        let hash = hasher.finish();
        format!("{:06}", hash % 1_000_000)
    }

    /// Whether a chat has been explicitly approved by an admin.
    ///
    /// Approval is tracked explicitly (and persisted) rather than inferred from
    /// the existence of a channel session — otherwise any chat that ever
    /// interacted (e.g. before pairing was enabled) would be silently
    /// grandfathered and the pairing gate would never trigger.
    pub fn is_approved(&self, platform: &str, chat_id: &str) -> bool {
        self.approved
            .get(&(platform.to_string(), chat_id.to_string()))
            .map(|e| e.enabled)
            .unwrap_or(false)
    }

    /// Mark a chat as approved (enabled) and persist it. Re-enables an existing
    /// record while preserving its original `approved_at`.
    pub fn mark_approved(&self, platform: &str, chat_id: &str) {
        let key = (platform.to_string(), chat_id.to_string());
        let approved_at = self
            .approved
            .get(&key)
            .map(|e| e.approved_at.clone())
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        self.approved.insert(
            key,
            ApprovedChat {
                platform: platform.to_string(),
                chat_id: chat_id.to_string(),
                approved_at: approved_at.clone(),
                enabled: true,
            },
        );
        db::insert_approved_chat(platform, chat_id, &approved_at, true);
    }

    /// Suspend or resume a kept approval record without re-pairing.
    /// Returns `false` if no approval record exists for the chat.
    pub fn set_approval_enabled(&self, platform: &str, chat_id: &str, enabled: bool) -> bool {
        let key = (platform.to_string(), chat_id.to_string());
        match self.approved.get_mut(&key) {
            Some(mut entry) => {
                entry.enabled = enabled;
                db::set_approved_chat_enabled(platform, chat_id, enabled);
                true
            }
            None => false,
        }
    }

    /// Permanently delete an approval record. The chat must pair again to regain
    /// access. Returns `false` if no record existed.
    pub fn delete_approval(&self, platform: &str, chat_id: &str) -> bool {
        let key = (platform.to_string(), chat_id.to_string());
        let existed = self.approved.remove(&key).is_some();
        if existed {
            db::delete_approved_chat(platform, chat_id);
        }
        existed
    }

    /// List all approval records (enabled and suspended) for the admin UI.
    pub fn list_approved(&self) -> Vec<ApprovedChat> {
        self.approved.iter().map(|e| e.value().clone()).collect()
    }

    /// Get existing or create a new pending pairing for a chat.
    /// Returns the pairing code.
    pub fn get_or_create_pending(&self, platform: &str, chat_id: &str) -> String {
        let key = (platform.to_string(), chat_id.to_string());
        if let Some(code) = self.by_chat.get(&key) {
            return code.clone();
        }

        // Generate a unique code (retry on collision, unlikely but safe).
        let code = loop {
            let candidate = Self::generate_code();
            if !self.pending.contains_key(&candidate) {
                break candidate;
            }
        };

        let entry = PendingPairing {
            pairing_code: code.clone(),
            platform: platform.to_string(),
            chat_id: chat_id.to_string(),
            created_at: Utc::now(),
        };

        self.by_chat.insert(key, code.clone());
        self.pending.insert(code.clone(), entry.clone());

        db::insert_pending_pairing(
            &entry.pairing_code,
            &entry.platform,
            &entry.chat_id,
            &entry.created_at.to_rfc3339(),
        );

        code
    }

    /// Approve a pending pairing by code.
    /// Returns `Some((platform, chat_id))` on success, `None` if code not found.
    pub fn approve(&self, pairing_code: &str) -> Option<(String, String)> {
        let entry = self.pending.remove(pairing_code)?;
        let p = entry.1;
        self.by_chat
            .remove(&(p.platform.clone(), p.chat_id.clone()));
        db::delete_pending_pairing(&p.pairing_code);
        self.mark_approved(&p.platform, &p.chat_id);
        let result = (p.platform, p.chat_id);
        Some(result)
    }

    /// Reject a pending pairing by code.
    /// Returns `true` if the pairing was found and removed.
    pub fn reject(&self, pairing_code: &str) -> bool {
        if let Some((_, p)) = self.pending.remove(pairing_code) {
            self.by_chat
                .remove(&(p.platform.clone(), p.chat_id.clone()));
            db::delete_pending_pairing(&p.pairing_code);
            true
        } else {
            false
        }
    }

    /// List all pending pairings.
    pub fn list_pending(&self) -> Vec<PendingPairing> {
        self.pending.iter().map(|e| e.value().clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::PairingManager;

    #[test]
    fn require_pairing_defaults_to_true_and_updates_live() {
        let mgr = PairingManager::new();
        // Secure-by-default when never configured.
        assert!(mgr.require_pairing("feishu"));
        assert!(mgr.require_pairing("telegram"));

        // Updating the flag is reflected immediately (no restart needed).
        mgr.set_require_pairing("feishu", false);
        assert!(!mgr.require_pairing("feishu"));
        assert!(mgr.require_pairing("telegram"));

        mgr.set_require_pairing("feishu", true);
        assert!(mgr.require_pairing("feishu"));
    }

    #[test]
    fn chat_is_unapproved_until_explicitly_approved() {
        let mgr = PairingManager::new();
        // A fresh chat is NOT approved, even if it has interacted before.
        assert!(!mgr.is_approved("telegram", "123"));

        // Creating a pending request does not approve it.
        let code = mgr.get_or_create_pending("telegram", "123");
        assert!(!mgr.is_approved("telegram", "123"));

        // Approving the pending code marks the chat approved.
        let approved = mgr.approve(&code);
        assert_eq!(approved, Some(("telegram".to_string(), "123".to_string())));
        assert!(mgr.is_approved("telegram", "123"));

        // Other chats remain unaffected.
        assert!(!mgr.is_approved("telegram", "456"));
        assert!(!mgr.is_approved("feishu", "123"));
    }

    #[test]
    fn suspended_approval_is_kept_and_resumable_without_repairing() {
        let mgr = PairingManager::new();
        let code = mgr.get_or_create_pending("telegram", "123");
        mgr.approve(&code);
        assert!(mgr.is_approved("telegram", "123"));

        // Suspend (取消放行): record kept, access denied.
        assert!(mgr.set_approval_enabled("telegram", "123", false));
        assert!(!mgr.is_approved("telegram", "123"));
        assert_eq!(mgr.list_approved().len(), 1);

        // Resume (重新放行): no new pairing needed.
        assert!(mgr.set_approval_enabled("telegram", "123", true));
        assert!(mgr.is_approved("telegram", "123"));

        // set_approval_enabled on an unknown chat reports failure.
        assert!(!mgr.set_approval_enabled("telegram", "999", true));

        // Deleting the record requires re-pairing.
        assert!(mgr.delete_approval("telegram", "123"));
        assert!(!mgr.is_approved("telegram", "123"));
        assert!(mgr.list_approved().is_empty());
        assert!(!mgr.delete_approval("telegram", "123"));
    }
}
