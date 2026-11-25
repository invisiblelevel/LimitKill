use crate::models::{ApiProfile, ChatMessage, ProviderKind};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ProviderCallError {
    pub message: String,
    pub is_rate_limited: bool,
    pub cooldown_secs: i64,
}

impl ProviderCallError {
    pub fn new(message: impl Into<String>, is_rate_limited: bool, cooldown_secs: i64) -> Self {
        Self {
            message: message.into(),
            is_rate_limited,
            cooldown_secs,
        }
    }

    pub fn cancelled() -> Self {
        Self::new("CANCELLED", false, 0)
    }

    pub fn is_cancelled(&self) -> bool {
        self.message.starts_with("CANCELLED")
    }
}

/// Успешный ответ + модель, которой он реально получен (может отличаться от сохранённой после авто-подбора)
#[derive(Debug, Clone)]
pub struct ProviderOk {
    pub text: String,
    pub model_used: String,
    /// true — сохранить model_used в профиль (модель снята с выдачи); false — разовый запасной вариант
    pub persist_model: bool,
}

/// Код «ошибка сети» (запрос не дошёл до сервера) — не HTTP-статус
pub const NETWORK_ERR: u16 = 599;
/// Код «операция отменена пользователем»
pub const CANCELLED_ERR: u16 = 598;

fn cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.map_or(false, |flag| flag.load(Ordering::Relaxed))
}

fn cancelled_error() -> (u16, ProviderCallError) {
    (CANCELLED_ERR, ProviderCallError::cancelled())
}

async fn wait_cancel(cancel: Option<&AtomicBool>) {
    if let Some(flag) = cancel {
        while !flag.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    } else {
        std::future::pending::<()>().await;
    }
}

/// Полная цепочка причин ошибки reqwest (иначе видно только «error sending request for url»)
pub fn net_err_text(e: &reqwest::Error) -> String {
    let mut s = e.to_string();
    let mut cur = std::error::Error::source(e);
    while let Some(c) = cur {
        s.push_str(" ← ");
        s.push_str(&c.to_string());
        cur = c.source();
    }
    if e.is_timeout() {
        s.push_str(" [таймаут: сервер не ответил]");
    } else if e.is_connect() {
        s.push_str(" [нет соединения с сервером: проверь интернет / VPN / прокси / фаервол]");
    }
    s
}

#[derive(Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: &'a [OpenAiMessage],
    temperature: f32,
    stream: bool,
}

#[derive(Deserialize)]
struct OpenAiChoiceMessage {
    content: Option<Value>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: Option<OpenAiChoiceMessage>,
    delta: Option<StreamDelta>,
}

#[derive(Deserialize)]
struct StreamDelta {
    content: Option<Value>,
    #[serde(default)]
    reasoning: Option<Value>,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Option<Vec<OpenAiChoice>>,
    error: Option<OpenAiErrorBody>,
}

#[derive(Deserialize)]
struct OpenAiErrorBody {
    message: Option<String>,
}

#[derive(Deserialize)]
struct OrModels {
    data: Option<Vec<OrModel>>,
}

#[derive(Deserialize)]
struct OrModel {
    id: String,
    context_length: Option<u64>,
}

fn perr(message: String, is_rate_limited: bool, cooldown_secs: i64) -> ProviderCallError {
    ProviderCallError::new(message, is_rate_limited, cooldown_secs)
}

/// 429/402 и текстовые признаки лимита → флаг + кулдаун.
/// Исчерпанный баланс/дневная квота паркуется на час, кратковременный rate-limit — на 2 минуты.
pub fn classify_rate_limit(status_code: u16, body_text: &str) -> (bool, i64) {
    let lower = body_text.to_lowercase();
    let hit = status_code == 429
        || status_code == 402
        || lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("too many requests")
        || lower.contains("quota")
        || lower.contains("insufficient")
        || lower.contains("credits")
        || lower.contains("exhausted")
        || lower.contains("usage limit");

    let long = status_code == 402
        || lower.contains("credits")
        || lower.contains("insufficient")
        || lower.contains("per day")
        || lower.contains("daily")
        || lower.contains("free_tier")
        || lower.contains("exhausted");

    let cooldown = if long {
        3600
    } else if hit {
        120
    } else {
        0
    };

    (hit, cooldown)
}

/// Достаём текст из content, который провайдеры отдают то строкой, то массивом частей.
fn content_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                p.as_str().map(|s| s.to_string()).or_else(|| {
                    p.get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                })
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn model_is_gone(status: u16, message: &str) -> bool {
    let lower = message.to_lowercase();
    status == 404
        || (status == 400
            && (lower.contains("model")
                || lower.contains("endpoint")
                || lower.contains("no endpoints")))
        || lower.contains("no endpoints found")
        || lower.contains("not a valid model")
        || lower.contains("model not found")
        || lower.contains("does not exist")
}

/// OpenRouter: free-модели регулярно пропадают. Берём актуальный список и выбираем бесплатную
/// модель из того же семейства (deepseek/…, qwen/…), а если семейства больше нет —
/// лучшую доступную :free модель вообще (иначе смена каталога полностью ломала провайдера).
pub async fn discover_openrouter_free(
    client: &Client,
    base: &str,
    old_model: &str,
) -> Result<String, String> {
    let list = fetch_openrouter_models(client, base).await?;
    let candidate = pick_best_free(&list, old_model)
        .ok_or_else(|| "no :free models currently available on OpenRouter".to_string())?;
    Ok(candidate)
}

async fn fetch_openrouter_models(client: &Client, base: &str) -> Result<Vec<OrModel>, String> {
    let resp = client
        .get(format!("{base}/models"))
        .send()
        .await
        .map_err(|e| format!("model list network error: {}", net_err_text(&e)))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "model list HTTP {}: {}",
            status.as_u16(),
            body.chars().take(200).collect::<String>()
        ));
    }
    let list: OrModels =
        serde_json::from_str(&body).map_err(|e| format!("model list parse error: {e}"))?;
    Ok(list.data.unwrap_or_default())
}

fn is_chat_free_model(id: &str) -> bool {
    let lower = id.to_lowercase();
    if !lower.ends_with(":free") {
        return false;
    }
    const NON_CHAT: [&str; 8] = [
        "embed",
        "whisper",
        "tts",
        "audio",
        "moderation",
        "rerank",
        "image",
        "dall-e",
    ];
    !NON_CHAT.iter().any(|bad| lower.contains(bad))
}

fn pick_best_free(models: &[OrModel], hint_model: &str) -> Option<String> {
    let hint = hint_model.trim();
    let hint_family = if hint.is_empty() || hint.eq_ignore_ascii_case("auto") {
        String::new()
    } else {
        hint.split('/').next().unwrap_or("").to_lowercase()
    };
    let wants_coder = hint.to_lowercase().contains("coder");

    let best = models
        .iter()
        .filter(|m| is_chat_free_model(&m.id))
        .max_by_key(|m| {
            let id = m.id.to_lowercase();
            let family_match =
                !hint_family.is_empty() && id.starts_with(&format!("{hint_family}/"));
            let coder_match = id.contains("coder") == wants_coder;
            (
                family_match as u8,
                coder_match as u8,
                m.context_length.unwrap_or(0),
            )
        })?;
    Some(best.id.clone())
}

/// Список моделей для UI («Определить модели»). Для OpenRouter — только :free.
pub async fn fetch_models_for_profile(
    client: &Client,
    profile: &ApiProfile,
) -> Result<Vec<String>, String> {
    let base = profile.base_url.trim_end_matches('/');
    match profile.kind {
        ProviderKind::OpenRouter => {
            let mut models = fetch_openrouter_models(client, base).await?;
            models.retain(|m| is_chat_free_model(&m.id));
            let mut ids: Vec<String> = models.into_iter().map(|m| m.id).collect();
            ids.sort();
            Ok(ids)
        }
        ProviderKind::Groq | ProviderKind::CustomOpenAi => {
            let mut req = client.get(format!("{base}/models"));
            if !profile.api_key.trim().is_empty() {
                req = req.bearer_auth(profile.api_key.trim());
            }
            let resp = req
                .send()
                .await
                .map_err(|e| format!("model list network error: {}", net_err_text(&e)))?;
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(format!(
                    "model list HTTP {}: {}",
                    status.as_u16(),
                    body.chars().take(200).collect::<String>()
                ));
            }
            let parsed: Value =
                serde_json::from_str(&body).map_err(|e| format!("model list parse error: {e}"))?;
            let mut ids: Vec<String> = parsed
                .get("data")
                .and_then(|d| d.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|m| m.get("id").and_then(|v| v.as_str()))
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();
            ids.sort();
            if ids.is_empty() {
                return Err("provider returned an empty model list".to_string());
            }
            Ok(ids)
        }
        ProviderKind::Gemini => Err("use the Gemini model list command".to_string()),
        ProviderKind::DeepSeekWeb => Err("DeepSeek Web has no HTTP model list".to_string()),
    }
}

fn with_auth(req: reqwest::RequestBuilder, profile: &ApiProfile) -> reqwest::RequestBuilder {
    let key = profile.api_key.trim();
    if key.is_empty() {
        req
    } else {
        req.bearer_auth(key)
    }
}

async fn call_chat_json(
    client: &Client,
    profile: &ApiProfile,
    base: &str,
    model: &str,
    msgs: &[OpenAiMessage],
    cancel: Option<&AtomicBool>,
) -> Result<String, (u16, ProviderCallError)> {
    if cancelled(cancel) {
        return Err(cancelled_error());
    }
    let endpoint = format!("{base}/chat/completions");
    let payload = OpenAiRequest {
        model,
        messages: msgs,
        temperature: 0.25,
        stream: false,
    };

    let mut req = with_auth(
        client
            .post(&endpoint)
            .header("Content-Type", "application/json"),
        profile,
    );

    if base.contains("openrouter.ai") {
        req = req
            .header("HTTP-Referer", "https://limitkill.app")
            .header("X-Title", "LimitKill");
    }

    let resp = tokio::select! {
        _ = wait_cancel(cancel) => return Err(cancelled_error()),
        result = req.json(&payload).send() => result.map_err(|e| {
            (
                NETWORK_ERR,
                perr(
                    format!("Network error ({}): {}", profile.name, net_err_text(&e)),
                    false,
                    0,
                ),
            )
        })?,
    };

    let status = resp.status().as_u16();
    let body_text = resp.text().await.unwrap_or_default();
    if cancelled(cancel) {
        return Err(cancelled_error());
    }

    if !(200..300).contains(&status) {
        eprintln!(
            "[openai-compat] HTTP {} profile='{}' model={} body={}",
            status,
            profile.name,
            model,
            body_text.chars().take(800).collect::<String>()
        );
        let (is_rl, cd) = classify_rate_limit(status, &body_text);
        return Err((
            status,
            perr(
                format!(
                    "HTTP {status} ({}): {}",
                    profile.name,
                    body_text.chars().take(400).collect::<String>()
                ),
                is_rl,
                cd,
            ),
        ));
    }

    let parsed: OpenAiResponse = serde_json::from_str(&body_text).map_err(|e| {
        (
            0,
            perr(
                format!("JSON parse error ({}): {e}", profile.name),
                false,
                0,
            ),
        )
    })?;

    if let Some(err_obj) = parsed.error {
        let err_msg = err_obj
            .message
            .unwrap_or_else(|| "Unknown provider error".to_string());
        let (is_rl, cd) = classify_rate_limit(429, &err_msg);
        return Err((
            0,
            perr(
                format!("Provider error ({}): {err_msg}", profile.name),
                is_rl,
                cd,
            ),
        ));
    }

    let reply = parsed
        .choices
        .and_then(|mut c| c.pop())
        .and_then(|c| c.message)
        .and_then(|m| m.content)
        .map(|v| content_text(&v))
        .unwrap_or_default();

    if reply.trim().is_empty() {
        return Err((
            0,
            perr(format!("Empty response from '{}'", profile.name), false, 0),
        ));
    }

    Ok(reply)
}

async fn call_chat_stream(
    client: &Client,
    profile: &ApiProfile,
    base: &str,
    model: &str,
    msgs: &[OpenAiMessage],
    on_delta: Option<&(dyn Fn(&str) + Send + Sync)>,
    cancel: Option<&AtomicBool>,
) -> Result<String, (u16, ProviderCallError)> {
    if cancelled(cancel) {
        return Err(cancelled_error());
    }
    let endpoint = format!("{base}/chat/completions");
    let payload = OpenAiRequest {
        model,
        messages: msgs,
        temperature: 0.25,
        stream: true,
    };

    let mut req = with_auth(
        client
            .post(&endpoint)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream"),
        profile,
    );

    if base.contains("openrouter.ai") {
        req = req
            .header("HTTP-Referer", "https://limitkill.app")
            .header("X-Title", "LimitKill");
    }

    let resp = tokio::select! {
        _ = wait_cancel(cancel) => return Err(cancelled_error()),
        result = req.json(&payload).send() => result.map_err(|e| {
            (
                NETWORK_ERR,
                perr(
                    format!("Network error ({}): {}", profile.name, net_err_text(&e)),
                    false,
                    0,
                ),
            )
        })?,
    };

    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    if !(200..300).contains(&status) {
        let body_text = resp.text().await.unwrap_or_default();
        eprintln!(
            "[openai-compat] HTTP {} profile='{}' model={} body={}",
            status,
            profile.name,
            model,
            body_text.chars().take(800).collect::<String>()
        );
        let (is_rl, cd) = classify_rate_limit(status, &body_text);
        return Err((
            status,
            perr(
                format!(
                    "HTTP {status} ({}): {}",
                    profile.name,
                    body_text.chars().take(400).collect::<String>()
                ),
                is_rl,
                cd,
            ),
        ));
    }

    // Провайдер проигнорировал stream и вернул обычный JSON — это нормально для custom-эндпоинтов.
    if !content_type.contains("event-stream") {
        let body_text = resp.text().await.unwrap_or_default();
        let parsed: OpenAiResponse = serde_json::from_str(&body_text).map_err(|e| {
            (
                0,
                perr(
                    format!(
                        "JSON parse error ({}): {e}; body={}",
                        profile.name,
                        body_text.chars().take(200).collect::<String>()
                    ),
                    false,
                    0,
                ),
            )
        })?;
        if let Some(err_obj) = parsed.error {
            let msg = err_obj
                .message
                .unwrap_or_else(|| "Unknown provider error".to_string());
            let (is_rl, cd) = classify_rate_limit(429, &msg);
            return Err((
                0,
                perr(
                    format!("Provider error ({}): {msg}", profile.name),
                    is_rl,
                    cd,
                ),
            ));
        }
        let reply = parsed
            .choices
            .and_then(|mut c| c.pop())
            .and_then(|c| c.message)
            .and_then(|m| m.content)
            .map(|v| content_text(&v))
            .unwrap_or_default();
        if let Some(cb) = on_delta {
            cb(&reply);
        }
        return if reply.trim().is_empty() {
            Err((
                0,
                perr(format!("Empty response from '{}'", profile.name), false, 0),
            ))
        } else {
            Ok(reply)
        };
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut full = String::new();
    let mut saw_data = false;

    loop {
        let next = tokio::select! {
            _ = wait_cancel(cancel) => return Err(cancelled_error()),
            item = stream.next() => item,
        };
        let Some(chunk) = next else { break };
        if cancelled(cancel) {
            return Err(cancelled_error());
        }
        let chunk = chunk.map_err(|e| {
            (
                NETWORK_ERR,
                perr(
                    format!("Stream error ({}): {}", profile.name, net_err_text(&e)),
                    false,
                    0,
                ),
            )
        })?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buffer.find('\n') {
            let raw_line: String = buffer.drain(..=pos).collect();
            let line = raw_line.trim_end_matches(['\r', '\n']).trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            saw_data = true;
            let parsed: OpenAiResponse = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some(err_obj) = parsed.error {
                let msg = err_obj
                    .message
                    .unwrap_or_else(|| "Provider stream error".to_string());
                let (is_rl, cd) = classify_rate_limit(429, &msg);
                return Err((
                    0,
                    perr(
                        format!("Provider error ({}): {msg}", profile.name),
                        is_rl,
                        cd,
                    ),
                ));
            }
            if let Some(choice) = parsed.choices.and_then(|mut c| c.pop()) {
                if let Some(delta) = choice.delta {
                    // reasoning-дельты не показываем в ответе, но ответ всё равно придёт в content
                    if let Some(content) = delta.content {
                        let piece = content_text(&content);
                        if !piece.is_empty() {
                            full.push_str(&piece);
                            if let Some(cb) = on_delta {
                                cb(&full);
                            }
                        }
                    }
                    let _ = delta.reasoning;
                }
            }
        }
    }

    // Хвост SSE без завершающего \n — обрабатываем отдельно, иначе последний
    // кусок ответа (иногда весь короткий ответ) терялся.
    let tail = buffer.trim().to_string();
    if let Some(data) = tail.strip_prefix("data:") {
        let data = data.trim();
        if !data.is_empty() && data != "[DONE]" {
            if let Ok(parsed) = serde_json::from_str::<OpenAiResponse>(data) {
                if let Some(err_obj) = parsed.error {
                    let msg = err_obj
                        .message
                        .unwrap_or_else(|| "Provider stream error".to_string());
                    let (is_rl, cd) = classify_rate_limit(429, &msg);
                    return Err((
                        0,
                        perr(
                            format!("Provider error ({}): {msg}", profile.name),
                            is_rl,
                            cd,
                        ),
                    ));
                }
                if let Some(choice) = parsed.choices.and_then(|mut c| c.pop()) {
                    if let Some(delta) = choice.delta {
                        if let Some(content) = delta.content {
                            let piece = content_text(&content);
                            if !piece.is_empty() {
                                full.push_str(&piece);
                                if let Some(cb) = on_delta {
                                    cb(&full);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if full.trim().is_empty() && !saw_data {
        // некоторые прокси игнорируют Content-Type и всё же вернули JSON
        let fallback = call_chat_json(client, profile, base, model, msgs, cancel).await?;
        return Ok(fallback);
    }

    if full.trim().is_empty() {
        return Err((
            0,
            perr(format!("Empty response from '{}'", profile.name), false, 0),
        ));
    }

    Ok(full)
}

pub async fn send_openai_compatible(
    client: &Client,
    profile: &ApiProfile,
    system_prompt: &str,
    history: &[ChatMessage],
    user_prompt: &str,
    on_delta: Option<&(dyn Fn(&str) + Send + Sync)>,
    cancel: Option<&AtomicBool>,
) -> Result<ProviderOk, ProviderCallError> {
    if cancelled(cancel) {
        return Err(ProviderCallError::cancelled());
    }
    if profile.api_key.trim().is_empty() && profile.kind != ProviderKind::CustomOpenAi {
        return Err(perr(
            format!("API key is empty for profile '{}'", profile.name),
            false,
            0,
        ));
    }

    let mut msgs = Vec::with_capacity(history.len() + 2);
    if !system_prompt.trim().is_empty() {
        msgs.push(OpenAiMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        });
    }

    for m in history
        .iter()
        .rev()
        .take(16)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        if m.role == "user" || m.role == "assistant" {
            msgs.push(OpenAiMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            });
        }
    }

    msgs.push(OpenAiMessage {
        role: "user".to_string(),
        content: user_prompt.to_string(),
    });

    let base = profile.base_url.trim_end_matches('/');
    let is_openrouter = profile.kind == ProviderKind::OpenRouter || base.contains("openrouter.ai");

    let mut model = profile.model.trim().to_string();
    if is_openrouter && (model.is_empty() || model.eq_ignore_ascii_case("auto")) {
        model = discover_openrouter_free(client, base, "")
            .await
            .map_err(|why| perr(format!("OpenRouter auto-pick failed: {why}"), false, 0))?;
        eprintln!("[openrouter] auto-picked free model '{}'", model);
    }
    if model.is_empty() {
        return Err(perr(
            format!("Model is empty for profile '{}'", profile.name),
            false,
            0,
        ));
    }

    let mut persist = false;
    let first = call_chat_stream(client, profile, base, &model, &msgs, on_delta, cancel).await;
    match first {
        Ok(text) => Ok(ProviderOk {
            text,
            model_used: model,
            persist_model: persist,
        }),
        Err((status, e)) => {
            if e.is_cancelled() {
                return Err(e);
            }
            // Некоторые custom-серверы не умеют stream — пробуем обычный запрос.
            let stream_unsupported =
                (status == 400 || status == 422) && e.message.to_lowercase().contains("stream");
            if stream_unsupported {
                if let Ok(text) = call_chat_json(client, profile, base, &model, &msgs, cancel).await
                {
                    return Ok(ProviderOk {
                        text,
                        model_used: model,
                        persist_model: false,
                    });
                }
            }

            if !is_openrouter || !model_is_gone(status, &e.message) {
                return Err(e);
            }
            if cancelled(cancel) {
                return Err(ProviderCallError::cancelled());
            }
            eprintln!(
                "[openrouter] model '{}' is gone, discovering a replacement…",
                model
            );
            let new_model = discover_openrouter_free(client, base, &model)
                .await
                .map_err(|why| {
                    perr(format!("{} | auto-pick failed: {why}", e.message), false, 0)
                })?;
            if new_model == model {
                return Err(e);
            }
            // Не подменяем молча конкретную выбранную модель моделью другого семейства:
            // профиль «Nemotron» не должен превращаться в «flint» после временного 404.
            let old_family = model.split('/').next().unwrap_or("");
            let new_family = new_model.split('/').next().unwrap_or("");
            persist = old_family == new_family;
            call_chat_stream(client, profile, base, &new_model, &msgs, on_delta, cancel)
                .await
                .map(|text| ProviderOk {
                    text,
                    model_used: new_model.clone(),
                    persist_model: persist,
                })
                .map_err(|(_, e2)| e2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_classification() {
        assert_eq!(classify_rate_limit(429, "Too Many Requests").0, true);
        assert_eq!(classify_rate_limit(402, "insufficient credits").1, 3600);
        assert_eq!(
            classify_rate_limit(429, "quota exceeded: requests per day").1,
            3600
        );
        assert_eq!(classify_rate_limit(500, "server error"), (false, 0));
    }

    #[test]
    fn content_text_handles_string_and_parts() {
        assert_eq!(content_text(&Value::String("hi".into())), "hi");
        let parts = serde_json::json!([{"type":"text","text":"a"},{"type":"text","text":"b"}]);
        assert_eq!(content_text(&parts), "ab");
    }

    #[test]
    fn free_model_filter_and_pick() {
        assert!(is_chat_free_model("deepseek/deepseek-chat:free"));
        assert!(!is_chat_free_model("openai/text-embedding-3:free"));
        assert!(!is_chat_free_model("deepseek/deepseek-chat"));
        let models = vec![
            OrModel {
                id: "deepseek/other:free".into(),
                context_length: Some(1000),
            },
            OrModel {
                id: "deepseek/deepseek-coder:free".into(),
                context_length: Some(2000),
            },
            OrModel {
                id: "qwen/qwen3-coder:free".into(),
                context_length: Some(9000),
            },
        ];
        assert_eq!(
            pick_best_free(&models, "deepseek/deepseek-coder:free").as_deref(),
            Some("deepseek/deepseek-coder:free")
        );
        // семейства нет в каталоге — не падаем, а берём лучшую :free вообще
        assert_eq!(
            pick_best_free(&models, "openai/gpt-4o:free").as_deref(),
            Some("deepseek/other:free")
        );
    }

    #[test]
    fn model_gone_detection() {
        assert!(model_is_gone(404, "not found"));
        assert!(model_is_gone(400, "not a valid model"));
        assert!(!model_is_gone(429, "rate limit"));
    }

    async fn mock_sse_server(body: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 8192];
            let _ = socket.read(&mut buf).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn stream_parser_reads_final_line_without_newline() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"world\"}}]}"
        );
        let base = mock_sse_server(body).await;
        let client = Client::new();
        let profile = ApiProfile {
            id: "t".into(),
            name: "t".into(),
            kind: ProviderKind::CustomOpenAi,
            api_key: "k".into(),
            base_url: base.clone(),
            model: "m".into(),
            enabled: true,
            cooldown_until: None,
            last_error: None,
            requests_count: 0,
        };
        let msgs = vec![OpenAiMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let text = call_chat_stream(&client, &profile, &base, "m", &msgs, None, None)
            .await
            .expect("stream should parse");
        assert_eq!(text, "Hello world");
    }

    /// Живая проверка каталога OpenRouter (без ключа): /models доступен публично.
    #[tokio::test]
    #[ignore]
    async fn live_openrouter_discovery() {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();
        let picked = discover_openrouter_free(&client, "https://openrouter.ai/api/v1", "auto")
            .await
            .expect("openrouter discovery failed");
        eprintln!("live openrouter picked: {picked}");
        assert!(picked.ends_with(":free"));
    }

    /// Живой тест конкретной :free модели.
    /// `LIMITKILL_TEST_OPENROUTER_KEY=sk-or-v1-... cargo test live_openrouter_completion -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_openrouter_completion() {
        let key = std::env::var("LIMITKILL_TEST_OPENROUTER_KEY")
            .or_else(|_| std::env::var("DEEPBRIDGE_TEST_OPENROUTER_KEY"))
            .expect("LIMITKILL_TEST_OPENROUTER_KEY is not set");
        let model = std::env::var("LIMITKILL_TEST_OPENROUTER_MODEL")
            .or_else(|_| std::env::var("DEEPBRIDGE_TEST_OPENROUTER_MODEL"))
            .unwrap_or_else(|_| "nvidia/nemotron-3.5-super-vl:free".to_string());
        let profile = ApiProfile {
            id: "test".into(),
            name: "test".into(),
            kind: ProviderKind::OpenRouter,
            api_key: key,
            base_url: "https://openrouter.ai/api/v1".into(),
            model: model.clone(),
            enabled: true,
            cooldown_until: None,
            last_error: None,
            requests_count: 0,
        };
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap();
        let deltas: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let on_delta = |text: &str| {
            if let Ok(mut guard) = deltas.lock() {
                guard.push(text.to_string());
            }
        };
        let ok = send_openai_compatible(
            &client,
            &profile,
            "You are a health check. Reply with exactly: OK",
            &[],
            "ping",
            Some(&on_delta),
            None,
        )
        .await
        .expect("openrouter call failed");
        eprintln!(
            "live openrouter reply: {:?} model={}",
            ok.text, ok.model_used
        );
        assert!(!ok.text.trim().is_empty());
        assert!(!deltas.into_inner().unwrap().is_empty());
    }
}
