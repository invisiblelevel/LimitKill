use crate::models::WebProfile;
use crate::storage::get_web_profiles_base_dir;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{
    AppHandle, Emitter, LogicalPosition, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

const PARKED_X: f64 = -12000.0;

/// Больше этого handoff-пакет в окно DeepSeek не отправится — сайт просто обрежет/зависнет.
pub const MAX_WEB_PACKET_CHARS: usize = 48_000;

#[derive(Debug, Clone)]
pub struct Report {
    pub seq: u64,
    pub kind: String,
    pub text: String,
}

#[derive(Default)]
pub struct WebBridgeState {
    /// thread_id, в который уже «вживлён» контекст конкретного аккаунта
    pub primed: Mutex<HashMap<String, String>>,
    /// Очередь отчётов из JS-моста (IPC-канал; работает параллельно с hash-каналом)
    pub reports: Mutex<HashMap<String, VecDeque<Report>>>,
}

impl WebBridgeState {
    pub fn push_report(&self, profile_id: &str, seq: u64, kind: &str, text: &str) {
        if let Ok(mut map) = self.reports.lock() {
            let queue = map.entry(profile_id.to_string()).or_default();
            queue.push_back(Report {
                seq,
                kind: kind.to_string(),
                text: text.to_string(),
            });
            while queue.len() > 256 {
                queue.pop_front();
            }
        }
    }

    pub fn take_reports(&self, profile_id: &str) -> Vec<Report> {
        if let Ok(mut map) = self.reports.lock() {
            if let Some(mut queue) = map.remove(profile_id) {
                return queue.drain(..).collect();
            }
        }
        Vec::new()
    }

    pub fn clear_reports(&self, profile_id: &str) {
        if let Ok(mut map) = self.reports.lock() {
            map.remove(profile_id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyState {
    Ready,
    Login,
    Pending,
    Cancelled,
}

pub enum WebOutcome {
    Reply(String),
    Limit(String),
}

#[derive(Clone, serde::Serialize)]
struct PartialPayload {
    text: String,
    profile: String,
}

/// JS-мост.
///
/// ВАЖНО: DeepSeek — SPA, поэтому:
///  * селекторы перепроверяются на каждом шаге, скрипт не кэширует элементы;
///  * отчёт идёт двумя каналами: IPC (`web_report`) и hash (fallback для старых сборок);
///  * `history.replaceState` вместо `location.hash`, чтобы не спамить историю на каждый partial;
///  * завершение ответа определяется не только тишиной, но и исчезновением кнопки Stop.
const BRIDGE_HELPER_SCRIPT: &str = r#"
(function () {
  if (window.__deepbridgeInjected) return;
  window.__deepbridgeInjected = true;

  const BASE = {
    input: [
      '#chat-input',
      'textarea#chat-input',
      'textarea[placeholder]',
      'div[contenteditable="true"][role="textbox"]',
      'div[contenteditable="true"]'
    ],
    send: [
      'button[aria-label*="Send" i]',
      'button[aria-label*="Отправить" i]',
      'button[aria-label*="Отправ" i]',
      'button[aria-label*="发送"]',
      '[aria-label*="Send" i][role="button"]',
      '[aria-label*="Отправ" i][role="button"]',
      '[aria-label*="发送"][role="button"]',
      'button[data-testid*="send" i]',
      'button[type="submit"]',
      'button[class*="send"]',
      '[class*="send-button"]',
      'div[class*="send-btn"]',
      'div[class*="send"]'
    ],
    reply: [
      'div.ds-markdown',
      'div[class*="ds-markdown"]',
      'div[class*="markdown"]'
    ],
    stop: [
      'button[aria-label*="Stop" i]',
      'div[class*="stop"]',
      'button[class*="stop"]'
    ],
    alerts: [
      '[role="alert"]',
      '[role="dialog"]',
      '[class*="toast"]',
      '[class*="Toast"]',
      '[class*="modal"]'
    ],
    limitPatterns: [
      /too many (requests|messages)/i,
      /(usage|message|rate) limit/i,
      /limit (reached|exceeded)/i,
      /try again (later|in)/i,
      /server is busy/i,
      /слишком много/i,
      /лимит/i,
      /请稍后再试/,
      /过于频繁/
    ],
    pollMs: 400,
    submitDelayMs: 350,
    stableMs: 1600,
    hardStableMs: 5000,
    partialEveryMs: 500,
    noStartMs: 60000,
    maxWaitMs: 420000,
    maxPartialChars: 12000,
    maxFinalChars: 60000
  };

  const SITES = {
    gemini: {
      input: [
        'rich-textarea .ql-editor[contenteditable="true"]',
        'div.ql-editor[contenteditable="true"]',
        'div[contenteditable="true"][role="textbox"]',
        'textarea'
      ],
      send: [
        'button[aria-label*="Send message" i]',
        'button[aria-label*="Send" i]',
        'button[aria-label*="Отправить сообщение" i]',
        'button[aria-label*="Отправ" i]',
        'button[aria-label*="发送"]',
        'button.send-button',
        'button[type="submit"]',
        'button[class*="send"]'
      ],
      reply: [
        'message-content',
        'model-response',
        'model-response-text',
        'div[class*="model-response"]',
        'div[class*="response-container"]',
        'div[class*="markdown"]'
      ],
      stop: [
        'button[aria-label*="Stop" i]',
        'button[aria-label*="Остановить" i]',
        'button[class*="stop"]'
      ]
    },
    qwen: {
      input: [
        'textarea.message-input-textarea',
        'textarea',
        'div[contenteditable="true"][role="textbox"]',
        'div[contenteditable="true"]'
      ],
      send: [
        'button[aria-label*="Send" i]',
        'button[aria-label*="Отправ" i]',
        'button[aria-label*="发送"]',
        'button[data-testid*="send" i]',
        'button[type="submit"]',
        'button[class*="send"]',
        'div[class*="send-btn"]',
        'div[class*="message-input-right-button-send"]',
        'div[class*="send"]'
      ],
      reply: [
        'div[class*="chat-message"]:not([class*="input"]):not([class*="fixed"])',
        'div[class*="markdown-body"]',
        'div[class*="qwen-markdown"]',
        'div[class*="message-content"]',
        'div[class*="response-content"]',
        'div[class*="chat-message"]:not([class*="input"])'
      ]
    },
    claude: {
      input: [
        'div.ProseMirror[contenteditable="true"]',
        'div[contenteditable="true"][data-placeholder]',
        'div[contenteditable="true"][role="textbox"]',
        'div[contenteditable="true"]',
        'textarea'
      ],
      send: [
        'button[aria-label*="Send" i]',
        'button[aria-label*="Отправ" i]',
        'button[type="submit"]',
        'button[class*="send"]'
      ],
      reply: [
        'div[class*="font-claude-message"]',
        'div[data-testid*="message"]',
        'div[class*="markdown"]',
        'div[class*="prose"]'
      ],
      stop: [
        'button[aria-label*="Stop" i]',
        'button[aria-label*="Остановить" i]',
        'button[class*="stop"]'
      ]
    }
  };

  const CFG = Object.assign({}, BASE, SITES[window.__deepbridgeKind] || {});

  let seqN = 0;
  let ipcBroken = false;
  let lastHash = '';

  const visible = function (el) {
    return !!(el && (el.offsetWidth || el.offsetHeight || el.getClientRects().length));
  };
  const q = function (sels) {
    for (const s of sels) {
      const el = document.querySelector(s);
      if (visible(el)) return el;
    }
    return null;
  };
  const qa = function (sels) {
    for (const s of sels) {
      const list = Array.from(document.querySelectorAll(s)).filter(visible);
      if (list.length) return list;
    }
    return [];
  };
  const qaText = function (sels) {
    for (const s of sels) {
      const list = Array.from(document.querySelectorAll(s)).filter(function (el) {
        if (!visible(el) || !(el.innerText || '').trim()) return false;
        if (el.closest('[class*="user-message"]')) return false;
        if (el.closest('[data-message-author-role="user"]')) return false;
        if (el.closest('[contenteditable="true"]')) return false;
        if (el.closest('[class*="ProseMirror"]')) return false;
        if (el.closest('[class*="composer"]')) return false;
        const input = inputEl();
        if (input && (el === input || el.contains(input))) return false;
        return true;
      });
      if (list.length) return list;
    }
    return [];
  };

  const report = function (kind, text) {
    text = String(text == null ? '' : text);
    if (kind === 'partial') text = text.slice(0, CFG.maxPartialChars);
    else text = text.slice(0, CFG.maxFinalChars);
    const seq = Date.now() * 100 + (seqN++ % 100);
    const pid = window.__deepbridgeProfileId || '';
    let delivered = false;

    if (!ipcBroken && window.__TAURI_INTERNALS__ && typeof window.__TAURI_INTERNALS__.invoke === 'function') {
      try {
        const p = window.__TAURI_INTERNALS__.invoke('web_report', { profileId: pid, kind: kind, text: text, seq: seq });
        if (p && typeof p.catch === 'function') p.catch(function () { ipcBroken = true; });
        delivered = true;
      } catch (e) {
        ipcBroken = true;
      }
    }

    // Финальные события и partial'ы дублируем в hash: IPC из remote-origin может быть
    // запрещён ACL'ом, а hash-канал работает всегда. Дубли отсекаются по seq в Rust.
    try {
      const hash = '#db:' + seq + ':' + kind + ':' + encodeURIComponent(text);
      if (hash !== lastHash) {
        lastHash = hash;
        history.replaceState(null, '', hash);
      }
    } catch (e) { console.error('[deepbridge] hash report failed', e); }
    window.__deepbridgeLast = { seq: seq, kind: kind, text: text };
  };

  function inputEl() { return q(CFG.input); }
  function passwordVisible() { return visible(document.querySelector('input[type="password"]')); }
  function loginVisible() {
    if (passwordVisible()) return true;
    if (inputEl()) return false;
    const text = (document.body && document.body.innerText || '').slice(0, 4000).toLowerCase();
    return /log in|sign in|登录|войти|вход/.test(text);
  }
  function isReady() { return !!inputEl() && !passwordVisible(); }

  function setInput(text) {
    const el = inputEl();
    if (!el) return false;
    el.focus();
    if (el.tagName === 'TEXTAREA' || el.tagName === 'INPUT') {
      const proto = el.tagName === 'TEXTAREA' ? window.HTMLTextAreaElement.prototype : window.HTMLInputElement.prototype;
      const desc = Object.getOwnPropertyDescriptor(proto, 'value');
      const setter = desc && desc.set;
      if (setter) setter.call(el, text); else el.value = text;
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
    } else {
      el.focus();
      try {
        const sel = window.getSelection();
        const range = document.createRange();
        range.selectNodeContents(el);
        if (sel) {
          sel.removeAllRanges();
          sel.addRange(range);
        }
        document.execCommand('insertText', false, text);
      } catch (e) {}

      // Quill-based editors (Gemini) may ignore execCommand in a background WebView.
      // A synthetic paste goes through Quill's clipboard module and updates its model.
      if (!(el.innerText || '').includes(text.slice(0, Math.min(12, text.length)))) {
        try {
          const dt = new DataTransfer();
          dt.setData('text/plain', text);
          const paste = new Event('paste', { bubbles: true, cancelable: true });
          Object.defineProperty(paste, 'clipboardData', { value: dt });
          el.dispatchEvent(paste);
        } catch (e) {}
      }

      // Last resort: write the DOM directly and notify the framework.
      if (!(el.innerText || '').includes(text.slice(0, Math.min(12, text.length)))) {
        el.textContent = text;
      }
      el.classList.remove('ql-blank');
      el.dispatchEvent(new InputEvent('input', { bubbles: true, data: text, inputType: 'insertText' }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
    }
    return true;
  }

  function pressEnter(el) {
    const opts = { key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true, cancelable: true };
    for (const type of ['keydown', 'keypress', 'keyup']) {
      el.dispatchEvent(new KeyboardEvent(type, opts));
    }
  }

  function submit(el) {
    const button = q(CFG.send);
    if (button && !button.disabled) {
      button.click();
      // Some sites wrap the real send button in a div and ignore .click();
      // if the composer is still filled shortly after, press Enter as a fallback.
      setTimeout(function () {
        const current = inputEl();
        if (!current) return;
        const left = (current.value != null ? current.value : current.innerText || '').trim();
        if (left) pressEnter(current);
      }, 1200);
      return true;
    }
    pressEnter(el);
    return false;
  }

  function scanLimit() {
    for (const el of qa(CFG.alerts)) {
      if (!visible(el)) continue;
      const text = (el.innerText || '').trim();
      if (text && CFG.limitPatterns.some(function (re) { return re.test(text); })) {
        return text.slice(0, 300);
      }
    }
    return null;
  }

  function watch(baselineCount, baselineText) {
    const startedAt = Date.now();
    let started = false;
    let last = '';
    let lastChange = Date.now();
    let lastPartialAt = 0;
    let done = false;
    let timer = null;

    const finish = function (kind, text) {
      if (done) return;
      done = true;
      if (timer) clearInterval(timer);
      window.__deepbridgeBusy = false;
      report(kind, text);
    };

    timer = setInterval(function () {
      if (done) return;
      if (window.__deepbridgeAborted) {
        done = true;
        clearInterval(timer);
        window.__deepbridgeBusy = false;
        return;
      }
      const replies = qaText(CFG.reply);
      const current = replies.length ? (replies[replies.length - 1].innerText || '').trim() : '';
      const newReply = replies.length > baselineCount || (current && current !== baselineText);
      if (!started && newReply) {
        started = true;
        lastChange = Date.now();
      }

      const limit = scanLimit();
      if (!started) {
        if (limit) return finish('limit', limit);
        if (Date.now() - startedAt > CFG.noStartMs) {
          return finish('error', 'Ответ не начался. Проверь, что аккаунт залогинен, и повтори запрос.');
        }
        return;
      }

      const now = Date.now();
      if (current !== last) {
        last = current;
        lastChange = now;
      }
      if (current && now - lastPartialAt >= CFG.partialEveryMs) {
        lastPartialAt = now;
        report('partial', current);
      }

      const stopVisible = !!q(CFG.stop);
      if (limit && !stopVisible && current) return finish('limit', limit);
      if (current && !stopVisible && now - lastChange > CFG.stableMs) return finish('reply', current);
      // Страховка: даже если кнопка Stop не найдена, не висим вечно на «застывшем» тексте.
      if (current && now - lastChange > CFG.hardStableMs) return finish('reply', current);
      if (now - startedAt > CFG.maxWaitMs) {
        return finish(current ? 'reply' : 'error', current || 'Таймаут ожидания ответа');
      }
    }, CFG.pollMs);
  }

  function selectorReport(sels) {
    return sels.map(function (s) {
      let n = 0;
      try { n = document.querySelectorAll(s).length; } catch (e) { n = -1; }
      return s + '=' + n;
    }).join('; ');
  }
  function classOf(el) {
    if (!el) return 'none';
    return (el.tagName || '?').toLowerCase() + ':' + String(el.className || '').slice(0, 90);
  }
  window.__deepbridgeInsertPrompt = setInput;
  function diagText() {
    const input = inputEl();
    const replies = qa(CFG.reply);
    const buttonSamples = Array.from(document.querySelectorAll('button'))
      .filter(visible).slice(0, 10)
      .map(function (b) {
        return (b.getAttribute('aria-label') || '') + '|' + String(b.className || '').slice(0, 55) + '|' + (b.innerText || '').trim().slice(0, 24);
      }).join(' || ');
    const classSamples = Array.from(document.querySelectorAll('[class*="message"],[class*="response"],[class*="markdown"],[class*="chat"]'))
      .filter(visible).slice(0, 10)
      .map(function (e) { return classOf(e) + '|' + (e.innerText || '').trim().slice(0, 30); })
      .join(' || ');
    const ariaSamples = Array.from(document.querySelectorAll('[aria-label]'))
      .filter(visible).slice(0, 14)
      .map(function (e) { return (e.getAttribute('aria-label') || '') + '|' + classOf(e); })
      .join(' || ');
    const sendSamples = Array.from(document.querySelectorAll('[class*="send"],[class*="Send"],[data-testid*="send"],[role="button"]'))
      .filter(visible).slice(0, 10)
      .map(function (e) { return classOf(e) + '|' + (e.innerText || '').trim().slice(0, 20); })
      .join(' || ');
    return location.origin + location.pathname +
      ' | input=' + classOf(input) +
      ' | send=' + selectorReport(CFG.send) +
      ' | reply=' + selectorReport(CFG.reply) +
      ' | stop=' + selectorReport(CFG.stop) +
      ' | lastReply=' + classOf(replies[replies.length - 1]) +
      ' | buttons=' + buttonSamples +
      ' | aria=' + ariaSamples +
      ' | sendSamples=' + sendSamples +
      ' | classes=' + classSamples +
      ' | textarea=' + document.querySelectorAll('textarea').length +
      ' | contenteditable=' + document.querySelectorAll('[contenteditable="true"]').length;
  }
  window.__deepbridgePing = function () {
    const detail = diagText();
    if (isReady()) return report('ready', detail);
    if (loginVisible()) return report('login', detail);
    report('diag', detail);
  };
  window.__deepbridgeSend = function (text) {
    if (window.__deepbridgeBusy) return report('error', 'Окно занято предыдущим запросом');
    if (!isReady()) {
      return report(loginVisible() ? 'login' : 'error',
        loginVisible() ? 'Нужен логин в аккаунт' : 'Поле ввода не найдено');
    }
    const replies = qaText(CFG.reply);
    const baselineCount = replies.length;
    const baselineText = replies.length ? (replies[replies.length - 1].innerText || '').trim() : '';
    if (!setInput(text)) return report('error', 'Не удалось ввести текст в поле ввода');
    window.__deepbridgeBusy = true;
    window.__deepbridgeAborted = false;
    setTimeout(function () {
      const el = inputEl();
      if (!el) return;
      const entered = (el.value != null ? el.value : el.innerText || '').trim();
      if (!entered || !entered.includes(text.slice(0, Math.min(20, text.length)))) {
        window.__deepbridgeBusy = false;
        return report('error', 'Текст не попал в поле ввода. Возможно, сайт использует защищённый редактор.');
      }
      submit(el);
    }, CFG.submitDelayMs);
    watch(baselineCount, baselineText);
  };
  window.__deepbridgeAbort = function () {
    window.__deepbridgeAborted = true;
    window.__deepbridgeBusy = false;
    // Останавливаем генерацию на самой странице DeepSeek, если кнопка Stop существует.
    const stop = q(CFG.stop);
    if (stop) {
      try { stop.click(); } catch (e) {}
    }
  };
  window.__deepbridgeCancel = window.__deepbridgeAbort;
})();
"#;

pub fn window_label(profile_id: &str) -> String {
    let cleaned: String = profile_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("ds_web_{cleaned}")
}

fn profile_dir(profile: &WebProfile) -> Result<std::path::PathBuf, String> {
    let name = profile.user_data_dir.trim();
    if name.is_empty()
        || name.contains("..")
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
    {
        return Err(format!(
            "Некорректная папка профиля: '{}'. Используй простое имя без слэшей.",
            profile.user_data_dir
        ));
    }
    Ok(get_web_profiles_base_dir()?.join(name))
}

pub fn reveal_window(win: &WebviewWindow) {
    let _ = win.set_skip_taskbar(false);
    let _ = win.set_position(LogicalPosition::new(120.0, 60.0));
    let _ = win.unminimize();
    let _ = win.show();
    let _ = win.set_focus();
}

pub fn park_window(app: &AppHandle, win: &WebviewWindow) {
    // Не скрываем окно полностью: WebView2 душит таймеры у скрытых окон,
    // из-за чего стриминг «замирал». Показываем его уже за пределами экрана.
    let _ = win.set_skip_taskbar(true);
    let _ = win.set_position(LogicalPosition::new(PARKED_X, PARKED_X));
    let _ = win.show();
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_focus();
    }
}

pub fn open_web_profile_window(
    app: &AppHandle,
    profile: &WebProfile,
    background: bool,
) -> Result<(), String> {
    let label = window_label(&profile.id);
    if let Some(existing) = app.get_webview_window(&label) {
        if !background {
            reveal_window(&existing);
        }
        return Ok(());
    }

    let profile_dir = profile_dir(profile)?;
    if !profile_dir.exists() {
        fs::create_dir_all(&profile_dir)
            .map_err(|e| format!("Failed to create profile dir: {e}"))?;
    }

    let parsed_url: url::Url = profile
        .url
        .parse()
        .map_err(|e| format!("Invalid Web profile URL: {e}"))?;
    let id_json = serde_json::to_string(&profile.id).map_err(|e| e.to_string())?;
    let kind_json = serde_json::to_string(&profile.kind).map_err(|e| e.to_string())?;
    let script = format!(
        "window.__deepbridgeProfileId = {id_json};\nwindow.__deepbridgeKind = {kind_json};\n{BRIDGE_HELPER_SCRIPT}"
    );

    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parsed_url))
        .title(format!("LimitKill — {}", profile.name))
        .inner_size(1180.0, 820.0)
        .min_inner_size(640.0, 480.0)
        .decorations(true)
        .resizable(true)
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        )
        .data_directory(profile_dir)
        .initialization_script(script.as_str())
        .focused(!background);

    if background {
        builder = builder
            .visible(false)
            .position(PARKED_X, PARKED_X)
            .skip_taskbar(true);
    } else {
        builder = builder.visible(true);
    }

    let win = builder
        .build()
        .map_err(|e| format!("Failed to open WebView window: {e}"))?;
    if background {
        park_window(app, &win);
    }
    Ok(())
}

pub fn close_web_profile_window(app: &AppHandle, profile_id: &str) {
    if let Some(win) = app.get_webview_window(&window_label(profile_id)) {
        let _ = win.close();
    }
}

/// Удаляет куки/сессию аккаунта. Окно должно быть закрыто, иначе WebView2 держит файлы.
pub fn reset_web_profile_data(profile: &WebProfile) -> Result<(), String> {
    let dir = profile_dir(profile)?;
    if !dir.exists() {
        return Ok(());
    }
    fs::remove_dir_all(&dir).map_err(|e| {
        format!(
            "Не удалось удалить папку сессии {}: {e}. Закрой окно аккаунта и повтори.",
            dir.display()
        )
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn read_hash_report(win: &WebviewWindow) -> Option<Report> {
    let url = win.url().ok()?;
    let fragment = url.fragment()?;
    let rest = fragment.strip_prefix("db:")?;
    let mut iter = rest.splitn(3, ':');
    let seq: u64 = iter.next()?.parse().ok()?;
    let kind = iter.next()?.to_string();
    let text = percent_decode(iter.next().unwrap_or(""));
    Some(Report { seq, kind, text })
}

fn ensure_bridge(win: &WebviewWindow) -> Result<(), String> {
    let script = format!(
        "(function(){{ if (window.__deepbridgePing) return; window.__deepbridgeKind = window.__deepbridgeKind || 'deepseek'; try {{ {BRIDGE_HELPER_SCRIPT} }} catch (e) {{}} }})();"
    );
    win.eval(script)
        .map_err(|e| format!("Failed to inject bridge: {e}"))
}

fn collect_reports(
    bridge: &WebBridgeState,
    win: &WebviewWindow,
    profile_id: &str,
    last_seq: &mut u64,
) -> Vec<Report> {
    let mut all = bridge.take_reports(profile_id);
    if let Some(report) = read_hash_report(win) {
        all.push(report);
    }
    all.sort_by_key(|r| r.seq);
    all.retain(|r| r.seq > *last_seq);
    if let Some(last) = all.last() {
        *last_seq = last.seq;
    }
    all
}

async fn wait_for_ready(
    app: &AppHandle,
    bridge: &WebBridgeState,
    win: &WebviewWindow,
    profile: &WebProfile,
    timeout: Duration,
    reveal_after: Duration,
    cancel: Option<&AtomicBool>,
) -> Result<(ReadyState, String), String> {
    let started = Instant::now();
    let mut last_seq = read_hash_report(win).map(|r| r.seq).unwrap_or(0);
    let mut last_bridge_eval = Instant::now() - Duration::from_secs(10);
    let mut last_ping = Instant::now() - Duration::from_secs(10);
    let mut revealed = false;
    let mut last_diag = String::new();
    let label = window_label(&profile.id);

    loop {
        if cancel.map_or(false, |flag| flag.load(Ordering::Relaxed)) {
            return Ok((ReadyState::Cancelled, last_diag));
        }
        if app.get_webview_window(&label).is_none() {
            return Err(format!("Окно аккаунта '{}' было закрыто", profile.name));
        }
        if last_bridge_eval.elapsed() > Duration::from_secs(5) {
            let _ = ensure_bridge(win);
            last_bridge_eval = Instant::now();
        }
        if last_ping.elapsed() > Duration::from_secs(2) {
            let _ = win.eval("window.__deepbridgePing && window.__deepbridgePing();");
            last_ping = Instant::now();
        }

        for report in collect_reports(bridge, win, &profile.id, &mut last_seq) {
            match report.kind.as_str() {
                "ready" => {
                    if revealed {
                        park_window(app, win);
                    }
                    if !report.text.trim().is_empty() {
                        last_diag = report.text;
                    }
                    return Ok((ReadyState::Ready, last_diag));
                }
                "login" => {
                    reveal_window(win);
                    if !report.text.trim().is_empty() {
                        last_diag = report.text;
                    }
                    return Ok((ReadyState::Login, last_diag));
                }
                "diag" => last_diag = report.text,
                _ => {}
            }
        }

        if !revealed && started.elapsed() > reveal_after {
            reveal_window(win);
            revealed = true;
        }
        if started.elapsed() > timeout {
            return Ok((ReadyState::Pending, last_diag));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

/// Проверка аккаунта из UI: открыть фоновое окно и понять, залогинен ли он.
pub async fn probe_web_profile(
    app: &AppHandle,
    bridge: &WebBridgeState,
    profile: &WebProfile,
) -> Result<(ReadyState, String), String> {
    open_web_profile_window(app, profile, true)?;
    let label = window_label(&profile.id);
    let win = app
        .get_webview_window(&label)
        .ok_or_else(|| "Окно аккаунта не создалось".to_string())?;
    bridge.clear_reports(&profile.id);
    let (state, detail) = wait_for_ready(
        app,
        bridge,
        &win,
        profile,
        Duration::from_secs(40),
        Duration::from_secs(20),
        None,
    )
    .await?;
    match state {
        ReadyState::Ready => {
            park_window(app, &win);
            Ok((ReadyState::Ready, detail))
        }
        ReadyState::Login => {
            reveal_window(&win);
            Ok((ReadyState::Login, detail))
        }
        ReadyState::Pending => {
            reveal_window(&win);
            Ok((ReadyState::Pending, detail))
        }
        ReadyState::Cancelled => Ok((ReadyState::Cancelled, detail)),
    }
}

pub fn cap_packet(packet: &str, max_chars: usize) -> String {
    if packet.chars().count() <= max_chars {
        return packet.to_string();
    }
    let keep = max_chars.saturating_sub(240);
    let mut out: String = packet.chars().take(keep).collect();
    out.push_str(
        "\n\n[... Контекст сокращён LimitKill: пакет превысил лимит окна DeepSeek. \
Выбери меньше файлов или переключи их в режим «Скелет».]",
    );
    out
}

/// Отправляет пакет в конкретный Web-аккаунт и ждёт ответ.
///
/// `reset_chat = true` — перед отправкой открыть новый чат (нужно при первом
/// использовании аккаунта в треде, иначе контекст смешается со старым диалогом).
pub async fn run_web_prompt(
    app: &AppHandle,
    bridge: &WebBridgeState,
    profile: &WebProfile,
    packet: &str,
    reset_chat: bool,
    cancel: Option<&AtomicBool>,
) -> Result<WebOutcome, String> {
    open_web_profile_window(app, profile, true)?;
    if cancel.map_or(false, |flag| flag.load(Ordering::Relaxed)) {
        return Err("CANCELLED".to_string());
    }

    let label = window_label(&profile.id);
    let win = app
        .get_webview_window(&label)
        .ok_or_else(|| "Окно аккаунта не создалось".to_string())?;

    bridge.clear_reports(&profile.id);

    let (ready_state, _ready_detail) = wait_for_ready(
        app,
        bridge,
        &win,
        profile,
        Duration::from_secs(35),
        Duration::from_secs(12),
        cancel,
    )
    .await?;
    match ready_state {
        ReadyState::Ready => park_window(app, &win),
        ReadyState::Cancelled => return Err("CANCELLED".to_string()),
        ReadyState::Login => {
            return Err(format!(
                "'{}' не залогинен. Войди в открытое окно DeepSeek и повтори запрос — дальше аккаунт запомнится.",
                profile.name
            ));
        }
        ReadyState::Pending => {
            reveal_window(&win);
            return Err(format!(
                "'{}' не загрузился за 35 секунд. Окно открыто — проверь логин/интернет и повтори.",
                profile.name
            ));
        }
    }

    if reset_chat {
        let url_json = serde_json::to_string(&profile.url).map_err(|e| e.to_string())?;
        let _ = win.eval(format!("location.href = {url_json};"));
        tokio::time::sleep(Duration::from_millis(1200)).await;
        bridge.clear_reports(&profile.id);
        let (reset_state, _) = wait_for_ready(
            app,
            bridge,
            &win,
            profile,
            Duration::from_secs(20),
            Duration::from_secs(20),
            cancel,
        )
        .await?;
        if reset_state == ReadyState::Cancelled {
            return Err("CANCELLED".to_string());
        }
        park_window(app, &win);
    }

    if cancel.map_or(false, |flag| flag.load(Ordering::Relaxed)) {
        return Err("CANCELLED".to_string());
    }

    bridge.clear_reports(&profile.id);
    let mut last_seq = read_hash_report(&win).map(|r| r.seq).unwrap_or(0);
    let packet = cap_packet(packet, MAX_WEB_PACKET_CHARS);
    let serialized = serde_json::to_string(&packet).map_err(|e| e.to_string())?;
    win.eval(format!("window.__deepbridgeSend({serialized});"))
        .map_err(|e| format!("Не удалось отправить промпт в WebView: {e}"))?;

    let deadline = Instant::now() + Duration::from_secs(450);
    loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if cancel.map_or(false, |flag| flag.load(Ordering::Relaxed)) {
            let _ = win.eval("window.__deepbridgeAbort && window.__deepbridgeAbort();");
            return Err("CANCELLED".to_string());
        }
        if app.get_webview_window(&label).is_none() {
            return Err(format!(
                "Окно аккаунта '{}' закрыто во время ответа",
                profile.name
            ));
        }

        for report in collect_reports(bridge, &win, &profile.id, &mut last_seq) {
            match report.kind.as_str() {
                "partial" => {
                    let _ = app.emit(
                        "web_partial",
                        PartialPayload {
                            text: report.text,
                            profile: profile.name.clone(),
                        },
                    );
                }
                "reply" => return Ok(WebOutcome::Reply(report.text)),
                "limit" => return Ok(WebOutcome::Limit(report.text)),
                "login" => {
                    reveal_window(&win);
                    return Err(format!(
                        "'{}' разлогинился — войди в открытое окно",
                        profile.name
                    ));
                }
                "error" => return Err(format!("{}: {}", profile.name, report.text)),
                _ => {}
            }
        }

        if Instant::now() > deadline {
            reveal_window(&win);
            return Err(format!(
                "{}: таймаут ожидания ответа (7.5 мин). Окно открыто — проверь, не висит ли генерация.",
                profile.name
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_works() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(
            percent_decode("%D0%BF%D1%80%D0%B8%D0%B2%D0%B5%D1%82"),
            "привет"
        );
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn window_labels_are_safe() {
        assert_eq!(window_label("web-1"), "ds_web_web_1");
        assert_eq!(window_label("a b/c:d"), "ds_web_a_b_c_d");
    }

    #[test]
    fn packet_is_capped_on_char_boundary() {
        let packet = "я".repeat(1000);
        let capped = cap_packet(&packet, 500);
        assert!(capped.chars().count() <= 500);
        assert!(capped.contains("сокращён"));
        assert_eq!(cap_packet("short", 500), "short");
    }
}
