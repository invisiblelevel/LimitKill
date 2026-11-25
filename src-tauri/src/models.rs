use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_CONTEXT_BUDGET: usize = 60_000;
pub const MIN_CONTEXT_BUDGET: usize = 2_048;
pub const MAX_CONTEXT_BUDGET: usize = 1_000_000;
pub const DEFAULT_CHAT_CONTEXT_BUDGET: usize = 128_000;
pub const MIN_CHAT_CONTEXT_BUDGET: usize = 8_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LocaleCode {
    Ru,
    En,
    Zh,
}

impl Default for LocaleCode {
    fn default() -> Self {
        Self::Ru
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenRouter,
    Gemini,
    Groq,
    CustomOpenAi,
    DeepSeekWeb,
}

impl ProviderKind {
    pub fn is_openai_compatible(self) -> bool {
        matches!(self, Self::OpenRouter | Self::Groq | Self::CustomOpenAi)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiProfile {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub enabled: bool,
    pub cooldown_until: Option<i64>,
    pub last_error: Option<String>,
    pub requests_count: u64,
}

impl ApiProfile {
    /// Профиль реально можно вызвать: включён и есть ключ.
    /// Custom OpenAI-совместимые локальные серверы (Ollama/LM Studio) работают без ключа.
    pub fn is_usable(&self) -> bool {
        self.enabled && (self.kind == ProviderKind::CustomOpenAi || !self.api_key.trim().is_empty())
    }

    /// Модель задана явно (а не пустая / "auto").
    pub fn explicit_model(&self) -> Option<&str> {
        let m = self.model.trim();
        if m.is_empty() || m.eq_ignore_ascii_case("auto") {
            None
        } else {
            Some(m)
        }
    }

    pub fn on_cooldown(&self, now_ts: i64) -> bool {
        matches!(self.cooldown_until, Some(until) if until > now_ts)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum WebKind {
    #[default]
    Deepseek,
    Gemini,
    Qwen,
    Claude,
    ChatGpt,
    /// Kept only so old configs can be parsed; profiles are removed in normalize.
    Kimi,
}

impl WebKind {
    pub fn default_url(self) -> &'static str {
        match self {
            Self::Deepseek => "https://chat.deepseek.com",
            Self::Gemini => "https://gemini.google.com/app",
            Self::Qwen => "https://chat.qwen.ai/",
            Self::Claude => "https://claude.ai/new",
            Self::ChatGpt => "https://chatgpt.com/",
            Self::Kimi => "https://www.kimi.com/",
        }
    }

    pub fn default_name(self) -> &'static str {
        match self {
            Self::Deepseek => "DeepSeek",
            Self::Gemini => "Google Gemini",
            Self::Qwen => "Qwen",
            Self::Claude => "Claude",
            Self::ChatGpt => "ChatGPT",
            Self::Kimi => "Kimi",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: WebKind,
    pub url: String,
    pub user_data_dir: String,
    #[serde(default)]
    pub limit_hit: bool,
    #[serde(default)]
    pub notes: String,
    /// Unix-время, до которого аккаунт считается упёршимся в лимит
    #[serde(default)]
    pub limit_until: Option<i64>,
    /// Последняя ошибка работы с этим аккаунтом (логин, таймаут, лимит)
    #[serde(default)]
    pub last_error: Option<String>,
    /// Когда аккаунт последний раз успешно ответил
    #[serde(default)]
    pub last_used_at: Option<i64>,
}

impl WebProfile {
    pub fn limited_now(&self, now_ts: i64) -> bool {
        match self.limit_until {
            // Явный срок важнее флага: истёкший лимит больше не блокирует аккаунт.
            Some(until) => until > now_ts,
            None => self.limit_hit,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ContextMode {
    None,
    Skeleton,
    Full,
}

impl Default for ContextMode {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFileNode {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub estimated_tokens: usize,
    pub skeleton_tokens: usize,
    pub language: String,
    pub context_mode: ContextMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatAttachment {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String, // "user" | "assistant"
    pub content: String,
    pub provider_name: Option<String>,
    pub model_name: Option<String>,
    pub timestamp: i64,
    pub tokens_approx: usize,
    #[serde(default)]
    pub attachments: Vec<ChatAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModelServer {
    pub name: String,
    pub base_url: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatThread {
    pub id: String,
    pub title: String,
    pub updated_at: i64,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub locale: LocaleCode,
    pub active_mode: String, // "api" | "web"
    pub project_root: Option<String>,
    pub active_profile_id: Option<String>,
    pub active_web_profile_id: Option<String>,
    pub auto_rotate_on_limit: bool,
    pub max_context_tokens_budget: usize,
    /// Soft limit for the whole request: project context + chat history + prompt.
    pub chat_context_budget: usize,
    pub api_profiles: Vec<ApiProfile>,
    pub web_profiles: Vec<WebProfile>,
    pub file_modes: HashMap<String, ContextMode>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            locale: LocaleCode::Ru,
            active_mode: "api".to_string(),
            project_root: None,
            active_profile_id: Some("default-gemini".to_string()),
            active_web_profile_id: Some("web-1".to_string()),
            auto_rotate_on_limit: true,
            max_context_tokens_budget: DEFAULT_CONTEXT_BUDGET,
            chat_context_budget: DEFAULT_CHAT_CONTEXT_BUDGET,
            api_profiles: default_api_profiles(),
            web_profiles: default_web_profiles(),
            file_modes: HashMap::new(),
        }
    }
}

pub fn default_api_profiles() -> Vec<ApiProfile> {
    vec![
        ApiProfile {
            id: "default-gemini".to_string(),
            name: "Google AI Studio".to_string(),
            kind: ProviderKind::Gemini,
            api_key: String::new(),
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            model: "gemini-3.5-flash-lite".to_string(),
            enabled: true,
            cooldown_until: None,
            last_error: None,
            requests_count: 0,
        },
        ApiProfile {
            id: "default-openrouter-ds".to_string(),
            name: "OpenRouter Free".to_string(),
            kind: ProviderKind::OpenRouter,
            api_key: String::new(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            // "auto" — взять лучшую доступную :free модель через GET /models
            model: "auto".to_string(),
            enabled: true,
            cooldown_until: None,
            last_error: None,
            requests_count: 0,
        },
        ApiProfile {
            id: "default-groq".to_string(),
            name: "Groq Free".to_string(),
            kind: ProviderKind::Groq,
            api_key: String::new(),
            base_url: "https://api.groq.com/openai/v1".to_string(),
            model: "llama-3.3-70b-versatile".to_string(),
            enabled: true,
            cooldown_until: None,
            last_error: None,
            requests_count: 0,
        },
    ]
}

pub fn default_web_profiles() -> Vec<WebProfile> {
    (1..=4)
        .map(|i| WebProfile {
            id: format!("web-{i}"),
            name: format!("DeepSeek #{i}"),
            kind: WebKind::Deepseek,
            url: "https://chat.deepseek.com".to_string(),
            user_data_dir: format!("web_profile_{i}"),
            limit_hit: false,
            notes: String::new(),
            limit_until: None,
            last_error: None,
            last_used_at: None,
        })
        .collect()
}

impl AppConfig {
    /// Приводит старые/битые конфиги к рабочему виду.
    /// `#[serde(default)]` подставляет пустые значения для отсутствующих полей,
    /// поэтому без нормализации старый config.json ломал режим/SELECT'ы.
    pub fn normalize(mut self) -> Self {
        if self.active_mode != "api" && self.active_mode != "web" {
            self.active_mode = "api".to_string();
        }
        if self.max_context_tokens_budget == 0 {
            self.max_context_tokens_budget = DEFAULT_CONTEXT_BUDGET;
        }
        self.max_context_tokens_budget = self
            .max_context_tokens_budget
            .clamp(MIN_CONTEXT_BUDGET, MAX_CONTEXT_BUDGET);
        if self.chat_context_budget == 0 || self.chat_context_budget == 64_000 {
            // 64k was the old default; upgrade existing installs to the new safe default.
            self.chat_context_budget = DEFAULT_CHAT_CONTEXT_BUDGET;
        }
        self.chat_context_budget = self
            .chat_context_budget
            .clamp(MIN_CHAT_CONTEXT_BUDGET, MAX_CONTEXT_BUDGET);

        for p in self.api_profiles.iter_mut() {
            if p.base_url.trim().is_empty() {
                p.base_url = match p.kind {
                    ProviderKind::Gemini => "https://generativelanguage.googleapis.com/v1beta",
                    ProviderKind::OpenRouter => "https://openrouter.ai/api/v1",
                    ProviderKind::Groq => "https://api.groq.com/openai/v1",
                    _ => "",
                }
                .to_string();
            }
            if p.name.trim().is_empty() {
                p.name = format!("{:?}", p.kind);
            }
            if let Some(until) = p.cooldown_until {
                if until <= chrono::Utc::now().timestamp() {
                    p.cooldown_until = None;
                }
            }
        }

        for (idx, w) in self.web_profiles.iter_mut().enumerate() {
            if w.url.trim().is_empty() {
                w.url = w.kind.default_url().to_string();
            }
            if w.user_data_dir.trim().is_empty() {
                w.user_data_dir = format!("web_profile_{}", idx + 1);
            }
            if w.name.trim().is_empty() {
                w.name = format!("{} #{}", w.kind.default_name(), idx + 1);
            }
            if w.limit_hit
                && w.limit_until
                    .map_or(true, |u| u <= chrono::Utc::now().timestamp())
            {
                w.limit_hit = false;
                w.limit_until = None;
            }
        }

        let api_ids: Vec<String> = self.api_profiles.iter().map(|p| p.id.clone()).collect();
        if let Some(active) = self.active_profile_id.clone() {
            if !api_ids.contains(&active) {
                self.active_profile_id = self
                    .api_profiles
                    .iter()
                    .find(|p| p.is_usable())
                    .or_else(|| self.api_profiles.first())
                    .map(|p| p.id.clone());
            }
        } else {
            self.active_profile_id = self.api_profiles.first().map(|p| p.id.clone());
        }

        // Kimi and ChatGPT Web cannot complete a reply reliably inside WebView2
        // (OAuth / anti-automation). Their old profiles are dropped; use API presets.
        self.web_profiles
            .retain(|w| !matches!(w.kind, WebKind::Kimi | WebKind::ChatGpt));

        let web_ids: Vec<String> = self.web_profiles.iter().map(|p| p.id.clone()).collect();
        if let Some(active) = self.active_web_profile_id.clone() {
            if !web_ids.contains(&active) {
                self.active_web_profile_id = self.web_profiles.first().map(|p| p.id.clone());
            }
        } else {
            self.active_web_profile_id = self.web_profiles.first().map(|p| p.id.clone());
        }

        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltContextResult {
    pub system_prompt: String,
    pub total_tokens: usize,
    pub full_files_count: usize,
    pub skeleton_files_count: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderTestResult {
    pub ok: bool,
    pub profile_id: String,
    pub profile_name: String,
    pub model: String,
    pub reply: String,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_recovers_broken_config() {
        let mut cfg = AppConfig::default();
        cfg.active_mode = "".into();
        cfg.max_context_tokens_budget = 0;
        cfg.active_profile_id = Some("missing".into());
        cfg.active_web_profile_id = Some("missing".into());
        let cfg = cfg.normalize();
        assert_eq!(cfg.active_mode, "api");
        assert_eq!(cfg.max_context_tokens_budget, DEFAULT_CONTEXT_BUDGET);
        assert_eq!(
            cfg.active_profile_id.as_deref(),
            Some(cfg.api_profiles[0].id.as_str())
        );
        assert_eq!(
            cfg.active_web_profile_id.as_deref(),
            Some(cfg.web_profiles[0].id.as_str())
        );
    }

    #[test]
    fn explicit_model_ignores_auto() {
        let mut p = ApiProfile {
            id: "x".into(),
            name: "x".into(),
            kind: ProviderKind::OpenRouter,
            api_key: "k".into(),
            base_url: "b".into(),
            model: "auto".into(),
            enabled: true,
            cooldown_until: None,
            last_error: None,
            requests_count: 0,
        };
        assert!(p.explicit_model().is_none());
        p.model = "  ".into();
        assert!(p.explicit_model().is_none());
        p.model = "deepseek/deepseek-chat:free".into();
        assert_eq!(p.explicit_model(), Some("deepseek/deepseek-chat:free"));
    }
}
