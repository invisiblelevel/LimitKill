# Changelog

All notable changes to LimitKill are documented here.

## [1.0.0] — 2026-11-24

### Added

- Local-first AI coding workspace with Smart RAG: file scanner, skeletons, context budget and Handoff snapshot.
- API providers: Google Gemini, OpenRouter `:free`, Groq and custom OpenAI-compatible servers.
- Manual model selection: model lists in Settings and in the chat target switcher.
- Provider diagnostics: per-profile **Test** and **List models** actions.
- Streaming responses (SSE) for Gemini and OpenAI-compatible APIs.
- Stop button for API streams and DeepSeek Web generation.
- DeepSeek Web multi-account support with isolated sessions, login check, limit marking and session reset.
- Text selection copy button inside chat messages.
- Markdown rendering with tables, code highlighting and per-code-block copy.
- Warm light “paper & sage” UI, RU / EN / 中文 localization.
- Custom frameless title bar with window controls.
- Chat readability pass: 15 px body, 68ch measure, timestamps, assistant cards.
- Amber text selection and native right-click Copy; removed the duplicate floating copy button.
- Chat context menu: rename, archive/restore, JSON/Markdown export, JSON import, delete.
- Message actions: edit-and-resend and regenerate with the current target.
- Web accounts for DeepSeek, Google Gemini, Qwen and Claude, with service tabs and a scrollable list.
- Claude web fixed: replies are detected via text-bearing response blocks; Gemini/Qwen send fallback works.
- Claude web preset; Kimi web removed (Google login is blocked inside WebView2). Kimi API preset remains.
- Local model discovery for Ollama, LM Studio, llama.cpp, vLLM, text-generation-webui and Jan.
- Text/code/RTF attachments plus `.docx` and text-based PDF extraction.
- Query-aware + BM25 content ranking for project context.
- NSIS installer language selector: English, Russian, Simplified Chinese.
- Chat context meter + 95% warning with one-click handoff to a new thread (goal, last state, active files).
- Configurable chat context budget (default 128k, max 1M) alongside the project context budget (default 60k).
- OpenAI API preset (`gpt-4o-mini`); ChatGPT Web removed because its assistant turn does not appear under WebView2 automation.
- Installed NSIS package now defaults to `C:\Program Files\LimitKill` (per-machine, admin required).
- New app icon set regenerated from the updated root `icon.png`.
- Qwen (DashScope) and Kimi (Moonshot) API presets.
- Donation copy localized as “Разрабу на еду.” / “Food for the developer.” / “给开发者买饭。”
- Smart auto-scroll with “jump to latest”, per-code-block copy buttons.
- First-run three-step checklist (provider → project → task).
- Web account diagnostics (login state, bridge detail, URL) and a compact actions menu.
- Provider “Test all”, honest verified/unverified statuses, API-key visibility toggle.
- Focus trap for modal and target switcher, keyboard-friendly controls.
- Full icon set generated from `icon.png`, plus portable shortcut script.
- Windows DPAPI encryption for API keys stored on disk.
- Content Security Policy for the main window.
- CI workflow, MIT license and release notes.

### Security

- API keys in `config.json` are encrypted with Windows DPAPI (per-user).
- Content Security Policy enabled for the main window.
- Local data (`config.json`, `threads.json`, `webview_profiles/`) stays outside the repository and is ignored by git.

### Known limitations

- DeepSeek Web automation depends on the current site markup and may need selector updates.
- Gemini/OpenRouter free tiers are rate-limited; cooldowns are enforced.
- Groq and custom providers are not covered by live tests in this release.
- The installer is not code-signed yet, so Windows SmartScreen may show a warning.
