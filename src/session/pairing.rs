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

pub struct PairingManager {
    /// Pending pairings keyed by pairing_code.
    pending: DashMap<String, PendingPairing>,
    /// Reverse index: (platform, chat_id) → pairing_code for dedup.
    by_chat: DashMap<(String, String), String>,
}

impl PairingManager {
    fn new() -> Self {
        Self {
            pending: DashMap::new(),
            by_chat: DashMap::new(),
        }
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
        info!(
            "Loaded {} pending pairings from DB",
            self.pending.len()
        );
    }

    fn generate_code() -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let mut hasher = DefaultHasher::new();
        id.hash(&mut hasher);
        let hash = hasher.finish();
        format!("{:06}", hash % 1_000_000)
    }

    /// Check whether a chat is already approved (has a channel session).
    pub fn is_approved(&self, platform: &str, chat_id: &str) -> bool {
        // Check if a channel already exists for this platform + chat_id.
        // If it does, the chat was approved (either via WebUI or because
        // pairing was not required when it first connected).
        super::channel_manager::GLOBAL_CHANNEL_SESSIONS
            .list_channels()
            .iter()
            .any(|c| c.platform == platform && c.channel_id == chat_id)
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
        self.by_chat.remove(&(p.platform.clone(), p.chat_id.clone()));
        db::delete_pending_pairing(&p.pairing_code);
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
