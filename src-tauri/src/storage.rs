use crate::models::{AppConfig, ChatThread};
use crate::secrets;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Максимум сообщений в одном треде на диске — защита threads.json от бесконечного роста.
const MAX_MESSAGES_PER_THREAD: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadsStore {
    pub active_thread_id: String,
    pub threads: Vec<ChatThread>,
}

impl Default for ThreadsStore {
    fn default() -> Self {
        let now = Utc::now().timestamp();
        let initial = ChatThread {
            id: Uuid::new_v4().to_string(),
            title: "New Chat".to_string(),
            updated_at: now,
            messages: Vec::new(),
            archived: false,
        };
        let active_id = initial.id.clone();
        Self {
            active_thread_id: active_id,
            threads: vec![initial],
        }
    }
}

impl ThreadsStore {
    /// Убирает пустые/битые треды и гарантирует корректный active_thread_id.
    pub fn normalize(mut self) -> Self {
        self.threads.retain(|t| !t.id.is_empty());
        for t in self.threads.iter_mut() {
            if t.title.trim().is_empty() {
                t.title = "New Chat".to_string();
            }
            if t.messages.len() > MAX_MESSAGES_PER_THREAD {
                let drop_n = t.messages.len() - MAX_MESSAGES_PER_THREAD;
                t.messages.drain(0..drop_n);
            }
        }
        if self.threads.is_empty() {
            return ThreadsStore::default();
        }
        if !self.threads.iter().any(|t| t.id == self.active_thread_id) {
            self.active_thread_id = self.threads[0].id.clone();
        }
        let active_archived = self
            .threads
            .iter()
            .find(|t| t.id == self.active_thread_id)
            .map(|t| t.archived)
            .unwrap_or(false);
        if active_archived {
            if let Some(t) = self.threads.iter().find(|t| !t.archived) {
                self.active_thread_id = t.id.clone();
            }
        }
        self
    }
}

const APP_DIR_NAME: &str = "LimitKill";
const LEGACY_APP_DIR_NAME: &str = "DeepBridge";

pub fn get_app_data_dir() -> Result<PathBuf, String> {
    let base = dirs::config_dir()
        .ok_or_else(|| "Failed to resolve system config directory".to_string())?;
    let dir = base.join(APP_DIR_NAME);

    // Миграция со старого имени: переносим всю папку (включая Web-сессии),
    // чтобы не потерять ключи и логины. Если rename не удался — работаем в старой папке.
    if !dir.exists() {
        let legacy = base.join(LEGACY_APP_DIR_NAME);
        if legacy.exists() {
            match fs::rename(&legacy, &dir) {
                Ok(()) => eprintln!("[storage] migrated data dir DeepBridge -> LimitKill"),
                Err(err) => {
                    eprintln!(
                        "[storage] cannot rename legacy data dir ({err}); continuing in {}",
                        legacy.display()
                    );
                    return Ok(legacy);
                }
            }
        }
    }

    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| format!("Failed to create config dir: {e}"))?;
    }
    Ok(dir)
}

pub fn get_web_profiles_base_dir() -> Result<PathBuf, String> {
    let dir = get_app_data_dir()?.join("webview_profiles");
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create webview_profiles dir: {e}"))?;
    }
    Ok(dir)
}

/// Атомарная запись: пишем во временный файл и переименовываем — краш посреди записи не убьёт config.json
fn write_atomic(path: &Path, data: &str) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data).map_err(|e| format!("Write error: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("Rename error: {e}"))?;
    Ok(())
}

/// Если файл не парсится — не затираем молча (там API-ключи!), а откладываем копию рядом
fn backup_corrupted(path: &Path) {
    let bak = path.with_extension(format!("json.bak-{}", Utc::now().timestamp()));
    let _ = fs::copy(path, bak);
}

fn config_path() -> Result<PathBuf, String> {
    Ok(get_app_data_dir()?.join("config.json"))
}

fn threads_path() -> Result<PathBuf, String> {
    Ok(get_app_data_dir()?.join("threads.json"))
}

pub fn load_config() -> AppConfig {
    let Ok(path) = config_path() else {
        return AppConfig::default();
    };
    if !path.exists() {
        let default_cfg = AppConfig::default();
        let _ = save_config(&default_cfg);
        return default_cfg;
    }
    match fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<AppConfig>(&raw) {
            Ok(raw_cfg) => {
                let raw_web_count = raw_cfg.web_profiles.len();
                let raw_chat_budget = raw_cfg.chat_context_budget;
                let mut cfg = raw_cfg.normalize();
                let needs_save = raw_web_count != cfg.web_profiles.len()
                    || raw_chat_budget != cfg.chat_context_budget;
                let mut needs_encrypt_migration = false;
                for p in cfg.api_profiles.iter_mut() {
                    if secrets::is_encrypted(&p.api_key) {
                        match secrets::unprotect(&p.api_key) {
                            Ok(plain) => p.api_key = plain,
                            Err(err) => {
                                eprintln!(
                                    "[storage] cannot decrypt API key for profile '{}': {err}",
                                    p.name
                                );
                                p.api_key = String::new();
                                p.last_error = Some(
                                    "Не удалось расшифровать API-ключ на этой машине. Введи ключ заново."
                                        .to_string(),
                                );
                            }
                        }
                    } else if !p.api_key.trim().is_empty() {
                        needs_encrypt_migration = true;
                    }
                }
                if needs_encrypt_migration || needs_save {
                    if let Err(err) = save_config(&cfg) {
                        eprintln!("[storage] cannot migrate config: {err}");
                    }
                }
                cfg
            }
            Err(err) => {
                eprintln!("[storage] config.json parse error: {err}");
                backup_corrupted(&path);
                AppConfig::default()
            }
        },
        Err(err) => {
            eprintln!("[storage] cannot read config.json: {err}");
            AppConfig::default()
        }
    }
}

pub fn save_config(cfg: &AppConfig) -> Result<(), String> {
    let path = config_path()?;
    let mut stored = cfg.clone();
    for p in stored.api_profiles.iter_mut() {
        if !p.api_key.trim().is_empty() {
            p.api_key = secrets::protect(&p.api_key)?;
        }
    }
    let json = serde_json::to_string_pretty(&stored)
        .map_err(|e| format!("Serialize config error: {e}"))?;
    write_atomic(&path, &json)?;
    Ok(())
}

pub fn load_threads() -> ThreadsStore {
    let Ok(path) = threads_path() else {
        return ThreadsStore::default();
    };
    if !path.exists() {
        let store = ThreadsStore::default();
        let _ = save_threads(&store);
        return store;
    }
    match fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<ThreadsStore>(&raw) {
            Ok(store) => store.normalize(),
            Err(err) => {
                eprintln!("[storage] threads.json parse error: {err}");
                backup_corrupted(&path);
                ThreadsStore::default()
            }
        },
        Err(err) => {
            eprintln!("[storage] cannot read threads.json: {err}");
            ThreadsStore::default()
        }
    }
}

pub fn save_threads(store: &ThreadsStore) -> Result<(), String> {
    let path = threads_path()?;
    let json = serde_json::to_string(store).map_err(|e| format!("Serialize threads error: {e}"))?;
    write_atomic(&path, &json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ChatMessage;

    fn msg(i: usize) -> ChatMessage {
        ChatMessage {
            id: format!("m{i}"),
            role: "user".into(),
            content: "hi".into(),
            provider_name: None,
            model_name: None,
            timestamp: i as i64,
            tokens_approx: 1,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn normalize_fixes_active_thread() {
        let mut store = ThreadsStore::default();
        store.active_thread_id = "missing".into();
        let store = store.normalize();
        assert_eq!(store.active_thread_id, store.threads[0].id);
    }

    #[test]
    fn normalize_caps_messages() {
        let mut store = ThreadsStore::default();
        let id = store.threads[0].id.clone();
        store.threads[0].messages = (0..MAX_MESSAGES_PER_THREAD + 25).map(msg).collect();
        let store = store.normalize();
        assert_eq!(store.threads[0].messages.len(), MAX_MESSAGES_PER_THREAD);
        assert_eq!(store.threads[0].id, id);
    }

    #[test]
    fn empty_store_is_replaced() {
        let store = ThreadsStore {
            active_thread_id: "".into(),
            threads: vec![],
        }
        .normalize();
        assert_eq!(store.threads.len(), 1);
    }
}
