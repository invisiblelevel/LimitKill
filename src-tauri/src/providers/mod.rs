pub mod gemini;
pub mod openai_compat;

use crate::models::{ApiProfile, ChatMessage, ProviderKind};
use crate::providers::gemini::send_gemini;
use crate::providers::openai_compat::send_openai_compatible;
use chrono::Utc;
use reqwest::Client;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

pub type DeltaSink<'a> = Option<&'a (dyn Fn(&str) + Send + Sync)>;

pub fn is_cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.map_or(false, |flag| flag.load(Ordering::Relaxed))
}

#[derive(Debug, Serialize)]
pub struct DispatchResult {
    pub reply: String,
    pub used_profile_id: String,
    pub used_profile_name: String,
    pub used_model: String,
    pub rotated_from: Option<String>,
    /// Диагностика: чем закончились неудачные попытки до успешной.
    pub warnings: Vec<String>,
}

pub fn build_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        // Общий таймаут нужен для не-стриминговых запросов и discovery.
        // 300 c: длинные ответы стриминга не должны обрываться на полуслове.
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))
}

/// Порядок перебора: активный профиль первым, дальше — все остальные включённые с ключом.
pub fn candidate_order(profiles: &[ApiProfile], preferred_profile_id: Option<&str>) -> Vec<usize> {
    let mut order: Vec<usize> = Vec::new();
    if let Some(pref_id) = preferred_profile_id {
        if let Some(pos) = profiles
            .iter()
            .position(|p| p.id == pref_id && p.is_usable())
        {
            order.push(pos);
        }
    }
    for (idx, p) in profiles.iter().enumerate() {
        if p.is_usable() && !order.contains(&idx) {
            order.push(idx);
        }
    }
    order
}

fn cooldown_note(profile: &ApiProfile, now_ts: i64) -> String {
    match profile.cooldown_until {
        Some(until) if until > now_ts => {
            let mins = ((until - now_ts) + 59) / 60;
            format!("{}: кулдаун ещё ~{} мин", profile.name, mins)
        }
        _ => format!("{}: пропущен", profile.name),
    }
}

pub async fn dispatch_chat_message(
    profiles: &mut [ApiProfile],
    preferred_profile_id: Option<&str>,
    auto_rotate: bool,
    system_prompt: &str,
    history: &[ChatMessage],
    user_prompt: &str,
    on_delta: DeltaSink<'_>,
    cancel: Option<&AtomicBool>,
) -> Result<DispatchResult, String> {
    let now_ts = Utc::now().timestamp();
    let client = build_client()?;

    let order = candidate_order(profiles, preferred_profile_id);
    if order.is_empty() {
        return Err(
            "Нет ни одного включённого API-профиля с ключом. Добавь бесплатный ключ в Настройках → Провайдеры."
                .to_string(),
        );
    }

    let mut warnings: Vec<String> = Vec::new();
    let mut first_failed_name: Option<String> = None;
    let mut attempted = 0usize;

    for &idx in &order {
        if is_cancelled(cancel) {
            return Err("CANCELLED".to_string());
        }
        // Кулдаун не считается «попыткой»: при выключенной ротации логично дать шанс
        // следующему живому профилю, а не падать из-за первого остывающего.
        if profiles[idx].on_cooldown(now_ts) {
            warnings.push(cooldown_note(&profiles[idx], now_ts));
            continue;
        }
        if attempted > 0 && !auto_rotate {
            break;
        }
        attempted += 1;

        let profile_snapshot = profiles[idx].clone();
        if first_failed_name.is_none() {
            first_failed_name = Some(profile_snapshot.name.clone());
        }
        eprintln!(
            "[api] trying '{}' kind={:?} model='{}'",
            profile_snapshot.name, profile_snapshot.kind, profile_snapshot.model
        );

        let call_res = match profile_snapshot.kind {
            ProviderKind::Gemini => {
                send_gemini(
                    &client,
                    &profile_snapshot,
                    system_prompt,
                    history,
                    user_prompt,
                    on_delta,
                    cancel,
                )
                .await
            }
            ProviderKind::OpenRouter | ProviderKind::Groq | ProviderKind::CustomOpenAi => {
                send_openai_compatible(
                    &client,
                    &profile_snapshot,
                    system_prompt,
                    history,
                    user_prompt,
                    on_delta,
                    cancel,
                )
                .await
            }
            ProviderKind::DeepSeekWeb => Err(openai_compat::ProviderCallError::new(
                "DeepSeekWeb профиль используется только в режиме Web Bridge".to_string(),
                false,
                0,
            )),
        };

        match call_res {
            Ok(ok) => {
                if ok.persist_model && profiles[idx].model != ok.model_used {
                    profiles[idx].model = ok.model_used.clone();
                }
                profiles[idx].requests_count += 1;
                profiles[idx].last_error = None;
                profiles[idx].cooldown_until = None;
                let shown_model = ok.model_used.clone();

                let rotated_from = if attempted > 1 || !warnings.is_empty() {
                    first_failed_name.clone()
                } else {
                    None
                };

                return Ok(DispatchResult {
                    reply: ok.text,
                    used_profile_id: profiles[idx].id.clone(),
                    used_profile_name: profiles[idx].name.clone(),
                    used_model: shown_model,
                    rotated_from,
                    warnings,
                });
            }
            Err(err) => {
                if err.is_cancelled() {
                    return Err("CANCELLED".to_string());
                }
                eprintln!("[api] FAILED '{}': {}", profile_snapshot.name, err.message);
                warnings.push(format!("{}: {}", profile_snapshot.name, err.message));
                profiles[idx].last_error = Some(err.message.clone());
                if err.is_rate_limited && err.cooldown_secs > 0 {
                    profiles[idx].cooldown_until = Some(now_ts + err.cooldown_secs);
                }
                if !auto_rotate {
                    break;
                }
            }
        }
    }

    let mut summary = warnings.join("\n• ");
    summary.insert_str(0, "• ");
    Err(format!(
        "Ни один API-профиль не ответил.\n{summary}\n\nПроверь ключи/модели в Настройках → Провайдеры (кнопка «Тест»)."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str, key: &str, enabled: bool) -> ApiProfile {
        ApiProfile {
            id: id.into(),
            name: id.into(),
            kind: ProviderKind::OpenRouter,
            api_key: key.into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            model: "auto".into(),
            enabled,
            cooldown_until: None,
            last_error: None,
            requests_count: 0,
        }
    }

    #[test]
    fn active_profile_goes_first_then_others() {
        let profiles = vec![
            profile("a", "k1", true),
            profile("b", "k2", true),
            profile("c", "", true),
            profile("d", "k3", false),
        ];
        let order = candidate_order(&profiles, Some("b"));
        assert_eq!(order, vec![1, 0]);
        let order = candidate_order(&profiles, Some("missing"));
        assert_eq!(order, vec![0, 1]);
        let order = candidate_order(&profiles, None);
        assert_eq!(order, vec![0, 1]);
    }

    #[test]
    fn cooldown_is_reported_not_counted() {
        let now = Utc::now().timestamp();
        let mut p = profile("a", "k", true);
        p.cooldown_until = Some(now + 600);
        assert!(p.on_cooldown(now));
        assert!(cooldown_note(&p, now).contains("кулдаун"));
        p.cooldown_until = Some(now - 1);
        assert!(!p.on_cooldown(now));
    }
}
