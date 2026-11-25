mod models;
mod providers;
mod rag;
mod secrets;
mod storage;
mod web_profiles;

use chrono::Utc;
use models::{
    ApiProfile, AppConfig, BuiltContextResult, ChatAttachment, ChatMessage, ChatThread,
    ContextMode, LocalModelServer, LocaleCode, ProjectFileNode, ProviderKind, ProviderTestResult,
    WebKind, WebProfile, MAX_CONTEXT_BUDGET, MIN_CHAT_CONTEXT_BUDGET, MIN_CONTEXT_BUDGET,
};
use providers::{build_client, dispatch_chat_message, DispatchResult};
use quick_xml::escape::unescape as xml_unescape;
use quick_xml::events::Event as XmlEvent;
use quick_xml::Reader as XmlReader;
use rag::scanner::{estimate_tokens_from_text, scan_project};
use rag::{build_smart_context, generate_handoff_snapshot};
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use storage::{load_config, load_threads, save_config, save_threads, ThreadsStore};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;
use web_profiles::{
    close_web_profile_window, open_web_profile_window, probe_web_profile, reset_web_profile_data,
    run_web_prompt, window_label, ReadyState, WebBridgeState, WebOutcome,
};

pub struct BridgeState {
    pub config: Mutex<AppConfig>,
    pub threads_store: Mutex<ThreadsStore>,
    pub files: Mutex<Vec<ProjectFileNode>>,
    /// Защита от параллельных send_prompt (кнопка в UI, но команду можно вызвать и напрямую).
    pub busy: AtomicBool,
    /// Флаг отмены текущей генерации (кнопка «Стоп»).
    pub cancel: AtomicBool,
}

#[derive(serde::Serialize)]
pub struct BootstrapPayload {
    pub config: AppConfig,
    pub threads_store: ThreadsStore,
    pub files: Vec<ProjectFileNode>,
}

#[derive(Clone, serde::Serialize)]
struct PartialPayload {
    text: String,
    profile: String,
}

#[derive(Clone, serde::Serialize)]
struct WebCheckResult {
    status: String,
    detail: String,
    url: String,
    checked_at: i64,
}

/// Префикс «Авто-переключение…» нужен только для UI — в историю для моделей он попадать не должен.
fn strip_notice(content: &str) -> &str {
    let first_line = content.lines().next().unwrap_or("");
    let markers = [
        "Авто-переключение",
        "Автопереключение",
        "Auto-switch",
        "Auto switch",
        "自动切换",
    ];
    if first_line.starts_with('>') && markers.iter().any(|m| first_line.contains(m)) {
        content
            .split_once("\n\n")
            .map(|(_, rest)| rest)
            .unwrap_or(content)
    } else {
        content
    }
}

fn clean_history(msgs: &[ChatMessage]) -> Vec<ChatMessage> {
    msgs.iter()
        .map(|m| {
            let mut c = m.clone();
            if c.role == "assistant" {
                c.content = strip_notice(&c.content).to_string();
            }
            c
        })
        .collect()
}

fn make_thread_title(prompt: &str) -> String {
    let clean: String = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = clean.chars();
    let short: String = chars.by_ref().take(40).collect();
    if chars.next().is_some() {
        format!("{short}…")
    } else if short.is_empty() {
        "New Chat".to_string()
    } else {
        short
    }
}

fn active_history(store: &ThreadsStore) -> Vec<ChatMessage> {
    store
        .threads
        .iter()
        .find(|t| t.id == store.active_thread_id)
        .map(|t| {
            let mut messages = clean_history(&t.messages);
            for m in &mut messages {
                if !m.attachments.is_empty() {
                    m.content = message_content_for_model(m);
                }
            }
            messages
        })
        .unwrap_or_default()
}

fn emit_threads(app: &AppHandle, store: &ThreadsStore) {
    let _ = app.emit("threads_updated", store);
}

#[tauri::command]
fn get_bootstrap_state(state: State<'_, BridgeState>) -> Result<BootstrapPayload, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
    let store = state
        .threads_store
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let files = state.files.lock().map_err(|e| e.to_string())?.clone();
    Ok(BootstrapPayload {
        config: cfg,
        threads_store: store,
        files,
    })
}

#[tauri::command]
fn set_locale(state: State<'_, BridgeState>, locale: LocaleCode) -> Result<AppConfig, String> {
    let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
    cfg.locale = locale;
    save_config(&cfg)?;
    Ok(cfg.clone())
}

#[tauri::command]
fn create_chat_thread(state: State<'_, BridgeState>) -> Result<ThreadsStore, String> {
    let mut store = state.threads_store.lock().map_err(|e| e.to_string())?;
    let idx = store.threads.len() + 1;
    let thread = ChatThread {
        id: Uuid::new_v4().to_string(),
        title: format!("Chat #{idx}"),
        updated_at: Utc::now().timestamp(),
        messages: Vec::new(),
        archived: false,
    };
    store.active_thread_id = thread.id.clone();
    store.threads.insert(0, thread);
    save_threads(&store)?;
    Ok(store.clone())
}

#[tauri::command]
fn select_chat_thread(
    state: State<'_, BridgeState>,
    thread_id: String,
) -> Result<ThreadsStore, String> {
    let mut store = state.threads_store.lock().map_err(|e| e.to_string())?;
    if store.threads.iter().any(|t| t.id == thread_id) {
        store.active_thread_id = thread_id;
        save_threads(&store)?;
    }
    Ok(store.clone())
}

#[tauri::command]
fn delete_chat_thread(
    state: State<'_, BridgeState>,
    thread_id: String,
) -> Result<ThreadsStore, String> {
    let mut store = state.threads_store.lock().map_err(|e| e.to_string())?;
    store.threads.retain(|t| t.id != thread_id);
    if store.threads.is_empty() {
        let fresh = ChatThread {
            id: Uuid::new_v4().to_string(),
            title: "Chat #1".to_string(),
            updated_at: Utc::now().timestamp(),
            messages: Vec::new(),
            archived: false,
        };
        store.active_thread_id = fresh.id.clone();
        store.threads.push(fresh);
    } else if store.active_thread_id == thread_id {
        store.active_thread_id = store
            .threads
            .iter()
            .find(|t| !t.archived)
            .or_else(|| store.threads.first())
            .map(|t| t.id.clone())
            .unwrap_or_default();
    }
    save_threads(&store)?;
    Ok(store.clone())
}

#[tauri::command]
fn rename_chat_thread(
    state: State<'_, BridgeState>,
    thread_id: String,
    title: String,
) -> Result<ThreadsStore, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("Название чата не может быть пустым".to_string());
    }
    let mut store = state.threads_store.lock().map_err(|e| e.to_string())?;
    if let Some(t) = store.threads.iter_mut().find(|t| t.id == thread_id) {
        t.title = title.chars().take(120).collect();
        t.updated_at = Utc::now().timestamp();
    }
    save_threads(&store)?;
    Ok(store.clone())
}

#[tauri::command]
fn archive_chat_thread(
    state: State<'_, BridgeState>,
    thread_id: String,
    archived: bool,
) -> Result<ThreadsStore, String> {
    let mut store = state.threads_store.lock().map_err(|e| e.to_string())?;
    if let Some(t) = store.threads.iter_mut().find(|t| t.id == thread_id) {
        t.archived = archived;
        t.updated_at = Utc::now().timestamp();
    }
    if archived && store.active_thread_id == thread_id {
        store.active_thread_id = store
            .threads
            .iter()
            .find(|t| !t.archived)
            .or_else(|| store.threads.first())
            .map(|t| t.id.clone())
            .unwrap_or_default();
    }
    save_threads(&store)?;
    Ok(store.clone())
}

#[tauri::command]
fn truncate_chat_thread(
    state: State<'_, BridgeState>,
    thread_id: String,
    message_id: String,
) -> Result<ThreadsStore, String> {
    let mut store = state.threads_store.lock().map_err(|e| e.to_string())?;
    if let Some(t) = store.threads.iter_mut().find(|t| t.id == thread_id) {
        if let Some(index) = t.messages.iter().position(|m| m.id == message_id) {
            t.messages.truncate(index);
            t.updated_at = Utc::now().timestamp();
        }
    }
    save_threads(&store)?;
    Ok(store.clone())
}

#[tauri::command]
fn export_chat_thread(
    state: State<'_, BridgeState>,
    thread_id: String,
    path: String,
    format: String,
) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("Путь экспорта не указан".to_string());
    }
    let store = state.threads_store.lock().map_err(|e| e.to_string())?;
    let thread = store
        .threads
        .iter()
        .find(|t| t.id == thread_id)
        .ok_or_else(|| "Чат не найден".to_string())?;
    let content = if format == "markdown" {
        thread_to_markdown(thread)
    } else {
        serde_json::to_string_pretty(thread).map_err(|e| e.to_string())?
    };
    std::fs::write(&path, content).map_err(|e| format!("Не удалось записать файл: {e}"))?;
    Ok(())
}

#[tauri::command]
fn import_chat_thread(state: State<'_, BridgeState>, path: String) -> Result<ThreadsStore, String> {
    let raw =
        std::fs::read_to_string(&path).map_err(|e| format!("Не удалось прочитать файл: {e}"))?;
    let mut thread: ChatThread =
        serde_json::from_str(&raw).map_err(|e| format!("Это не JSON-экспорт чата: {e}"))?;
    if thread.messages.is_empty() && thread.title.trim().is_empty() {
        return Err("В файле нет чата".to_string());
    }
    thread.id = Uuid::new_v4().to_string();
    thread.archived = false;
    thread.updated_at = Utc::now().timestamp();
    if thread.title.trim().is_empty() {
        thread.title = "Импортированный чат".to_string();
    }
    thread.messages.truncate(400);
    let mut store = state.threads_store.lock().map_err(|e| e.to_string())?;
    store.active_thread_id = thread.id.clone();
    store.threads.insert(0, thread);
    save_threads(&store)?;
    Ok(store.clone())
}

fn thread_to_markdown(thread: &ChatThread) -> String {
    let mut out = format!("# {}\n\n", thread.title);
    for m in &thread.messages {
        let who = if m.role == "user" {
            "Вы"
        } else {
            "LimitKill"
        };
        out.push_str(&format!("## {who}\n\n{}\n\n", m.content));
    }
    out
}

#[tauri::command]
fn handoff_to_new_thread(
    app: AppHandle,
    state: State<'_, BridgeState>,
    note: Option<String>,
) -> Result<ThreadsStore, String> {
    let (project_root, files, mut store) = {
        let cfg = state.config.lock().map_err(|e| e.to_string())?;
        let files = state.files.lock().map_err(|e| e.to_string())?.clone();
        let store = state
            .threads_store
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        (cfg.project_root.clone(), files, store)
    };
    let old = store
        .threads
        .iter()
        .find(|t| t.id == store.active_thread_id)
        .cloned()
        .ok_or_else(|| "Активный чат не найден".to_string())?;
    if old.messages.is_empty() {
        return Err("В этом чате пока нет прогресса для сохранения".to_string());
    }
    let mut snapshot = generate_handoff_snapshot(project_root.as_deref(), &files, &old.messages);
    if let Some(note) = note.filter(|n| !n.trim().is_empty()) {
        snapshot.push_str(&format!("\n\n## Handoff note\n{}", note.trim()));
    }
    let now = Utc::now().timestamp();
    let thread = ChatThread {
        id: Uuid::new_v4().to_string(),
        title: format!("Продолжение: {}", old.title),
        updated_at: now,
        messages: vec![ChatMessage {
            id: Uuid::new_v4().to_string(),
            role: "assistant".to_string(),
            content: snapshot.clone(),
            provider_name: Some("LimitKill Handoff".to_string()),
            model_name: Some("Session save".to_string()),
            timestamp: now,
            tokens_approx: estimate_tokens_from_text(&snapshot),
            attachments: Vec::new(),
        }],
        archived: false,
    };
    let new_id = thread.id.clone();
    store.active_thread_id = new_id;
    store.threads.insert(0, thread);
    save_threads(&store)?;
    emit_threads(&app, &store);
    Ok(store)
}

#[tauri::command]
fn clear_active_thread(state: State<'_, BridgeState>) -> Result<ThreadsStore, String> {
    let mut store = state.threads_store.lock().map_err(|e| e.to_string())?;
    let active_id = store.active_thread_id.clone();
    if let Some(t) = store.threads.iter_mut().find(|t| t.id == active_id) {
        t.messages.clear();
        t.updated_at = Utc::now().timestamp();
    }
    save_threads(&store)?;
    Ok(store.clone())
}

async fn scan_blocking(
    root: Option<String>,
    modes: std::collections::HashMap<String, ContextMode>,
) -> Result<Vec<ProjectFileNode>, String> {
    tauri::async_runtime::spawn_blocking(move || match root {
        Some(p) => scan_project(&p, &modes),
        None => Ok(Vec::new()),
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn set_project_root(
    state: State<'_, BridgeState>,
    root_path: Option<String>,
) -> Result<Vec<ProjectFileNode>, String> {
    let modes = {
        let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
        cfg.project_root = root_path.clone();
        save_config(&cfg)?;
        cfg.file_modes.clone()
    };
    let scanned = scan_blocking(root_path, modes).await?;
    *state.files.lock().map_err(|e| e.to_string())? = scanned.clone();
    Ok(scanned)
}

#[tauri::command]
async fn rescan_project(state: State<'_, BridgeState>) -> Result<Vec<ProjectFileNode>, String> {
    let (root, modes) = {
        let cfg = state.config.lock().map_err(|e| e.to_string())?;
        (cfg.project_root.clone(), cfg.file_modes.clone())
    };
    let scanned = scan_blocking(root, modes).await?;
    *state.files.lock().map_err(|e| e.to_string())? = scanned.clone();
    Ok(scanned)
}

#[tauri::command]
fn set_file_context_mode(
    state: State<'_, BridgeState>,
    file_path: String,
    mode: ContextMode,
) -> Result<Vec<ProjectFileNode>, String> {
    {
        let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
        cfg.file_modes.insert(file_path.clone(), mode);
        save_config(&cfg)?;
    }

    let mut files_guard = state.files.lock().map_err(|e| e.to_string())?;
    if let Some(node) = files_guard.iter_mut().find(|n| n.path == file_path) {
        node.context_mode = mode;
    }
    Ok(files_guard.clone())
}

#[tauri::command]
fn build_context_preview(
    state: State<'_, BridgeState>,
    budget: Option<usize>,
) -> Result<BuiltContextResult, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?;
    let files = state.files.lock().map_err(|e| e.to_string())?;
    let limit = budget
        .unwrap_or(cfg.max_context_tokens_budget)
        .clamp(MIN_CONTEXT_BUDGET, MAX_CONTEXT_BUDGET);
    Ok(build_smart_context(
        cfg.project_root.as_deref(),
        &files,
        limit,
    ))
}

#[tauri::command]
fn upsert_api_profile(
    state: State<'_, BridgeState>,
    id: Option<String>,
    name: String,
    kind: ProviderKind,
    api_key: String,
    base_url: String,
    model: String,
    enabled: bool,
) -> Result<AppConfig, String> {
    let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Имя профиля не может быть пустым".to_string());
    }
    let base_url = if base_url.trim().is_empty() {
        match kind {
            ProviderKind::Gemini => "https://generativelanguage.googleapis.com/v1beta",
            ProviderKind::OpenRouter => "https://openrouter.ai/api/v1",
            ProviderKind::Groq => "https://api.groq.com/openai/v1",
            _ => "",
        }
        .to_string()
    } else {
        base_url.trim().to_string()
    };
    let model = model.trim().to_string();
    let api_key = api_key.trim().to_string();

    let mut updated = false;
    if let Some(existing_id) = id.as_deref() {
        if let Some(p) = cfg.api_profiles.iter_mut().find(|p| p.id == existing_id) {
            p.name = name.clone();
            p.kind = kind;
            p.api_key = api_key.clone();
            p.base_url = base_url.clone();
            p.model = model.clone();
            p.enabled = enabled;
            p.cooldown_until = None;
            p.last_error = None;
            updated = true;
        }
    }
    if !updated {
        let new_id = format!("prof-{}", Uuid::new_v4());
        cfg.api_profiles.push(ApiProfile {
            id: new_id.clone(),
            name,
            kind,
            api_key,
            base_url,
            model,
            enabled,
            cooldown_until: None,
            last_error: None,
            requests_count: 0,
        });
        if cfg.active_profile_id.is_none() {
            cfg.active_profile_id = Some(new_id);
        }
    }
    save_config(&cfg)?;
    Ok(cfg.clone())
}

#[tauri::command]
fn delete_api_profile(state: State<'_, BridgeState>, id: String) -> Result<AppConfig, String> {
    let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
    cfg.api_profiles.retain(|p| p.id != id);
    if cfg.active_profile_id.as_deref() == Some(id.as_str()) {
        cfg.active_profile_id = cfg.api_profiles.first().map(|p| p.id.clone());
    }
    save_config(&cfg)?;
    Ok(cfg.clone())
}

/// Ручной выбор модели для профиля (кнопка «Использовать» в списке моделей).
#[tauri::command]
fn set_api_profile_model(
    state: State<'_, BridgeState>,
    id: String,
    model: String,
) -> Result<AppConfig, String> {
    let model = model.trim().to_string();
    if model.is_empty() {
        return Err("Модель не может быть пустой".to_string());
    }
    let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
    let profile = cfg
        .api_profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| "Профиль не найден".to_string())?;
    profile.model = model;
    // Смена модели — это и смена квоты: старый кулдаун/ошибка больше не актуальны.
    profile.cooldown_until = None;
    profile.last_error = None;
    save_config(&cfg)?;
    Ok(cfg.clone())
}

/// Тест одного профиля без ротации: то, что нужно для диагностики «API не работает».
#[tauri::command]
async fn test_api_profile(
    state: State<'_, BridgeState>,
    id: String,
) -> Result<ProviderTestResult, String> {
    let profile = {
        let cfg = state.config.lock().map_err(|e| e.to_string())?;
        cfg.api_profiles
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| "Профиль не найден".to_string())?
    };

    let started = std::time::Instant::now();
    let mut profiles = vec![profile];
    let result = dispatch_chat_message(
        &mut profiles,
        Some(&id),
        false,
        "Ты — проверка соединения. Ответь одним словом: OK",
        &[],
        "ping",
        None,
        None,
    )
    .await;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    {
        let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
        if let Some(p) = cfg.api_profiles.iter_mut().find(|p| p.id == id) {
            let test = &profiles[0];
            p.cooldown_until = test.cooldown_until;
            p.last_error = test.last_error.clone();
            p.requests_count = test.requests_count;
            p.model = test.model.clone();
        }
        let _ = save_config(&cfg);
    }

    let test = profiles.remove(0);
    match result {
        Ok(res) => Ok(ProviderTestResult {
            ok: true,
            profile_id: id,
            profile_name: res.used_profile_name,
            model: res.used_model,
            reply: res.reply.chars().take(300).collect(),
            elapsed_ms,
            error: None,
        }),
        Err(err) => Ok(ProviderTestResult {
            ok: false,
            profile_id: id,
            profile_name: test.name,
            model: test.model,
            reply: String::new(),
            elapsed_ms,
            error: Some(err),
        }),
    }
}

/// Список моделей для формы профиля (кнопка «Определить модели»).
#[tauri::command]
async fn discover_local_models() -> Result<Vec<LocalModelServer>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1400))
        .build()
        .map_err(|e| e.to_string())?;

    let probes: [(&str, &str, &str, bool); 6] = [
        (
            "Ollama",
            "http://127.0.0.1:11434",
            "http://127.0.0.1:11434/api/tags",
            true,
        ),
        (
            "LM Studio",
            "http://127.0.0.1:1234/v1",
            "http://127.0.0.1:1234/v1/models",
            false,
        ),
        (
            "llama.cpp",
            "http://127.0.0.1:8080/v1",
            "http://127.0.0.1:8080/v1/models",
            false,
        ),
        (
            "vLLM",
            "http://127.0.0.1:8000/v1",
            "http://127.0.0.1:8000/v1/models",
            false,
        ),
        (
            "text-generation-webui",
            "http://127.0.0.1:5000/v1",
            "http://127.0.0.1:5000/v1/models",
            false,
        ),
        (
            "Jan",
            "http://127.0.0.1:1337/v1",
            "http://127.0.0.1:1337/v1/models",
            false,
        ),
    ];

    let mut found = Vec::new();
    for (name, base_url, models_url, ollama) in probes {
        let Ok(resp) = client.get(models_url).send().await else {
            continue;
        };
        if !resp.status().is_success() {
            continue;
        }
        let Ok(json) = resp.json::<serde_json::Value>().await else {
            continue;
        };
        let mut models: Vec<String> = if ollama {
            json.get("models")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        } else {
            json.get("data")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|m| m.get("id").and_then(|n| n.as_str()))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        models.sort();
        models.dedup();
        if !models.is_empty() {
            found.push(LocalModelServer {
                name: name.to_string(),
                base_url: base_url.to_string(),
                models,
            });
        }
    }
    Ok(found)
}

#[tauri::command]
async fn list_provider_models(
    kind: ProviderKind,
    base_url: String,
    api_key: String,
    model: String,
) -> Result<Vec<String>, String> {
    let client = build_client()?;
    let profile = ApiProfile {
        id: "temp".to_string(),
        name: "temp".to_string(),
        kind,
        api_key: api_key.trim().to_string(),
        base_url: if base_url.trim().is_empty() {
            match kind {
                ProviderKind::Gemini => "https://generativelanguage.googleapis.com/v1beta",
                ProviderKind::OpenRouter => "https://openrouter.ai/api/v1",
                ProviderKind::Groq => "https://api.groq.com/openai/v1",
                _ => "",
            }
            .to_string()
        } else {
            base_url.trim().to_string()
        },
        model,
        enabled: true,
        cooldown_until: None,
        last_error: None,
        requests_count: 0,
    };
    if profile.base_url.is_empty() {
        return Err("Укажи Base URL провайдера".to_string());
    }
    match kind {
        ProviderKind::Gemini => {
            if profile.api_key.is_empty() {
                return Err("Для Gemini нужен API-ключ".to_string());
            }
            providers::gemini::list_models(&client, &profile.base_url, &profile.api_key).await
        }
        ProviderKind::DeepSeekWeb => Err("У DeepSeek Web нет списка моделей".to_string()),
        _ => providers::openai_compat::fetch_models_for_profile(&client, &profile).await,
    }
}

fn extract_docx_text(path: &std::path::Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("DOCX open error: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("DOCX is not a valid zip: {e}"))?;
    let mut document = archive
        .by_name("word/document.xml")
        .map_err(|e| format!("DOCX document.xml missing: {e}"))?;
    let mut xml = String::new();
    std::io::Read::take(&mut document, 20 * 1024 * 1024)
        .read_to_string(&mut xml)
        .map_err(|e| format!("DOCX read error: {e}"))?;

    let mut reader = XmlReader::from_str(&xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut out = String::new();
    let mut in_text = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(e)) => {
                let name = e.name();
                let local = name.local_name();
                if local.as_ref() == "t" {
                    in_text = true;
                } else if local.as_ref() == "tab" {
                    out.push('\t');
                } else if local.as_ref() == "br" {
                    out.push('\n');
                }
            }
            Ok(XmlEvent::Empty(e)) => {
                let name = e.name();
                let local = name.local_name();
                if local.as_ref() == "tab" {
                    out.push('\t');
                } else if local.as_ref() == "br" {
                    out.push('\n');
                }
            }
            Ok(XmlEvent::Text(e)) => {
                if in_text {
                    out.push_str(&xml_unescape(e.as_ref()).unwrap_or_default());
                }
            }
            Ok(XmlEvent::End(e)) => {
                let name = e.name();
                let local = name.local_name();
                if local.as_ref() == "t" {
                    in_text = false;
                } else if local.as_ref() == "p" {
                    out.push('\n');
                }
            }
            Ok(XmlEvent::Eof) => break,
            Err(e) => return Err(format!("DOCX XML error: {e}")),
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn extract_pdf_text(path: &std::path::Path) -> Result<String, String> {
    pdf_extract::extract_text(path).map_err(|e| format!("PDF text extraction failed: {e}"))
}

#[tauri::command]
fn read_chat_attachments(paths: Vec<String>) -> Result<Vec<ChatAttachment>, String> {
    const PER_FILE_CHARS: usize = 48_000;
    const TOTAL_CHARS: usize = 160_000;
    let mut total = 0usize;
    let mut out = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for raw in paths {
        let path = std::path::PathBuf::from(&raw);
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(
            ext.as_str(),
            "txt"
                | "md"
                | "markdown"
                | "svelte"
                | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "rs"
                | "py"
                | "go"
                | "java"
                | "c"
                | "h"
                | "cpp"
                | "hpp"
                | "cs"
                | "json"
                | "toml"
                | "yaml"
                | "yml"
                | "html"
                | "css"
                | "scss"
                | "less"
                | "sql"
                | "sh"
                | "ps1"
                | "bat"
                | "rtf"
                | "xml"
                | "vue"
                | "docx"
                | "pdf"
        ) {
            errors.push(format!("{name}: формат пока не поддерживается"));
            continue;
        }
        let Ok(meta) = std::fs::metadata(&path) else {
            errors.push(format!("{name}: не удалось прочитать"));
            continue;
        };
        if meta.len() > 25 * 1024 * 1024 {
            errors.push(format!("{name}: файл больше 25 МБ"));
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            errors.push(format!("{name}: не удалось прочитать"));
            continue;
        };
        let mut text = if ext == "docx" {
            match extract_docx_text(&path) {
                Ok(text) => text,
                Err(e) => {
                    errors.push(format!("{name}: {e}"));
                    continue;
                }
            }
        } else if ext == "pdf" {
            match extract_pdf_text(&path) {
                Ok(text) => text,
                Err(e) => {
                    errors.push(format!("{name}: {e}"));
                    continue;
                }
            }
        } else {
            if bytes.contains(&0) {
                errors.push(format!("{name}: бинарный файл"));
                continue;
            }
            let mut text = String::from_utf8_lossy(&bytes).to_string();
            if ext == "rtf" {
                text = strip_rtf(&text);
            }
            text
        };
        let mut truncated = false;
        if text.chars().count() > PER_FILE_CHARS {
            text = text.chars().take(PER_FILE_CHARS).collect();
            truncated = true;
        }
        if total + text.chars().count() > TOTAL_CHARS {
            truncated = true;
            text = text
                .chars()
                .take(TOTAL_CHARS.saturating_sub(total))
                .collect();
        }
        if text.is_empty() {
            errors.push(format!("{name}: пустой файл"));
            continue;
        }
        total += text.chars().count();
        out.push(ChatAttachment {
            name,
            path: raw,
            size_bytes: bytes.len() as u64,
            text,
            truncated,
        });
        if total >= TOTAL_CHARS {
            break;
        }
    }

    if out.is_empty() {
        return Err(if errors.is_empty() {
            "Не выбран ни один текстовый файл".to_string()
        } else {
            errors.join("\n")
        });
    }
    Ok(out)
}

fn strip_rtf(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '\\' => {
                let mut word = String::new();
                while let Some(&n) = chars.peek() {
                    if n.is_ascii_alphabetic() {
                        word.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if word == "par" || word == "line" {
                    out.push('\n');
                } else if word == "tab" {
                    out.push('\t');
                } else if !word.is_empty() {
                    // control word: skip one optional space
                    if chars.peek() == Some(&' ') {
                        chars.next();
                    }
                } else if let Some(n) = chars.next() {
                    if n == '\'' {
                        let hex: String = chars.by_ref().take(2).collect();
                        if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                            out.push(byte as char);
                        }
                    } else {
                        out.push(n);
                    }
                }
            }
            '\r' | '\n' => {}
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn attachment_block(attachments: &[ChatAttachment]) -> String {
    let mut out = String::new();
    for a in attachments {
        let lang = std::path::Path::new(&a.name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("text");
        out.push_str(&format!(
            "\n\n### Attached file: `{}`{}\n```{}\n{}\n```",
            a.name,
            if a.truncated { " (truncated)" } else { "" },
            lang,
            a.text
        ));
    }
    out
}

fn message_content_for_model(m: &ChatMessage) -> String {
    if m.attachments.is_empty() {
        m.content.clone()
    } else {
        format!("{}{}", m.content, attachment_block(&m.attachments))
    }
}

#[tauri::command]
fn update_bridge_settings(
    state: State<'_, BridgeState>,
    active_mode: String,
    active_profile_id: Option<String>,
    active_web_profile_id: Option<String>,
    auto_rotate_on_limit: bool,
    max_context_tokens_budget: usize,
    chat_context_budget: Option<usize>,
) -> Result<AppConfig, String> {
    let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
    cfg.active_mode = if active_mode == "web" { "web" } else { "api" }.to_string();
    cfg.active_profile_id = active_profile_id;
    cfg.active_web_profile_id = active_web_profile_id;
    cfg.auto_rotate_on_limit = auto_rotate_on_limit;
    cfg.max_context_tokens_budget =
        max_context_tokens_budget.clamp(MIN_CONTEXT_BUDGET, MAX_CONTEXT_BUDGET);
    if let Some(budget) = chat_context_budget {
        cfg.chat_context_budget = budget.clamp(MIN_CHAT_CONTEXT_BUDGET, MAX_CONTEXT_BUDGET);
    }
    save_config(&cfg)?;
    Ok(cfg.clone())
}

#[tauri::command]
fn upsert_web_profile(
    state: State<'_, BridgeState>,
    id: Option<String>,
    name: String,
    kind: Option<WebKind>,
    url: String,
    notes: Option<String>,
) -> Result<AppConfig, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Имя аккаунта не может быть пустым".to_string());
    }
    let kind = kind.unwrap_or(WebKind::Deepseek);
    let url = url.trim().to_string();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("URL аккаунта должен начинаться с http(s)://".to_string());
    }
    let notes = notes.unwrap_or_default().trim().to_string();

    let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
    let mut updated = false;
    if let Some(existing_id) = id.as_deref() {
        if let Some(p) = cfg.web_profiles.iter_mut().find(|p| p.id == existing_id) {
            p.name = name.clone();
            p.kind = kind;
            p.url = url.clone();
            p.notes = notes.clone();
            updated = true;
        }
    }
    if !updated {
        let short = Uuid::new_v4().to_string();
        let short = &short[..8];
        let new_id = format!("web-{short}");
        cfg.web_profiles.push(WebProfile {
            id: new_id.clone(),
            name,
            kind,
            url,
            user_data_dir: format!("web_profile_{short}"),
            limit_hit: false,
            notes,
            limit_until: None,
            last_error: None,
            last_used_at: None,
        });
        if cfg.active_web_profile_id.is_none() {
            cfg.active_web_profile_id = Some(new_id);
        }
    }
    save_config(&cfg)?;
    Ok(cfg.clone())
}

#[tauri::command]
fn delete_web_profile(
    app: AppHandle,
    state: State<'_, BridgeState>,
    id: String,
) -> Result<AppConfig, String> {
    close_web_profile_window(&app, &id);
    let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
    cfg.web_profiles.retain(|p| p.id != id);
    if cfg.active_web_profile_id.as_deref() == Some(id.as_str()) {
        cfg.active_web_profile_id = cfg.web_profiles.first().map(|p| p.id.clone());
    }
    save_config(&cfg)?;
    Ok(cfg.clone())
}

#[tauri::command]
fn set_web_profile_limit(
    state: State<'_, BridgeState>,
    profile_id: String,
    limit_hit: bool,
) -> Result<AppConfig, String> {
    let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
    if let Some(p) = cfg.web_profiles.iter_mut().find(|p| p.id == profile_id) {
        p.limit_hit = limit_hit;
        p.limit_until = if limit_hit {
            Some(Utc::now().timestamp() + 3 * 3600)
        } else {
            None
        };
        if !limit_hit {
            p.last_error = None;
        }
    }
    save_config(&cfg)?;
    Ok(cfg.clone())
}

#[tauri::command]
async fn open_web_profile(
    app: AppHandle,
    state: State<'_, BridgeState>,
    profile_id: String,
    background: Option<bool>,
) -> Result<AppConfig, String> {
    let (prof, updated_cfg) = {
        let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
        let prof: WebProfile = cfg
            .web_profiles
            .iter()
            .find(|p| p.id == profile_id)
            .cloned()
            .ok_or_else(|| "Web-профиль не найден".to_string())?;
        cfg.active_web_profile_id = Some(profile_id);
        save_config(&cfg)?;
        (prof, cfg.clone())
    };

    open_web_profile_window(&app, &prof, background.unwrap_or(false))?;
    Ok(updated_cfg)
}

#[tauri::command]
fn close_web_profile(app: AppHandle, profile_id: String) {
    close_web_profile_window(&app, &profile_id);
}

#[tauri::command]
async fn reset_web_profile(
    app: AppHandle,
    state: State<'_, BridgeState>,
    profile_id: String,
) -> Result<AppConfig, String> {
    let prof = {
        let cfg = state.config.lock().map_err(|e| e.to_string())?;
        cfg.web_profiles
            .iter()
            .find(|p| p.id == profile_id)
            .cloned()
            .ok_or_else(|| "Web-профиль не найден".to_string())?
    };
    close_web_profile_window(&app, &profile_id);
    // Дать WebView2 отпустить файлы сессии (иногда процессу нужно больше 600 мс).
    let mut last_err: Option<String> = None;
    for _ in 0..6 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        match reset_web_profile_data(&prof) {
            Ok(()) => {
                last_err = None;
                break;
            }
            Err(err) => last_err = Some(err),
        }
    }
    if let Some(err) = last_err {
        return Err(err);
    }

    let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
    if let Some(p) = cfg.web_profiles.iter_mut().find(|p| p.id == profile_id) {
        p.limit_hit = false;
        p.limit_until = None;
        p.last_error = None;
        p.last_used_at = None;
    }
    save_config(&cfg)?;
    Ok(cfg.clone())
}

/// Проверка входа: открывает фоновое окно и возвращает статус + диагностику моста.
#[tauri::command]
async fn check_web_profile(
    app: AppHandle,
    state: State<'_, BridgeState>,
    bridge: State<'_, WebBridgeState>,
    profile_id: String,
) -> Result<WebCheckResult, String> {
    let prof = {
        let cfg = state.config.lock().map_err(|e| e.to_string())?;
        cfg.web_profiles
            .iter()
            .find(|p| p.id == profile_id)
            .cloned()
            .ok_or_else(|| "Web-профиль не найден".to_string())?
    };
    let (state_result, mut detail) = probe_web_profile(&app, &bridge, &prof).await?;
    let status = match state_result {
        ReadyState::Ready => "ready",
        ReadyState::Login => "login",
        ReadyState::Pending => "timeout",
        ReadyState::Cancelled => "timeout",
    };
    if detail.trim().is_empty() {
        detail = format!("bridge: {status}");
    }
    let url = app
        .get_webview_window(&window_label(&profile_id))
        .and_then(|w| w.url().ok())
        .map(|u| u.to_string())
        .unwrap_or_default();

    let mut cfg = state.config.lock().map_err(|e| e.to_string())?;
    if let Some(p) = cfg.web_profiles.iter_mut().find(|p| p.id == profile_id) {
        p.last_error = match status {
            "ready" => None,
            "login" => Some("Нужен логин в аккаунт".to_string()),
            _ => Some("Окно не загрузилось".to_string()),
        };
    }
    let _ = save_config(&cfg);
    Ok(WebCheckResult {
        status: status.to_string(),
        detail,
        url,
        checked_at: Utc::now().timestamp(),
    })
}

/// Отчёт из JS-моста внутри WebView DeepSeek (IPC-канал).
#[tauri::command]
fn web_report(
    bridge: State<'_, WebBridgeState>,
    profile_id: String,
    kind: String,
    text: String,
    seq: Option<u64>,
) {
    let seq = seq.unwrap_or_else(|| (Utc::now().timestamp_millis() as u64) * 100);
    bridge.push_report(&profile_id, seq, &kind, &text);
}

async fn run_web_branch(
    app: &AppHandle,
    state: &BridgeState,
    bridge: &WebBridgeState,
    cfg: &AppConfig,
    files: &[ProjectFileNode],
    sys_prompt: &str,
    history: &[ChatMessage],
    active_thread_id: &str,
    prompt: &str,
    cancel: &AtomicBool,
) -> Result<ChatMessage, String> {
    let now_ts = Utc::now().timestamp();

    // Сбрасываем флаги лимита, у которых вышел срок.
    {
        let mut cfg_guard = state.config.lock().map_err(|e| e.to_string())?;
        let mut changed = false;
        for p in cfg_guard.web_profiles.iter_mut() {
            if p.limit_hit && p.limit_until.map_or(true, |u| u <= now_ts) {
                p.limit_hit = false;
                p.limit_until = None;
                changed = true;
            }
        }
        if changed {
            let _ = save_config(&cfg_guard);
        }
    }

    let mut candidates: Vec<WebProfile> = cfg
        .web_profiles
        .iter()
        .filter(|p| !p.limited_now(now_ts))
        .cloned()
        .collect();
    if let Some(pos) = candidates
        .iter()
        .position(|p| Some(p.id.as_str()) == cfg.active_web_profile_id.as_deref())
    {
        let active = candidates.remove(pos);
        candidates.insert(0, active);
    }
    if candidates.is_empty() {
        return Err(
            "Все Web-аккаунты упёрлись в лимит. Подожди или переключись на API-режим.".to_string(),
        );
    }

    let snapshot = generate_handoff_snapshot(cfg.project_root.as_deref(), files, history);

    let mut rotated_from: Option<String> = None;
    let mut failures: Vec<String> = Vec::new();
    let mut success: Option<(WebProfile, String)> = None;

    for prof in candidates {
        let primed = bridge
            .primed
            .lock()
            .map_err(|e| e.to_string())?
            .get(&prof.id)
            .map(|t| t == active_thread_id)
            .unwrap_or(false);

        // Новый аккаунт получает полный handoff — это и есть автопередача сессии.
        // Уже «вживлённый» в этот тред аккаунт получает только сам промпт (экономим окно/лимит).
        let packet = if primed {
            prompt.to_string()
        } else {
            format!("{snapshot}\n\n## Project Context\n{sys_prompt}\n\n## Current Task\n{prompt}")
        };
        eprintln!(
            "[web] account='{}' primed={} reset_chat={} packet_chars={}",
            prof.name,
            primed,
            !primed,
            packet.chars().count()
        );

        match run_web_prompt(app, bridge, &prof, &packet, !primed, Some(cancel)).await {
            Ok(WebOutcome::Reply(text)) => {
                bridge
                    .primed
                    .lock()
                    .map_err(|e| e.to_string())?
                    .insert(prof.id.clone(), active_thread_id.to_string());
                success = Some((prof, text));
                break;
            }
            Ok(WebOutcome::Limit(reason)) => {
                if rotated_from.is_none() {
                    rotated_from = Some(prof.name.clone());
                }
                failures.push(format!("{}: лимит ({})", prof.name, reason));
                let mut cfg_guard = state.config.lock().map_err(|e| e.to_string())?;
                if let Some(p) = cfg_guard.web_profiles.iter_mut().find(|p| p.id == prof.id) {
                    p.limit_hit = true;
                    p.limit_until = Some(Utc::now().timestamp() + 3 * 3600);
                    p.last_error = Some(reason);
                }
                let _ = save_config(&cfg_guard);
            }
            Err(err) => {
                if err == "CANCELLED" {
                    // После отмены диалог DeepSeek может остаться в промежуточном
                    // состоянии — при следующем запросе безопаснее открыть новый чат.
                    if let Ok(mut primed) = bridge.primed.lock() {
                        primed.remove(&prof.id);
                    }
                    return Err("CANCELLED".to_string());
                }
                if rotated_from.is_none() {
                    rotated_from = Some(prof.name.clone());
                }
                failures.push(format!("{}: {err}", prof.name));
                let mut cfg_guard = state.config.lock().map_err(|e| e.to_string())?;
                if let Some(p) = cfg_guard.web_profiles.iter_mut().find(|p| p.id == prof.id) {
                    p.last_error = Some(err.chars().take(400).collect());
                }
                let _ = save_config(&cfg_guard);
            }
        }
    }

    let (web_prof, text) = match success {
        Some(v) => v,
        None => {
            return Err(format!(
                "Ни один Web-аккаунт не ответил:\n• {}",
                failures.join("\n• ")
            ));
        }
    };

    {
        let mut cfg_guard = state.config.lock().map_err(|e| e.to_string())?;
        cfg_guard.active_web_profile_id = Some(web_prof.id.clone());
        if let Some(p) = cfg_guard
            .web_profiles
            .iter_mut()
            .find(|p| p.id == web_prof.id)
        {
            p.last_used_at = Some(Utc::now().timestamp());
            p.last_error = None;
            p.limit_hit = false;
            p.limit_until = None;
        }
        let _ = save_config(&cfg_guard);
    }

    let content = match rotated_from {
        Some(prev) if prev != web_prof.name => format!(
            "> *Авто-переключение с **{}** на **{}***\n\n{}",
            prev, web_prof.name, text
        ),
        _ => text,
    };

    Ok(ChatMessage {
        id: Uuid::new_v4().to_string(),
        role: "assistant".to_string(),
        tokens_approx: estimate_tokens_from_text(&content),
        attachments: Vec::new(),
        content,
        provider_name: Some(web_prof.name),
        model_name: Some(format!("{} Web", web_prof.kind.default_name())),
        timestamp: Utc::now().timestamp(),
    })
}

async fn run_api_branch(
    app: &AppHandle,
    state: &BridgeState,
    cfg: &AppConfig,
    sys_prompt: &str,
    history: &[ChatMessage],
    prompt: &str,
    cancel: &AtomicBool,
) -> Result<ChatMessage, String> {
    let mut profiles = cfg.api_profiles.clone();
    let api_label = cfg
        .api_profiles
        .iter()
        .find(|p| Some(p.id.as_str()) == cfg.active_profile_id.as_deref())
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "API".to_string());

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let app_emit = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(text) = rx.recv().await {
            let _ = app_emit.emit(
                "api_partial",
                PartialPayload {
                    text,
                    profile: api_label.clone(),
                },
            );
        }
    });

    let on_delta = |s: &str| {
        let _ = tx.send(s.to_string());
    };

    let dispatch_res = dispatch_chat_message(
        &mut profiles,
        cfg.active_profile_id.as_deref(),
        cfg.auto_rotate_on_limit,
        sys_prompt,
        history,
        prompt,
        Some(&on_delta),
        Some(cancel),
    )
    .await;

    // Сохраняем кулдауны/ошибки/счётчики ДАЖЕ при провале, и только эти поля —
    // чтобы не затирать настройки, которые пользователь мог поменять за время запроса.
    {
        let mut cfg_guard = state.config.lock().map_err(|e| e.to_string())?;
        for p in &profiles {
            if let Some(t) = cfg_guard.api_profiles.iter_mut().find(|x| x.id == p.id) {
                t.cooldown_until = p.cooldown_until;
                t.last_error = p.last_error.clone();
                t.requests_count = p.requests_count;
                t.model = p.model.clone();
            }
        }
        if let Ok(r) = &dispatch_res {
            cfg_guard.active_profile_id = Some(r.used_profile_id.clone());
        }
        let _ = save_config(&cfg_guard);
    }

    let res: DispatchResult = dispatch_res?;
    let reply_content = match res.rotated_from {
        Some(ref prev) if prev != &res.used_profile_name => format!(
            "> *Авто-переключение с **{}** на **{}** (`{}`)*\n\n{}",
            prev, res.used_profile_name, res.used_model, res.reply
        ),
        _ => res.reply,
    };

    Ok(ChatMessage {
        id: Uuid::new_v4().to_string(),
        role: "assistant".to_string(),
        content: reply_content.clone(),
        provider_name: Some(res.used_profile_name),
        model_name: Some(res.used_model),
        timestamp: Utc::now().timestamp(),
        tokens_approx: estimate_tokens_from_text(&reply_content),
        attachments: Vec::new(),
    })
}

#[tauri::command]
async fn send_prompt(
    app: AppHandle,
    state: State<'_, BridgeState>,
    bridge: State<'_, WebBridgeState>,
    prompt: String,
    attachments: Option<Vec<ChatAttachment>>,
) -> Result<ThreadsStore, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("Prompt cannot be empty".to_string());
    }
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err("Предыдущий запрос ещё выполняется — дождись ответа.".to_string());
    }
    state.cancel.store(false, Ordering::SeqCst);

    let result = send_prompt_inner(
        &app,
        &state,
        &bridge,
        &prompt,
        &attachments.unwrap_or_default(),
    )
    .await;
    state.busy.store(false, Ordering::SeqCst);
    result
}

/// Остановить текущую генерацию (API-стрим или ожидание ответа DeepSeek Web).
#[tauri::command]
fn cancel_prompt(state: State<'_, BridgeState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

async fn send_prompt_inner(
    app: &AppHandle,
    state: &BridgeState,
    bridge: &WebBridgeState,
    prompt: &str,
    attachments: &[ChatAttachment],
) -> Result<ThreadsStore, String> {
    let (cfg_snap, files_snap, sys_prompt, history_snapshot, active_thread_id) = {
        let cfg = state.config.lock().map_err(|e| e.to_string())?.clone();
        let files = state.files.lock().map_err(|e| e.to_string())?.clone();
        let store = state
            .threads_store
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        let ctx = rag::build_smart_context_with_query(
            cfg.project_root.as_deref(),
            &files,
            cfg.max_context_tokens_budget,
            prompt,
        );
        let hist = active_history(&store);
        (cfg, files, ctx.system_prompt, hist, store.active_thread_id)
    };

    // Пользовательское сообщение сохраняем сразу: даже если провайдер упал,
    // вопрос не теряется, а UI получает его мгновенно (не ждёт ответа).
    let model_prompt = if attachments.is_empty() {
        prompt.to_string()
    } else {
        format!("{prompt}{}", attachment_block(attachments))
    };
    let user_msg_id = Uuid::new_v4().to_string();
    let mut was_empty = false;
    let mut old_title = String::new();
    {
        let user_msg = ChatMessage {
            id: user_msg_id.clone(),
            role: "user".to_string(),
            content: prompt.to_string(),
            provider_name: None,
            model_name: None,
            timestamp: Utc::now().timestamp(),
            tokens_approx: estimate_tokens_from_text(&model_prompt),
            attachments: attachments.to_vec(),
        };
        let mut store = state.threads_store.lock().map_err(|e| e.to_string())?;
        if let Some(thread) = store.threads.iter_mut().find(|t| t.id == active_thread_id) {
            was_empty = thread.messages.is_empty();
            old_title = thread.title.clone();
            if was_empty {
                thread.title = make_thread_title(prompt);
            }
            thread.messages.push(user_msg);
            thread.updated_at = Utc::now().timestamp();
        }
        save_threads(&store)?;
        let snapshot = store.clone();
        drop(store);
        emit_threads(app, &snapshot);
    }

    let branch_result = if cfg_snap.active_mode == "web" {
        run_web_branch(
            app,
            state,
            bridge,
            &cfg_snap,
            &files_snap,
            &sys_prompt,
            &history_snapshot,
            &active_thread_id,
            &model_prompt,
            &state.cancel,
        )
        .await
    } else {
        run_api_branch(
            app,
            state,
            &cfg_snap,
            &sys_prompt,
            &history_snapshot,
            &model_prompt,
            &state.cancel,
        )
        .await
    };

    let assistant_msg = match branch_result {
        Ok(msg) => msg,
        Err(err) if err == "CANCELLED" => {
            // Пользователь остановил генерацию: убираем вопрос из истории и
            // возвращаем текст в поле ввода (фронтенд обрабатывает CANCELLED).
            let mut store = state.threads_store.lock().map_err(|e| e.to_string())?;
            if let Some(thread) = store.threads.iter_mut().find(|t| t.id == active_thread_id) {
                thread.messages.retain(|m| m.id != user_msg_id);
                if was_empty {
                    thread.title = old_title;
                }
                thread.updated_at = Utc::now().timestamp();
            }
            save_threads(&store)?;
            let snapshot = store.clone();
            drop(store);
            emit_threads(app, &snapshot);
            return Err("CANCELLED".to_string());
        }
        Err(err) => return Err(err),
    };

    let mut store = state.threads_store.lock().map_err(|e| e.to_string())?;
    if let Some(thread) = store.threads.iter_mut().find(|t| t.id == active_thread_id) {
        thread.messages.push(assistant_msg);
        thread.updated_at = Utc::now().timestamp();
        // Не даём одному треду расти бесконечно (UI и модель всё равно видят хвост).
        const MAX_MESSAGES: usize = 400;
        if thread.messages.len() > MAX_MESSAGES {
            let excess = thread.messages.len() - MAX_MESSAGES;
            thread.messages.drain(0..excess);
        }
    }
    save_threads(&store)?;
    let snapshot = store.clone();
    drop(store);
    emit_threads(app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn create_handoff_packet(
    state: State<'_, BridgeState>,
    extra_note: Option<String>,
) -> Result<String, String> {
    let cfg = state.config.lock().map_err(|e| e.to_string())?;
    let files = state.files.lock().map_err(|e| e.to_string())?;
    let store = state.threads_store.lock().map_err(|e| e.to_string())?;
    let hist = active_history(&store);

    let ctx = build_smart_context(
        cfg.project_root.as_deref(),
        &files,
        cfg.max_context_tokens_budget,
    );
    let snapshot = generate_handoff_snapshot(cfg.project_root.as_deref(), &files, &hist);

    let note_block = extra_note
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .map(|n| format!("\n\n## Next Step / Task\n{n}"))
        .unwrap_or_default();

    Ok(format!(
        "{snapshot}{note_block}\n\n## Relevant Code Context\n{}",
        ctx.system_prompt
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cfg = load_config();
    let store = load_threads();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            use tauri::Manager;
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title("LimitKill");
            }
            eprintln!("[LimitKill] backend started");
            Ok(())
        })
        .manage(WebBridgeState::default())
        .manage(BridgeState {
            config: Mutex::new(cfg),
            threads_store: Mutex::new(store),
            // Сканирование проекта — по запросу фронтенда, чтобы не блокировать старт окна.
            files: Mutex::new(Vec::new()),
            busy: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            get_bootstrap_state,
            set_locale,
            create_chat_thread,
            select_chat_thread,
            delete_chat_thread,
            rename_chat_thread,
            archive_chat_thread,
            truncate_chat_thread,
            export_chat_thread,
            import_chat_thread,
            handoff_to_new_thread,
            clear_active_thread,
            set_project_root,
            rescan_project,
            set_file_context_mode,
            build_context_preview,
            upsert_api_profile,
            delete_api_profile,
            set_api_profile_model,
            test_api_profile,
            discover_local_models,
            list_provider_models,
            update_bridge_settings,
            upsert_web_profile,
            delete_web_profile,
            set_web_profile_limit,
            open_web_profile,
            close_web_profile,
            reset_web_profile,
            check_web_profile,
            web_report,
            send_prompt,
            read_chat_attachments,
            cancel_prompt,
            create_handoff_packet,
        ])
        .run(tauri::generate_context!())
        .expect("error while running LimitKill");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_are_stripped_only_from_assistant_headers() {
        assert_eq!(
            strip_notice("> *Авто-переключение с **A** на **B***\n\nanswer"),
            "answer"
        );
        assert_eq!(
            strip_notice("> *Auto-switch from **A** to **B***\n\nanswer"),
            "answer"
        );
        assert_eq!(strip_notice("> note\n\nanswer"), "> note\n\nanswer");
        assert_eq!(strip_notice("plain answer"), "plain answer");
    }

    #[test]
    fn titles_are_shortened() {
        assert_eq!(make_thread_title("  hello   world "), "hello world");
        let long = "a".repeat(100);
        let title = make_thread_title(&long);
        assert_eq!(title.chars().count(), 41);
        assert!(title.ends_with('…'));
        assert_eq!(make_thread_title("   "), "New Chat");
    }
}
