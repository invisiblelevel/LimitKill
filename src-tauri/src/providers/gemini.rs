use crate::models::{ApiProfile, ChatMessage};
use crate::providers::openai_compat::{net_err_text, ProviderCallError, ProviderOk, NETWORK_ERR};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Порядок перебора, если список моделей получить не удалось.
const FALLBACK_MODELS: &[&str] = &[
    "gemini-3.5-flash-lite",
    "gemini-3.1-flash-lite",
    "gemini-3.5-flash",
    "gemini-2.5-flash-lite",
    "gemini-flash-lite-latest",
    "gemini-flash-latest",
];

const MODELS_CACHE_TTL_SECS: u64 = 600;

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
}

#[derive(Deserialize)]
struct GeminiResponsePart {
    text: Option<String>,
    #[serde(default)]
    thought: Option<bool>,
}

#[derive(Deserialize)]
struct GeminiResponseContent {
    parts: Option<Vec<GeminiResponsePart>>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiResponseContent>,
}

#[derive(Deserialize)]
struct GeminiPromptFeedback {
    block_reason: Option<String>,
}

#[derive(Deserialize)]
struct GeminiErrorDetail {
    message: Option<String>,
    status: Option<String>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    error: Option<GeminiErrorDetail>,
    #[serde(default)]
    prompt_feedback: Option<GeminiPromptFeedback>,
}

#[derive(Deserialize)]
struct ModelsList {
    models: Option<Vec<ModelInfo>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelInfo {
    name: String,
    supported_generation_methods: Option<Vec<String>>,
}

fn perr(message: String, is_rate_limited: bool, cooldown_secs: i64) -> ProviderCallError {
    ProviderCallError::new(message, is_rate_limited, cooldown_secs)
}

fn cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.map_or(false, |flag| flag.load(Ordering::Relaxed))
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

fn strip_models_prefix(id: &str) -> &str {
    id.strip_prefix("models/").unwrap_or(id)
}

fn parse_version(id: &str) -> f32 {
    let rest = strip_models_prefix(id);
    let rest = rest.strip_prefix("gemini-").unwrap_or(rest);
    let first = rest.split('-').next().unwrap_or("");
    first.parse::<f32>().unwrap_or(0.0)
}

fn looks_like_chat_model(id: &str) -> bool {
    const BAD: [&str; 22] = [
        "image",
        "tts",
        "transcribe",
        "transcription",
        "live",
        "audio",
        "embedding",
        "embed",
        "robotics",
        "computer-use",
        "computer_use",
        "omni",
        "lyria",
        "nano-banana",
        "deep-research",
        "antigravity",
        "customtools",
        "gemma",
        "veo",
        "imagen",
        "argon",
        "carbon",
    ];
    let lower = id.to_lowercase();
    !BAD.iter().any(|bad| lower.contains(bad))
}

/// Ранжируем модели: flash-lite (дневной лимит больше) → flash → pro.
/// Внутри семейства — более новая версия выше. `-latest`-алиасы считаем самыми свежими.
fn rank_models(models: Vec<ModelInfo>) -> Vec<String> {
    let mut scored: Vec<(i32, String)> = models
        .into_iter()
        .filter_map(|m| {
            let id = strip_models_prefix(&m.name).to_string();
            let can_generate = m
                .supported_generation_methods
                .as_ref()
                .map_or(false, |v| v.iter().any(|x| x == "generateContent"));
            if !can_generate || !id.starts_with("gemini-") || !looks_like_chat_model(&id) {
                return None;
            }
            let family = if id.contains("flash-lite") {
                300
            } else if id.contains("flash") {
                200
            } else if id.contains("pro") {
                100
            } else {
                return None;
            };
            let is_alias = id.ends_with("-latest") || id == "gemini-flash-latest";
            let version = if is_alias { 99.0 } else { parse_version(&id) };
            let mut bonus = 0;
            if is_alias {
                bonus += 20;
            }
            if id.contains("preview") || id.contains("-exp") {
                bonus -= 15;
            } else {
                bonus += 10;
            }
            let score = family + (version * 10.0) as i32 + bonus;
            Some((score, id))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, id)| id).collect()
}

fn cache_key(base: &str, key: &str) -> String {
    let tail: String = key
        .chars()
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{base}|{tail}")
}

fn model_cache() -> &'static Mutex<HashMap<String, (Instant, Vec<String>)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, Vec<String>)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn fetch_models_from_api(
    client: &Client,
    base: &str,
    key: &str,
) -> Result<Vec<String>, String> {
    let resp = client
        .get(format!("{base}/models?pageSize=1000"))
        .header("x-goog-api-key", key)
        .send()
        .await
        .map_err(|e| format!("model list network error: {}", net_err_text(&e)))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "model list HTTP {}: {}",
            status.as_u16(),
            body.chars().take(300).collect::<String>()
        ));
    }
    let list: ModelsList =
        serde_json::from_str(&body).map_err(|e| format!("model list parse error: {e}"))?;
    let ranked = rank_models(list.models.unwrap_or_default());
    if ranked.is_empty() {
        Err("no suitable Gemini model available for this key".to_string())
    } else {
        Ok(ranked)
    }
}

/// Список моделей кэшируется на 10 минут: discovery больше не добавляет лишний
/// round-trip и не тратит квоту на каждый запрос.
pub async fn discover_models(
    client: &Client,
    base: &str,
    key: &str,
) -> Result<Vec<String>, String> {
    let cache_key = cache_key(base, key);
    if let Ok(guard) = model_cache().lock() {
        if let Some((at, models)) = guard.get(&cache_key) {
            if at.elapsed() < Duration::from_secs(MODELS_CACHE_TTL_SECS) && !models.is_empty() {
                return Ok(models.clone());
            }
        }
    }

    let ranked = fetch_models_from_api(client, base, key).await?;
    if let Ok(mut guard) = model_cache().lock() {
        guard.insert(cache_key, (Instant::now(), ranked.clone()));
    }
    Ok(ranked)
}

/// Список моделей для UI (без кэша-сюрпризов: возвращаем то, что реально отдаёт API).
pub async fn list_models(client: &Client, base: &str, key: &str) -> Result<Vec<String>, String> {
    fetch_models_from_api(client, base, key).await
}

fn classify_gemini_error(status: u16, body: &str, profile_name: &str) -> ProviderCallError {
    let lower = body.to_lowercase();
    let is_rl = status == 429
        || lower.contains("resource_exhausted")
        || lower.contains("rate limit")
        || lower.contains("quota")
        || lower.contains("exceeded");
    let long = lower.contains("per day")
        || lower.contains("daily")
        || lower.contains("free_tier")
        || lower.contains("current quota");
    let cooldown = if long {
        3600
    } else if is_rl {
        120
    } else {
        0
    };
    perr(
        format!(
            "Gemini HTTP {status} ({profile_name}): {}",
            body.chars().take(500).collect::<String>()
        ),
        is_rl,
        cooldown,
    )
}

fn extract_text(parsed: GeminiResponse, profile_name: &str) -> Result<String, ProviderCallError> {
    if let Some(err) = parsed.error {
        let msg = err
            .message
            .unwrap_or_else(|| "Gemini API error".to_string());
        let is_rl = err.status.as_deref() == Some("RESOURCE_EXHAUSTED");
        return Err(perr(
            format!("Gemini error ({profile_name}): {msg}"),
            is_rl,
            if is_rl { 120 } else { 0 },
        ));
    }

    if let Some(feedback) = parsed.prompt_feedback {
        if let Some(reason) = feedback.block_reason {
            return Err(perr(
                format!("Gemini blocked the prompt ({profile_name}): {reason}"),
                false,
                0,
            ));
        }
    }

    parsed
        .candidates
        .and_then(|mut c| c.pop())
        .and_then(|c| c.content)
        .and_then(|c| c.parts)
        .and_then(|parts| {
            let joined: String = parts
                .into_iter()
                .filter(|p| p.thought != Some(true))
                .filter_map(|p| p.text)
                .collect::<Vec<_>>()
                .join("");
            if joined.trim().is_empty() {
                None
            } else {
                Some(joined)
            }
        })
        .ok_or_else(|| {
            perr(
                format!("Empty Gemini response for '{profile_name}' (maybe safety filter or max tokens)"),
                false,
                0,
            )
        })
}

async fn call_generate_stream(
    client: &Client,
    profile: &ApiProfile,
    base: &str,
    key: &str,
    model: &str,
    payload: &GeminiRequest,
    on_delta: Option<&(dyn Fn(&str) + Send + Sync)>,
    cancel: Option<&AtomicBool>,
) -> Result<String, (u16, ProviderCallError)> {
    if cancelled(cancel) {
        return Err((0, ProviderCallError::cancelled()));
    }
    let url = format!("{base}/models/{model}:streamGenerateContent?alt=sse");
    let resp = tokio::select! {
        _ = wait_cancel(cancel) => return Err((0, ProviderCallError::cancelled())),
        result = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-goog-api-key", key)
            .json(payload)
            .send() => result.map_err(|e| {
                (
                    NETWORK_ERR,
                    perr(
                        format!("Gemini network error ({}): {}", profile.name, net_err_text(&e)),
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
        let body = resp.text().await.unwrap_or_default();
        eprintln!(
            "[gemini] HTTP {} model={} body={}",
            status,
            model,
            body.chars().take(700).collect::<String>()
        );
        return Err((status, classify_gemini_error(status, &body, &profile.name)));
    }

    if !content_type.contains("event-stream") {
        let body = resp.text().await.unwrap_or_default();
        let parsed: GeminiResponse = serde_json::from_str(&body).map_err(|e| {
            (
                0,
                perr(
                    format!("Gemini JSON parse error ({}): {e}", profile.name),
                    false,
                    0,
                ),
            )
        })?;
        let text = extract_text(parsed, &profile.name).map_err(|e| (0, e))?;
        if cancelled(cancel) {
            return Err((0, ProviderCallError::cancelled()));
        }
        if let Some(cb) = on_delta {
            cb(&text);
        }
        return Ok(text);
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut full = String::new();

    loop {
        let next = tokio::select! {
            _ = wait_cancel(cancel) => return Err((0, ProviderCallError::cancelled())),
            item = stream.next() => item,
        };
        let Some(chunk) = next else { break };
        if cancelled(cancel) {
            return Err((0, ProviderCallError::cancelled()));
        }
        let chunk = chunk.map_err(|e| {
            (
                NETWORK_ERR,
                perr(
                    format!(
                        "Gemini stream error ({}): {}",
                        profile.name,
                        net_err_text(&e)
                    ),
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
            let Ok(parsed) = serde_json::from_str::<GeminiResponse>(data) else {
                continue;
            };
            if let Some(err) = parsed.error {
                let msg = err
                    .message
                    .unwrap_or_else(|| "Gemini stream error".to_string());
                let is_rl = err.status.as_deref() == Some("RESOURCE_EXHAUSTED");
                return Err((
                    0,
                    perr(
                        format!("Gemini error ({}): {msg}", profile.name),
                        is_rl,
                        if is_rl { 120 } else { 0 },
                    ),
                ));
            }
            if let Some(parts) = parsed
                .candidates
                .and_then(|mut c| c.pop())
                .and_then(|c| c.content)
                .and_then(|c| c.parts)
            {
                let mut changed = false;
                for p in parts {
                    if p.thought == Some(true) {
                        continue;
                    }
                    if let Some(text) = p.text {
                        if !text.is_empty() {
                            full.push_str(&text);
                            changed = true;
                        }
                    }
                }
                if changed {
                    if let Some(cb) = on_delta {
                        cb(&full);
                    }
                }
            }
        }
    }

    // Хвост без завершающего \n: SSE-провайдеры не всегда ставят перевод строки
    // после последнего data-блока, а терять последний кусок ответа нельзя.
    let tail = buffer.trim().to_string();
    if let Some(data) = tail.strip_prefix("data:") {
        let data = data.trim();
        if !data.is_empty() && data != "[DONE]" {
            if let Ok(parsed) = serde_json::from_str::<GeminiResponse>(data) {
                if let Some(parts) = parsed
                    .candidates
                    .and_then(|mut c| c.pop())
                    .and_then(|c| c.content)
                    .and_then(|c| c.parts)
                {
                    let mut changed = false;
                    for p in parts {
                        if p.thought == Some(true) {
                            continue;
                        }
                        if let Some(text) = p.text {
                            if !text.is_empty() {
                                full.push_str(&text);
                                changed = true;
                            }
                        }
                    }
                    if changed {
                        if let Some(cb) = on_delta {
                            cb(&full);
                        }
                    }
                }
            }
        }
    }

    if full.trim().is_empty() {
        return Err((
            0,
            perr(
                format!("Empty Gemini stream response for '{}'", profile.name),
                false,
                0,
            ),
        ));
    }
    Ok(full)
}

async fn call_with_retry(
    client: &Client,
    profile: &ApiProfile,
    base: &str,
    key: &str,
    model: &str,
    payload: &GeminiRequest,
    on_delta: Option<&(dyn Fn(&str) + Send + Sync)>,
    cancel: Option<&AtomicBool>,
) -> Result<String, (u16, ProviderCallError)> {
    let mut delay = 2u64;
    let mut attempt = 0;
    loop {
        if cancelled(cancel) {
            return Err((0, ProviderCallError::cancelled()));
        }
        match call_generate_stream(client, profile, base, key, model, payload, on_delta, cancel)
            .await
        {
            Ok(t) => return Ok(t),
            // 503 «модель перегружена» — нет смысла ждать её же, быстрее перейти к следующей.
            Err((503, e)) => return Err((503, e)),
            Err((st, e)) if matches!(st, 500 | 502 | 504 | NETWORK_ERR) && attempt < 1 => {
                let _ = e;
                tokio::time::sleep(Duration::from_secs(delay)).await;
                delay *= 2;
                attempt += 1;
            }
            Err(x) => return Err(x),
        }
    }
}

fn build_contents(history: &[ChatMessage], user_prompt: &str) -> Vec<GeminiContent> {
    let mut contents: Vec<GeminiContent> = Vec::new();
    for m in history
        .iter()
        .rev()
        .take(16)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let role = match m.role.as_str() {
            "user" => "user",
            "assistant" => "model",
            _ => continue,
        };
        contents.push(GeminiContent {
            role: Some(role.to_string()),
            parts: vec![GeminiPart {
                text: m.content.clone(),
            }],
        });
    }
    contents.push(GeminiContent {
        role: Some("user".to_string()),
        parts: vec![GeminiPart {
            text: user_prompt.to_string(),
        }],
    });
    contents
}

pub async fn send_gemini(
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
    if profile.api_key.trim().is_empty() {
        return Err(perr(
            format!("Gemini API key is empty for '{}'", profile.name),
            false,
            0,
        ));
    }

    let base = profile.base_url.trim_end_matches('/');
    let key = profile.api_key.trim();

    let ranked = match discover_models(client, base, key).await {
        Ok(v) => {
            eprintln!(
                "[gemini] {} models available, top: {:?}",
                v.len(),
                &v[..v.len().min(5)]
            );
            v
        }
        Err(why) => {
            // Список моделей — вспомогательный: даже если он недоступен, пробуем известные модели.
            eprintln!("[gemini] model discovery failed: {why}; using fallback list");
            FALLBACK_MODELS.iter().map(|s| s.to_string()).collect()
        }
    };

    // Явно выбранную пользователем модель пробуем первой — иначе настройка «Модель» ничего не значит.
    let explicit = profile
        .explicit_model()
        .map(|m| strip_models_prefix(m).to_string());
    let mut order: Vec<String> = Vec::new();
    if let Some(m) = &explicit {
        order.push(m.clone());
    }
    for m in ranked {
        if !order.contains(&m) {
            order.push(m);
        }
    }
    order.truncate(10);

    let contents = build_contents(history, user_prompt);
    let sys_inst = if system_prompt.trim().is_empty() {
        None
    } else {
        Some(GeminiContent {
            role: None,
            parts: vec![GeminiPart {
                text: system_prompt.to_string(),
            }],
        })
    };
    let payload = GeminiRequest {
        contents,
        system_instruction: sys_inst,
    };

    let mut last_err: Option<ProviderCallError> = None;
    let mut any_rate_limited = false;

    for model in &order {
        if cancelled(cancel) {
            return Err(ProviderCallError::cancelled());
        }
        eprintln!("[gemini] trying model='{}'", model);
        match call_with_retry(
            client, profile, base, key, model, &payload, on_delta, cancel,
        )
        .await
        {
            Ok(text) => {
                eprintln!("[gemini] SUCCESS with model='{}'", model);
                let persist_model = explicit.as_deref() != Some(model.as_str());
                return Ok(ProviderOk {
                    text,
                    model_used: model.clone(),
                    persist_model,
                });
            }
            Err((st, e)) => {
                eprintln!(
                    "[gemini] model='{}' failed st={} : {}",
                    model, st, e.message
                );
                if e.is_cancelled() {
                    return Err(e);
                }
                any_rate_limited |= e.is_rate_limited;
                // 401/403 — ключ неверный/запрещён, перебор моделей не поможет.
                if st == 401 || st == 403 {
                    return Err(e);
                }
                last_err = Some(e);
            }
        }
    }

    let mut err = last_err.unwrap_or_else(|| {
        perr(
            "All Gemini models failed (no candidates)".to_string(),
            false,
            0,
        )
    });
    if any_rate_limited {
        err.is_rate_limited = true;
        err.cooldown_secs = err.cooldown_secs.max(3600);
    }
    Err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str) -> ModelInfo {
        ModelInfo {
            name: format!("models/{id}"),
            supported_generation_methods: Some(vec![
                "generateContent".into(),
                "countTokens".into(),
            ]),
        }
    }

    #[test]
    fn ranking_prefers_new_lite_and_skips_non_chat() {
        let models = vec![
            info("gemini-2.5-pro"),
            info("gemini-3.5-flash"),
            info("gemini-3.5-flash-lite"),
            info("gemini-3.1-flash-lite"),
            info("gemini-2.5-flash-image"),
            info("gemini-3.8-flash-tts"),
            info("gemma-4-31b-it"),
            info("gemini-robotics-er-2-preview"),
        ];
        let ranked = rank_models(models);
        assert_eq!(
            ranked.first().map(String::as_str),
            Some("gemini-3.5-flash-lite")
        );
        assert!(ranked.iter().all(|m| !m.contains("image")));
        assert!(ranked.iter().all(|m| !m.contains("tts")));
        assert!(ranked.iter().all(|m| !m.contains("gemma")));
        let pos_lite = ranked
            .iter()
            .position(|m| m == "gemini-3.5-flash-lite")
            .unwrap();
        let pos_flash = ranked.iter().position(|m| m == "gemini-3.5-flash").unwrap();
        let pos_pro = ranked.iter().position(|m| m == "gemini-2.5-pro").unwrap();
        assert!(pos_lite < pos_flash);
        assert!(pos_flash < pos_pro);
    }

    #[test]
    fn aliases_are_ranked_as_latest() {
        let models = vec![
            info("gemini-3.1-flash-lite"),
            info("gemini-flash-lite-latest"),
            info("gemini-flash-latest"),
        ];
        let ranked = rank_models(models);
        assert_eq!(
            ranked.first().map(String::as_str),
            Some("gemini-flash-lite-latest")
        );
    }

    #[test]
    fn version_parsing() {
        assert_eq!(parse_version("gemini-3.5-flash-lite"), 3.5);
        assert_eq!(parse_version("models/gemini-2.5-pro"), 2.5);
        assert_eq!(parse_version("gemini-flash-latest"), 0.0);
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
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello \"}]}}]}\n\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"world\"}]},\"finishReason\":\"STOP\"}]}"
        );
        let base = mock_sse_server(body).await;
        let client = Client::new();
        let profile = ApiProfile {
            id: "t".into(),
            name: "t".into(),
            kind: crate::models::ProviderKind::Gemini,
            api_key: "k".into(),
            base_url: base.clone(),
            model: "gemini-x".into(),
            enabled: true,
            cooldown_until: None,
            last_error: None,
            requests_count: 0,
        };
        let payload = GeminiRequest {
            contents: vec![GeminiContent {
                role: Some("user".into()),
                parts: vec![GeminiPart { text: "hi".into() }],
            }],
            system_instruction: None,
        };
        let text = call_generate_stream(
            &client, &profile, &base, "k", "gemini-x", &payload, None, None,
        )
        .await
        .expect("stream should parse");
        assert_eq!(text, "Hello world");
    }

    /// Живой тест Gemini. Запускается только с ключом:
    /// `LIMITKILL_TEST_GEMINI_KEY=... cargo test live_gemini -- --ignored --nocapture`
    /// (старое имя DEEPBRIDGE_TEST_GEMINI_KEY тоже поддерживается).
    #[tokio::test]
    #[ignore]
    async fn live_gemini_smoke() {
        let key = std::env::var("LIMITKILL_TEST_GEMINI_KEY")
            .or_else(|_| std::env::var("DEEPBRIDGE_TEST_GEMINI_KEY"))
            .expect("LIMITKILL_TEST_GEMINI_KEY is not set");
        let profile = ApiProfile {
            id: "test".into(),
            name: "test".into(),
            kind: crate::models::ProviderKind::Gemini,
            api_key: key,
            base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
            model: "gemini-3.5-flash-lite".into(),
            enabled: true,
            cooldown_until: None,
            last_error: None,
            requests_count: 0,
        };
        let client = Client::builder()
            .timeout(Duration::from_secs(90))
            .build()
            .unwrap();
        let deltas: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let on_delta = |text: &str| {
            if let Ok(mut guard) = deltas.lock() {
                guard.push(text.to_string());
            }
        };
        let ok = send_gemini(
            &client,
            &profile,
            "You are a health check. Reply with exactly: OK",
            &[],
            "ping",
            Some(&on_delta),
            None,
        )
        .await
        .expect("gemini call failed");
        eprintln!("live gemini reply: {:?} model={}", ok.text, ok.model_used);
        let deltas = deltas.into_inner().unwrap();
        eprintln!("live gemini delta callbacks: {}", deltas.len());
        assert!(!ok.text.trim().is_empty());
        assert!(!deltas.is_empty(), "streaming callback was never called");
        assert!(deltas.last().unwrap().contains(ok.text.trim()));
    }
}
