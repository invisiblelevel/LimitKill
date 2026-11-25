<div align="center">

<img src="src-tauri/icons/128x128.png" width="104" alt="LimitKill" />

# LimitKill

**One chat. Many models. No lost context.**

[English](#english) · [Русский](#русский) · [简体中文](#简体中文)

![Version](https://img.shields.io/badge/version-1.0.0-d99a3e?style=for-the-badge)
![Platform](https://img.shields.io/badge/platform-Windows-4f7a5a?style=for-the-badge&logo=windows&logoColor=white)
![Privacy](https://img.shields.io/badge/privacy-local--first%20%7C%20no%20telemetry-8b6f9e?style=for-the-badge)
![License](https://img.shields.io/badge/license-MIT-b86b4b?style=for-the-badge)

</div>

---

## English

**LimitKill** is a calm, local-first AI workspace for coding on Windows. Keep one chat, switch between API providers, local models and web accounts, and let LimitKill prepare the right project context for every task.

### Why LimitKill

- **One chat, many models.** Switch between Gemini, OpenRouter, Groq, Qwen, Kimi, OpenAI and your own OpenAI-compatible server without rewriting the question.
- **Web accounts included.** Use DeepSeek, Google Gemini, Qwen and Claude in isolated sessions — each account keeps its own login.
- **Local models, one click.** If Ollama or LM Studio is running, LimitKill finds it and adds the model for you.
- **Smart project context.** Pick a project folder, choose files, and use **Full / Skeleton / Off** modes. Content-aware search puts the most relevant code first.
- **Long chats do not die.** At 95% of the chat budget, LimitKill offers a new chat with a saved progress snapshot: goal, last state, recent messages and active files.
- **Files in chat.** Attach text, code, `.docx` or text-based `.pdf` files and ask about them directly.
- **Private by design.** Chats, settings and web sessions stay on your PC. API keys are encrypted; no telemetry, no LimitKill account.
- **Comfortable UI.** Warm light theme, Russian / English / 中文, Stop button, message editing, chat archive, export and import.

### Quick start

1. **Install LimitKill.** Run the installer and choose your language: English, Русский or 中文. The app installs to `C:\Program Files\LimitKill`.
2. **Choose where to send the task.** Click the model pill at the top of the chat. Add an API key, sign in to a web account, or connect a local model.
3. **Add your project.** Open **Context** on the left, choose the project folder and set files to **Full** or **Skeleton**.
4. **Ask your question.** Type a task, attach files with the paperclip if needed, and press **Send**.
5. **Keep the thread alive.** Use the model pill to switch models, **Stop** to cancel generation, and **Continue in a new chat** when the context is almost full.

### Good to know

- **Full** sends the whole file; **Skeleton** sends its structure — ideal for large projects.
- API providers need a key; web accounts need a normal sign-in in the opened window.
- Local models require Ollama or LM Studio. LimitKill does not bundle a model runtime.
- Installer language and app language are independent; both can be switched.

### Download & updates

Download the latest installer from the project's **GitHub Releases**. There is no automatic updater: install a new version over the old one — your chats, settings and web sessions are preserved.

### Privacy

LimitKill works locally. API requests go only to the provider you choose. Web sessions are isolated per account. API keys are encrypted on disk and never included in the installer.

### License

MIT. See [LICENSE](LICENSE).

---

## Русский

**LimitKill** — спокойный локальный AI-воркспейс для кодинга на Windows. Один чат, переключение между API-провайдерами, локальными моделями и веб-аккаунтами, а также автоматическая подготовка нужного контекста проекта.

### Зачем нужен LimitKill

- **Один чат — много моделей.** Переключайся между Gemini, OpenRouter, Groq, Qwen, Kimi, OpenAI и своим OpenAI-совместимым сервером, не переписывая задачу.
- **Веб-аккаунты.** DeepSeek, Google Gemini, Qwen и Claude работают в изолированных сессиях: у каждого аккаунта свой вход.
- **Локальные модели в один клик.** Если запущены Ollama или LM Studio, LimitKill сам найдёт сервер и добавит модель.
- **Умный контекст проекта.** Выбери папку, отметь файлы и используй режимы **Полный / Скелет / Выкл**. Поиск по содержимому поднимает вверх код, где реально встречаются нужные имена.
- **Длинные чаты не обрываются.** На 95% лимита LimitKill предложит новый чат со снимком прогресса: цель, последнее состояние, недавние сообщения и активные файлы.
- **Файлы прямо в чате.** Прикрепляй текстовые и кодовые файлы, `.docx` и текстовые `.pdf` и задавай вопросы по ним.
- **Приватность по умолчанию.** Чаты, настройки и веб-сессии остаются на компьютере. API-ключи шифруются, телеметрии и аккаунта LimitKill нет.
- **Удобный интерфейс.** Тёплая светлая тема, русский / английский / 中文, кнопка «Стоп», редактирование сообщений, архив чатов, экспорт и импорт.

### Быстрый старт

1. **Установи LimitKill.** Запусти установщик и выбери язык: English, Русский или 中文. Приложение ставится в `C:\Program Files\LimitKill`.
2. **Выбери, куда отправить задачу.** Нажми на плашку модели в шапке чата. Добавь API-ключ, войди в веб-аккаунт или подключи локальную модель.
3. **Добавь проект.** Открой **Контекст** слева, выбери папку проекта и отметь файлы как **Полный** или **Скелет**.
4. **Задай вопрос.** Опиши задачу, при необходимости прикрепи файлы скрепкой и нажми **Отправить**.
5. **Продолжай без потерь.** Переключай модели в шапке, останавливай генерацию кнопкой **Стоп**, а при заполнении контекста нажимай **«Продолжить в новом чате»**.

### Полезно знать

- **Полный** отправляет файл целиком, **Скелет** — только структуру; для больших проектов это экономит контекст.
- API-провайдерам нужен ключ, веб-аккаунтам — обычный вход в открывшемся окне.
- Локальным моделям нужны Ollama или LM Studio. LimitKill не устанавливает рантайм модели.
- Язык установщика и язык приложения независимы; оба можно менять.

### Скачать и обновить

Скачай свежий установщик в **GitHub Releases**. Автообновления нет: установи новую версию поверх старой — чаты, настройки и веб-сессии сохранятся.

### Приватность

LimitKill работает локально. Запросы уходят только выбранному провайдеру. Веб-сессии изолированы по аккаунтам. API-ключи шифруются на диске и не попадают в установщик.

### Лицензия

MIT. См. [LICENSE](LICENSE).

---

## 简体中文

**LimitKill** 是一款安静、注重隐私的 Windows 本地 AI 编程工作台。一个聊天窗口即可连接多个 API 服务商、本地模型和网页账号，并自动整理每个任务需要的项目上下文。

### 为什么选择 LimitKill

- **一个聊天，多种模型。** 在 Gemini、OpenRouter、Groq、Qwen、Kimi、OpenAI 和自建 OpenAI 兼容服务之间切换，无需重写问题。
- **网页账号。** DeepSeek、Google Gemini、Qwen 和 Claude 使用独立会话，每个账号保留自己的登录状态。
- **本地模型，一键连接。** 如果 Ollama 或 LM Studio 正在运行，LimitKill 会自动发现并添加模型。
- **智能项目上下文。** 选择项目文件夹和文件，使用 **完整 / 骨架 / 关闭** 模式。内容搜索会把包含关键名称的代码排在前面。
- **长对话不会中断。** 上下文达到 95% 时，LimitKill 会提示创建带进度快照的新对话：目标、最后状态、最近消息和活动文件都会保留。
- **在聊天中附加文件。** 支持文本、代码、`.docx` 和带文本层的 `.pdf`，可以直接针对文件提问。
- **默认保护隐私。** 聊天、设置和网页会话都保存在本机。API 密钥加密存储，无遥测，也无需 LimitKill 账号。
- **舒适的界面。** 温暖的浅色主题，支持中文 / 英文 / 俄文，包含停止、消息编辑、聊天归档、导出和导入。

### 快速开始

1. **安装 LimitKill。** 运行安装程序并选择语言：English、Русский 或 中文。程序安装在 `C:\Program Files\LimitKill`。
2. **选择任务目标。** 点击聊天顶部的模型标签。添加 API 密钥、登录网页账号，或连接本地模型。
3. **添加项目。** 打开左侧 **上下文**，选择项目文件夹，并把文件设为 **完整** 或 **骨架**。
4. **开始提问。** 描述任务，需要时点击回形针附加文件，然后按 **发送**。
5. **保持进度。** 用顶部标签切换模型，用 **停止** 取消生成；上下文快满时点击 **在新对话中继续**。

### 实用提示

- **完整** 会发送整个文件，**骨架** 只发送结构；大项目建议混合使用。
- API 服务商需要密钥；网页账号需要在打开的窗口中正常登录。
- 本地模型需要 Ollama 或 LM Studio；LimitKill 不内置模型运行时。
- 安装程序语言和应用语言相互独立，都可以切换。

### 下载与更新

请从项目的 **GitHub Releases** 下载最新安装包。没有自动更新：直接覆盖安装新版本即可，聊天、设置和网页会话都会保留。

### 隐私

LimitKill 在本机运行。请求只发送给你选择的服务商，网页会话按账号隔离。API 密钥加密保存在磁盘上，不会包含在安装包中。

### 许可证

MIT。详见 [LICENSE](LICENSE)。

---

<div align="center">

Made with care for long coding sessions.

**LimitKill 1.0.0** · Windows 10/11 · MIT

</div>
