<script lang="ts">
  import { tick } from 'svelte';
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';
  import hljs from 'highlight.js';
  import 'highlight.js/styles/github.css';
  import { open } from '@tauri-apps/plugin-dialog';
  import { getDictionary } from '../i18n/index.js';
  import { bridgeState } from '../state/appState.svelte.js';
  import type { SettingsTab } from '../types.js';
  import TargetSwitcher from './TargetSwitcher.svelte';
  import {
    Send,
    Eraser,
    Copy,
    Bot,
    User,
    Loader2,
    Check,
    ChevronDown,
    PanelLeft,
    Sparkles,
    Square,
    ArrowDown,
    FolderOpen,
    Pencil,
    RotateCcw,
    Paperclip,
    FileText,
    X,
  } from '@lucide/svelte';

  interface Props {
    onToggleSidebar: () => void;
    onOpenSettings: (tab?: SettingsTab) => void;
  }

  let { onToggleSidebar, onOpenSettings }: Props = $props();
  const dict = $derived(getDictionary(bridgeState.config.locale));

  let draft = $state('');
  let scrollEl: HTMLDivElement | undefined = $state();
  let inputEl: HTMLTextAreaElement | undefined = $state();
  let copiedId = $state<string | null>(null);
  let switcherOpen = $state(false);
  let stickToBottom = $state(true);
  let showJump = $state(false);
  let lastThreadId = '';
  let attachments = $state<import('../state/appState.svelte.js').ChatAttachment[]>([]);

  marked.setOptions({ breaks: true, gfm: true });

  const htmlCache = new Map<string, string>();

  function renderMd(raw: string, highlight = true): string {
    if (!raw) return '';
    const cacheKey = `${highlight ? 'h' : 'p'}:${raw}`;
    const cached = htmlCache.get(cacheKey);
    if (cached) return cached;
    const rawHtml = marked.parse(raw) as string;
    const cleanHtml = DOMPurify.sanitize(rawHtml);
    const div = document.createElement('div');
    div.innerHTML = cleanHtml;
    if (highlight) {
      div.querySelectorAll('pre').forEach((pre) => {
        const code = pre.querySelector('code');
        if (!code) return;
        try {
          hljs.highlightElement(code as HTMLElement);
        } catch {
          // ignore exotic language
        }
        const langClass = Array.from(code.classList).find((c) => c.startsWith('language-'));
        const lang = langClass ? langClass.replace('language-', '') : 'code';
        const head = document.createElement('div');
        head.className = 'code-head';
        const label = document.createElement('span');
        label.className = 'code-lang';
        label.textContent = lang;
        const button = document.createElement('button');
        button.type = 'button';
        button.className = 'code-copy';
        button.title = 'Copy code';
        button.setAttribute('aria-label', 'Copy code');
        button.innerHTML =
          '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>';
        head.append(label, button);
        pre.prepend(head);
      });
    }
    const html = div.innerHTML;
    if (htmlCache.size > 200) htmlCache.clear();
    htmlCache.set(cacheKey, html);
    return html;
  }

  function formatTime(ts: number): string {
    return new Date(ts * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }

  async function onMessagesClick(e: MouseEvent) {
    const target = e.target as Element | null;
    const button = target?.closest<HTMLButtonElement>('.code-copy');
    if (!button) return;
    const code = button.closest('pre')?.querySelector('code');
    if (!code) return;
    await navigator.clipboard.writeText((code as HTMLElement).innerText);
    button.dataset.copied = 'true';
    bridgeState.notify(dict.chat.codeCopied);
    setTimeout(() => {
      delete button.dataset.copied;
    }, 1600);
  }

  async function pickProjectFolder() {
    const selected = await open({ directory: true, multiple: false, title: dict.rag.pickFolder });
    if (typeof selected === 'string') await bridgeState.setProjectRoot(selected);
  }

  function providerLabel(name: string | null | undefined): string {
    const label = name ?? dict.chat.assistant;
    // Исторические сообщения могли сохранить старое имя продукта.
    return label.replace(/DeepBridge/g, 'LimitKill');
  }

  const partialHtml = $derived(
    bridgeState.partialText ? renderMd(bridgeState.partialText, false) : ''
  );

  const activeApi = $derived(bridgeState.activeApiProfile);
  const activeWeb = $derived(bridgeState.activeWebProfile);
  const isWeb = $derived(bridgeState.config.active_mode === 'web');

  const targetName = $derived(
    isWeb ? activeWeb?.name ?? dict.chat.noProfiles : activeApi?.name ?? dict.chat.noProfiles
  );
  const webService = $derived(
    activeWeb
      ? { deepseek: 'DeepSeek', gemini: 'Google Gemini', qwen: 'Qwen', claude: 'Claude', chatgpt: 'ChatGPT', kimi: 'Kimi' }[
          activeWeb.kind ?? 'deepseek'
        ] ?? 'Web'
      : 'Web'
  );
  const targetModel = $derived(
    isWeb ? `${webService} · Web` : activeApi?.model === 'auto' ? 'auto · free' : activeApi?.model ?? ''
  );

  const targetState = $derived.by(() => {
    if (isWeb) {
      const p = activeWeb;
      if (!p) return 'idle';
      if (p.limit_until ? p.limit_until * 1000 > Date.now() : p.limit_hit) return 'warn';
      if (p.last_error) return 'bad';
      return 'ok';
    }
    const p = activeApi;
    if (!p) return 'idle';
    if (p.cooldown_until && p.cooldown_until * 1000 > Date.now()) return 'warn';
    if (!p.api_key && p.kind !== 'custom_open_ai') return 'idle';
    if (p.last_error) return 'bad';
    return 'ok';
  });

  const hasApiTarget = $derived(
    bridgeState.config.api_profiles.some(
      (p) => p.enabled && (p.kind === 'custom_open_ai' || p.api_key.trim().length > 0)
    ) || bridgeState.config.web_profiles.some((p) => !!p.last_used_at)
  );
  const hasProject = $derived(!!bridgeState.config.project_root);
  const chatBudget = $derived(bridgeState.config.chat_context_budget || 128000);
  const chatTokens = $derived(
    bridgeState.contextPreview.total_tokens +
      bridgeState.messages.reduce((sum, m) => sum + (m.tokens_approx || 0), 0) +
      Math.ceil(draft.length / 4)
  );
  const chatPercent = $derived(
    Math.min(100, Math.round((chatTokens / Math.max(1, chatBudget)) * 100))
  );
  let handoffBusy = $state(false);

  async function continueInNewThread() {
    if (handoffBusy || bridgeState.messages.length === 0) return;
    handoffBusy = true;
    const ok = await bridgeState.handoffToNewThread(
      `Контекст предыдущего чата был заполнен примерно на ${chatPercent}%.`
    );
    handoffBusy = false;
    if (ok) {
      draft = '';
      attachments = [];
      bridgeState.notify(dict.chat.handoffCreated);
    }
  }

  async function pickAttachments() {
    if (bridgeState.sendingPrompt) return;
    const selected = await open({
      multiple: true,
      directory: false,
      title: dict.chat.attachFiles,
      filters: [
        {
          name: 'Text & code',
          extensions: [
            'txt', 'md', 'svelte', 'ts', 'tsx', 'js', 'jsx', 'rs', 'py', 'go', 'java',
            'c', 'h', 'cpp', 'hpp', 'cs', 'json', 'toml', 'yaml', 'yml', 'html', 'css',
            'scss', 'less', 'sql', 'sh', 'ps1', 'bat', 'rtf', 'xml', 'vue', 'docx', 'pdf',
          ],
        },
      ],
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    const files = await bridgeState.readChatAttachments(paths);
    if (files.length) {
      attachments = [...attachments, ...files].slice(0, 8);
    } else if (bridgeState.lastError) {
      bridgeState.notify(bridgeState.lastError);
    }
  }

  function removeAttachment(path: string) {
    attachments = attachments.filter((a) => a.path !== path);
  }

  async function submitPrompt() {
    const text = draft.trim();
    if ((!text && attachments.length === 0) || bridgeState.sendingPrompt) return;
    const sentAttachments = attachments;
    draft = '';
    attachments = [];
    const result = await bridgeState.sendChatPrompt(
      text || 'Проанализируй приложенные файлы.',
      sentAttachments
    );
    if (result === 'error') {
      draft = text;
      attachments = sentAttachments;
      bridgeState.notify(dict.chat.draftRestored);
    } else if (result === 'cancelled') {
      draft = text;
      attachments = sentAttachments;
      bridgeState.notify(dict.chat.stopped);
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      void submitPrompt();
    }
  }

  async function copyContext() {
    const pkt = await bridgeState.buildHandoffPacket(draft.trim() || undefined);
    if (!pkt) return;
    await navigator.clipboard.writeText(pkt);
    bridgeState.notify(dict.chat.handoffCopied);
  }

  async function copyMessage(m: { id: string; content: string }) {
    await navigator.clipboard.writeText(m.content);
    copiedId = m.id;
    setTimeout(() => {
      if (copiedId === m.id) copiedId = null;
    }, 1800);
  }

  async function editUserMessage(m: { id: string; content: string }, index: number) {
    if (bridgeState.sendingPrompt) return;
    if (index < bridgeState.messages.length - 1 && !confirm(dict.chat.rewindConfirm)) return;
    const text = m.content;
    if (!(await bridgeState.truncateThread(bridgeState.threadsStore.active_thread_id, m.id))) return;
    draft = text;
    await tick();
    inputEl?.focus();
  }

  async function regenerateMessage(m: { id: string }, index: number) {
    if (bridgeState.sendingPrompt) return;
    const previousUser = bridgeState.messages
      .slice(0, index)
      .reverse()
      .find((x) => x.role === 'user');
    if (!previousUser) return;
    if (index < bridgeState.messages.length - 1 && !confirm(dict.chat.rewindConfirm)) return;
    const text = previousUser.content;
    if (
      !(await bridgeState.truncateThread(
        bridgeState.threadsStore.active_thread_id,
        previousUser.id
      ))
    ) {
      return;
    }
    const result = await bridgeState.sendChatPrompt(text);
    if (result !== 'ok') {
      draft = text;
      if (result === 'cancelled') bridgeState.notify(dict.chat.stopped);
    }
  }

  function onMessagesScroll() {
    const el = scrollEl;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    stickToBottom = distance < 80;
    showJump = !stickToBottom;
  }

  function jumpToLatest() {
    stickToBottom = true;
    showJump = false;
    scrollEl?.scrollTo({ top: scrollEl.scrollHeight, behavior: 'smooth' });
  }

  $effect(() => {
    const count = bridgeState.messages.length;
    const lastContent = count > 0 ? bridgeState.messages[count - 1].content : '';
    const streaming = bridgeState.partialText;
    void lastContent;
    void streaming;
    if (scrollEl && stickToBottom) {
      scrollEl.scrollTop = scrollEl.scrollHeight;
    }
  });

  $effect(() => {
    const id = bridgeState.threadsStore.active_thread_id;
    if (id !== lastThreadId) {
      lastThreadId = id;
      stickToBottom = true;
      showJump = false;
      queueMicrotask(() => {
        if (scrollEl) scrollEl.scrollTop = scrollEl.scrollHeight;
      });
    }
  });

  $effect(() => {
    const value = draft;
    void value;
    if (inputEl) {
      inputEl.style.height = 'auto';
      inputEl.style.height = `${Math.min(inputEl.scrollHeight, 220)}px`;
    }
  });
</script>

<svelte:window
  onkeydown={(e) => {
    if (e.key === 'Escape') {
      if (switcherOpen) {
        switcherOpen = false;
      } else if (bridgeState.sendingPrompt) {
        void bridgeState.cancelPrompt();
      }
    }
  }}
/>

<section class="chat">
  <header class="chat-header">
    <button type="button" class="icon-btn" onclick={onToggleSidebar} title={dict.rag.title}>
      <PanelLeft size={18} />
    </button>

    <div class="target-wrap">
      <button type="button" class="target-pill" onclick={() => (switcherOpen = !switcherOpen)}>
        <span class="target-dot {targetState}"></span>
        <span class="target-text">
          <strong>{targetName}</strong>
          <small>{targetModel}</small>
        </span>
        <ChevronDown size={15} class="chevron {switcherOpen ? 'open' : ''}" />
      </button>
      <TargetSwitcher
        open={switcherOpen}
        onClose={() => (switcherOpen = false)}
        onManage={() => onOpenSettings('providers')}
      />
    </div>

    {#if bridgeState.streaming}
      <span class="live-badge"><Sparkles size={12} /> {dict.chat.streaming}</span>
    {/if}

    <div class="header-actions">
      <button type="button" class="icon-btn" onclick={copyContext} title={dict.chat.copyHandoff}>
        <Copy size={16} />
      </button>
      <button
        type="button"
        class="icon-btn danger"
        onclick={() => bridgeState.clearActiveThread()}
        title={dict.threads.clearChat}
      >
        <Eraser size={16} />
      </button>
    </div>
  </header>

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="messages"
    role="log"
    aria-live="polite"
    bind:this={scrollEl}
    onclick={onMessagesClick}
    onscroll={onMessagesScroll}
  >
    {#if bridgeState.messages.length === 0 && !bridgeState.sendingPrompt}
      <div class="empty-state">
        <div class="empty-mark">LK</div>
        <h2>{dict.chat.emptyTitle}</h2>
        <p>{dict.chat.emptyDesc}</p>
        <div class="setup-steps">
          <div class="setup-step" class:done={hasApiTarget}>
            <span class="step-num">{#if hasApiTarget}<Check size={13} />{:else}1{/if}</span>
            <div class="step-text">
              <strong>{dict.onboarding.providerTitle}</strong>
              <small>{dict.onboarding.providerDesc}</small>
            </div>
            <button type="button" class="soft-btn" onclick={() => onOpenSettings('providers')}>
              {hasApiTarget ? dict.common.edit : dict.onboarding.setup}
            </button>
          </div>
          <div class="setup-step" class:done={hasProject}>
            <span class="step-num">{#if hasProject}<Check size={13} />{:else}2{/if}</span>
            <div class="step-text">
              <strong>{dict.onboarding.contextTitle}</strong>
              <small>{bridgeState.config.project_root ?? dict.onboarding.contextDesc}</small>
            </div>
            <button type="button" class="soft-btn" onclick={pickProjectFolder}>
              <FolderOpen size={14} /> {hasProject ? dict.rag.changeFolder : dict.onboarding.pickFolder}
            </button>
          </div>
          <div class="setup-step">
            <span class="step-num">3</span>
            <div class="step-text">
              <strong>{dict.onboarding.writeTitle}</strong>
              <small>{dict.onboarding.writeDesc}</small>
            </div>
          </div>
        </div>
      </div>
    {:else}
      {#each bridgeState.messages as m, i (m.id)}
        <article class="msg {m.role}">
          <div class="avatar">
            {#if m.role === 'user'} <User size={16} /> {:else} <Bot size={16} /> {/if}
          </div>
          <div class="msg-main">
            <div class="msg-head">
              {#if m.role !== 'user'}
                <span class="role">{providerLabel(m.provider_name)}</span>
                {#if m.model_name}<span class="model">{m.model_name}</span>{/if}
              {/if}
              <span class="time" title={new Date(m.timestamp * 1000).toLocaleString()}>{formatTime(m.timestamp)}</span>
              {#if m.role === 'user'}
                <button
                  type="button"
                  class="copy-msg"
                  title={dict.chat.edit}
                  aria-label={dict.chat.edit}
                  onclick={() => editUserMessage(m, i)}
                >
                  <Pencil size={13} />
                </button>
              {:else if i === bridgeState.messages.length - 1 && !bridgeState.sendingPrompt}
                <button
                  type="button"
                  class="copy-msg"
                  title={dict.chat.regenerate}
                  aria-label={dict.chat.regenerate}
                  onclick={() => regenerateMessage(m, i)}
                >
                  <RotateCcw size={13} />
                </button>
              {/if}
              <button type="button" class="copy-msg" title={dict.chat.copy} onclick={() => copyMessage(m)}>
                {#if copiedId === m.id} <Check size={13} /> {:else} <Copy size={13} /> {/if}
              </button>
            </div>
            <div class="markdown-body">
              {@html renderMd(m.content)}
            </div>
            {#if m.attachments?.length}
              <div class="msg-files">
                {#each m.attachments as a (a.path)}
                  <span class="msg-file"><FileText size={11} /> {a.name}</span>
                {/each}
              </div>
            {/if}
          </div>
        </article>
      {/each}
    {/if}

    {#if bridgeState.sendingPrompt && bridgeState.partialText}
      <article class="msg assistant streaming-msg">
        <div class="avatar"><Bot size={16} /></div>
        <div class="msg-main">
          <div class="msg-head">
            <span class="role">{providerLabel(bridgeState.partialProfile)}</span>
            <span class="model streaming"><Loader2 size={12} class="spin" /> {dict.chat.streaming}</span>
          </div>
          <div class="markdown-body">{@html partialHtml}</div>
        </div>
      </article>
    {:else if bridgeState.sendingPrompt}
      <article class="msg assistant loading-msg">
        <div class="avatar"><Bot size={16} /></div>
        <div class="msg-main">
          <div class="loading-line">
            <Loader2 size={15} class="spin" />
            <span>{dict.chat.thinking}</span>
          </div>
        </div>
      </article>
    {/if}
  </div>

  {#if showJump}
    <button type="button" class="jump-btn" onclick={jumpToLatest}>
      <ArrowDown size={14} /> {dict.chat.toLatest}
    </button>
  {/if}

  {#if chatPercent >= 95 && bridgeState.messages.length > 0}
    <div class="context-warning" class:critical={chatPercent >= 100}>
      <div class="warning-text">
        <strong>{chatPercent >= 100 ? dict.chat.contextFull : dict.chat.contextAlmostFull}</strong>
        <span>{chatTokens} / {chatBudget} {dict.rag.tokens} · {100 - chatPercent}% {dict.chat.contextLeft}</span>
      </div>
      <div class="warning-actions">
        <button type="button" class="mini-primary" onclick={continueInNewThread} disabled={handoffBusy}>
          {#if handoffBusy}<Loader2 size={13} class="spin" />{:else}<ArrowDown size={13} />{/if}
          {dict.chat.continueInNewChat}
        </button>
        <button type="button" class="mini-ghost" onclick={copyContext}>
          {dict.chat.copyHandoff}
        </button>
      </div>
    </div>
  {/if}

  <div class="composer-wrap">
    {#if attachments.length}
      <div class="attachment-bar">
        {#each attachments as a (a.path)}
          <span class="attachment-chip" title={a.path}>
            <FileText size={11} />
            <span>{a.name}</span>
            <small>{Math.ceil(a.size_bytes / 1024)} KB</small>
            <button type="button" onclick={() => removeAttachment(a.path)} aria-label={dict.common.close}>
              <X size={11} />
            </button>
          </span>
        {/each}
      </div>
    {/if}
    <div class="composer" class:busy={bridgeState.sendingPrompt}>
      <button
        type="button"
        class="attach-btn"
        onclick={pickAttachments}
        disabled={bridgeState.sendingPrompt}
        title={dict.chat.attachFiles}
        aria-label={dict.chat.attachFiles}
      >
        <Paperclip size={16} />
      </button>
      <textarea
        bind:this={inputEl}
        rows="1"
        bind:value={draft}
        onkeydown={onKey}
        placeholder={dict.chat.placeholder}
        disabled={bridgeState.sendingPrompt}
      ></textarea>
      <button
        type="button"
        class="send-btn"
        class:stop={bridgeState.sendingPrompt}
        onclick={bridgeState.sendingPrompt ? () => bridgeState.cancelPrompt() : submitPrompt}
        disabled={!bridgeState.sendingPrompt && !draft.trim() && attachments.length === 0}
        title={bridgeState.sendingPrompt ? dict.chat.stop : dict.chat.send}
        aria-label={bridgeState.sendingPrompt ? dict.chat.stop : dict.chat.send}
      >
        {#if bridgeState.sendingPrompt}
          <Square size={14} />
          <span>{dict.chat.stop}</span>
        {:else}
          <Send size={17} />
        {/if}
      </button>
    </div>
    <div class="composer-meta">
      <span>{dict.chat.enterHint}</span>
      <span class="meta-right">
        {#if chatTokens > 0}
          <span class="context-chip" class:warn={chatPercent >= 90} class:bad={chatPercent >= 100}>
            {chatTokens} / {chatBudget} {dict.chat.contextTokens}
          </span>
        {/if}
      </span>
    </div>
  </div>
</section>

<style>
  .chat {
    position: relative;
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    height: 100%;
    background: transparent;
  }

  .chat-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--line);
    background: rgba(255, 253, 249, 0.7);
    backdrop-filter: blur(10px);
  }

  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: 9px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .icon-btn:hover {
    background: var(--panel);
    color: var(--ink);
  }
  .icon-btn.danger:hover {
    background: var(--danger-soft);
    color: var(--danger);
  }

  .target-wrap {
    position: relative;
  }
  .target-pill {
    display: flex;
    align-items: center;
    gap: 9px;
    max-width: min(520px, 52vw);
    padding: 6px 10px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--bg-elev);
    cursor: pointer;
    box-shadow: var(--shadow-sm);
  }
  .target-pill:hover {
    border-color: var(--line-strong);
  }
  .target-text {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    min-width: 0;
  }
  .target-text strong {
    font-size: 12.5px;
    font-weight: 600;
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .target-text small {
    font-size: 10.5px;
    color: var(--muted);
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .target-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
    flex-shrink: 0;
  }
  .target-dot.ok {
    background: var(--ok);
    box-shadow: 0 0 0 3px var(--ok-soft);
  }
  .target-dot.warn {
    background: var(--warm);
    box-shadow: 0 0 0 3px var(--warm-soft);
  }
  .target-dot.bad {
    background: var(--danger);
    box-shadow: 0 0 0 3px var(--danger-soft);
  }
  .chevron {
    color: var(--muted);
    transition: transform 0.15s ease;
  }
  .chevron.open {
    transform: rotate(180deg);
  }

  .live-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-left: 4px;
    font-size: 10.5px;
    font-weight: 600;
    color: var(--accent-strong);
    background: var(--accent-soft);
    padding: 3px 8px;
    border-radius: 999px;
  }

  .header-actions {
    display: flex;
    gap: 2px;
    margin-left: auto;
  }

  .messages {
    flex: 1;
    overflow-y: auto;
    padding: 24px max(20px, calc((100% - 840px) / 2)) 20px;
    display: flex;
    flex-direction: column;
    gap: 20px;
    scrollbar-width: thin;
    user-select: text;
    -webkit-user-select: text;
  }

  .empty-state {
    margin: auto;
    width: min(560px, 100%);
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 24px 8px;
  }
  .empty-mark {
    width: 54px;
    height: 54px;
    border-radius: 16px;
    background: var(--accent);
    color: #fbfdf9;
    font-weight: 800;
    font-size: 18px;
    letter-spacing: 0.03em;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 8px 22px rgba(79, 122, 90, 0.28);
    margin-bottom: 10px;
  }
  .empty-state h2 {
    margin: 0;
    font-size: 21px;
    font-weight: 700;
    color: var(--ink);
  }
  .empty-state p {
    margin: 4px 0 14px;
    font-size: 13.5px;
    line-height: 1.55;
    color: var(--ink-2);
  }
  .setup-steps {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 6px;
    text-align: left;
  }
  .setup-step {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 10px;
    border: 1px solid var(--line);
    border-radius: 11px;
    background: var(--bg-elev);
    box-shadow: var(--shadow-sm);
  }
  .setup-step.done {
    background: var(--accent-tint);
    border-color: rgba(79, 122, 90, 0.2);
  }
  .step-num {
    width: 24px;
    height: 24px;
    border-radius: 8px;
    background: var(--panel);
    color: var(--muted);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 700;
    flex-shrink: 0;
  }
  .setup-step.done .step-num {
    background: var(--accent);
    color: #fbfdf9;
  }
  .step-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .step-text strong {
    font-size: 12px;
    color: var(--ink);
  }
  .step-text small {
    font-size: 10.5px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .soft-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--line);
    background: var(--bg-elev);
    color: var(--ink-2);
    border-radius: 9px;
    padding: 6px 10px;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }
  .soft-btn:hover {
    border-color: var(--accent);
    color: var(--accent-strong);
    background: var(--accent-tint);
  }

  .msg {
    display: flex;
    gap: 10px;
    width: 100%;
    max-width: 820px;
    margin: 0 auto;
  }
  .msg.user {
    flex-direction: row-reverse;
  }
  .avatar {
    width: 28px;
    height: 28px;
    border-radius: 9px;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    margin-top: 2px;
  }
  .user .avatar {
    background: var(--accent);
    color: #fbfdf9;
  }
  .assistant .avatar {
    background: var(--panel-strong);
    color: var(--ink-2);
  }

  .msg-main {
    min-width: 0;
    max-width: min(720px, calc(100% - 40px));
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .user .msg-main {
    align-items: flex-end;
    max-width: min(560px, calc(100% - 40px));
  }
  .msg-head {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 11px;
  }
  .user .msg-head {
    flex-direction: row-reverse;
  }
  .role {
    font-weight: 600;
    color: var(--ink-2);
  }
  .time {
    font-size: 10px;
    color: var(--muted);
  }
  .model {
    font-size: 10px;
    color: var(--muted);
    background: var(--panel);
    border-radius: 5px;
    padding: 1px 6px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .model.streaming {
    color: var(--accent-strong);
    background: var(--accent-soft);
  }
  .copy-msg {
    border: none;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    padding: 2px;
    border-radius: 5px;
    opacity: 0;
  }
  .msg:hover .copy-msg {
    opacity: 1;
  }
  .copy-msg:hover {
    color: var(--accent-strong);
  }

  .user .msg-main {
    background: var(--accent-soft);
    border: 1px solid rgba(79, 122, 90, 0.18);
    border-radius: 14px 14px 4px 14px;
    padding: 10px 13px;
  }
  .user .markdown-body {
    color: var(--ink);
  }
  .assistant .msg-main {
    background: var(--bg-elev);
    border: 1px solid var(--line);
    border-radius: 14px 14px 14px 4px;
    padding: 12px 15px;
    box-shadow: var(--shadow-sm);
  }

  .loading-line {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
    font-size: 13px;
    padding: 6px 2px;
  }

  .composer-wrap {
    padding: 10px max(20px, calc((100% - 840px) / 2)) 16px;
    background: linear-gradient(to top, var(--bg) 70%, rgba(245, 242, 234, 0));
  }
  .composer {
    max-width: 840px;
    margin: 0 auto;
    display: flex;
    align-items: flex-end;
    gap: 8px;
    padding: 9px 10px 9px 14px;
    background: var(--bg-elev);
    border: 1px solid var(--line);
    border-radius: 16px;
    box-shadow: var(--shadow-sm);
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
  }
  .composer:focus-within {
    border-color: rgba(79, 122, 90, 0.45);
    box-shadow: 0 0 0 3px rgba(79, 122, 90, 0.12), var(--shadow-sm);
  }
  .composer.busy {
    opacity: 0.85;
  }
  textarea {
    flex: 1;
    border: none;
    outline: none;
    resize: none;
    background: transparent;
    font-size: 14.5px;
    line-height: 1.5;
    padding: 7px 0;
    max-height: 220px;
    color: var(--ink);
  }
  textarea::placeholder {
    color: var(--muted);
  }
  .context-warning {
    max-width: 840px;
    margin: 0 auto 8px;
    padding: 8px 10px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    border: 1px solid #dfc08a;
    border-radius: 9px;
    background: #fbf2df;
    color: #6d4d22;
  }
  .context-warning.critical {
    border-color: #d59a8f;
    background: #fbebe6;
    color: #7d342a;
  }
  .warning-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .warning-text strong {
    font-size: 11px;
  }
  .warning-text span {
    font-size: 9.5px;
    opacity: 0.8;
  }
  .warning-actions {
    display: flex;
    gap: 5px;
    flex-shrink: 0;
  }
  .mini-primary,
  .mini-ghost {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 5px 8px;
    border-radius: 6px;
    font-size: 10px;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }
  .mini-primary {
    border: 1px solid #b98a4e;
    background: #8a5a2b;
    color: #fff8ee;
  }
  .mini-ghost {
    border: 1px solid #dfc08a;
    background: transparent;
    color: #6d4d22;
  }
  .mini-primary:disabled {
    opacity: 0.65;
  }
  .context-chip.warn {
    background: #fbf2df;
    color: #8a5a2b;
  }
  .context-chip.bad {
    background: var(--danger-soft);
    color: var(--danger);
  }

  .attach-btn {
    width: 34px;
    height: 34px;
    flex-shrink: 0;
    border: 1px solid var(--line);
    border-radius: 9px;
    background: var(--bg-elev);
    color: var(--muted);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
  }
  .attach-btn:hover:not(:disabled) {
    border-color: var(--accent);
    color: var(--accent-strong);
    background: var(--accent-tint);
  }
  .attach-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .attachment-bar {
    max-width: 840px;
    margin: 0 auto 6px;
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }
  .attachment-chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    max-width: 260px;
    padding: 4px 7px;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: var(--bg-elev);
    color: var(--ink-2);
    font-size: 10.5px;
  }
  .attachment-chip span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .attachment-chip small {
    color: var(--muted);
    font-size: 9px;
    flex-shrink: 0;
  }
  .attachment-chip button {
    border: none;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    padding: 0;
    display: inline-flex;
  }
  .attachment-chip button:hover {
    color: var(--danger);
  }
  .msg-files {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 7px;
  }
  .msg-file {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 6px;
    border-radius: 5px;
    background: var(--panel);
    color: var(--muted);
    font-size: 9.5px;
  }

  .send-btn {
    width: 36px;
    height: 36px;
    flex-shrink: 0;
    border: none;
    border-radius: 11px;
    background: var(--accent);
    color: #fbfdf9;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: background 0.15s ease, transform 0.1s ease;
  }
  .send-btn:hover:not(:disabled) {
    background: var(--accent-strong);
  }
  .send-btn:active:not(:disabled) {
    transform: scale(0.96);
  }
  .send-btn:disabled {
    background: var(--panel-strong);
    color: var(--muted);
    cursor: not-allowed;
  }
  .send-btn.stop {
    width: auto;
    padding: 0 12px;
    gap: 6px;
    background: var(--danger);
    color: #fff8f5;
    font-size: 12px;
    font-weight: 600;
  }
  .send-btn.stop:hover {
    background: #913b32;
  }

  .jump-btn {
    position: absolute;
    left: 50%;
    bottom: 104px;
    transform: translateX(-50%);
    z-index: 5;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 11px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--bg-elev);
    color: var(--ink-2);
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    box-shadow: var(--shadow-sm);
  }
  .jump-btn:hover {
    border-color: var(--accent);
    color: var(--accent-strong);
  }

  .composer-meta {
    max-width: 840px;
    margin: 6px auto 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    font-size: 10.5px;
    color: var(--muted);
  }
  .meta-right {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .context-chip {
    background: var(--panel);
    border-radius: 999px;
    padding: 2px 8px;
    white-space: nowrap;
  }

  .markdown-body {
    font-size: 15px;
    line-height: 1.7;
    color: var(--ink);
    max-width: 68ch;
    word-wrap: break-word;
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
    cursor: text;
  }
  .markdown-body :global(p) {
    margin: 0 0 10px;
  }
  .markdown-body :global(p:last-child) {
    margin-bottom: 0;
  }
  .markdown-body :global(h1),
  .markdown-body :global(h2),
  .markdown-body :global(h3) {
    margin: 18px 0 8px;
    font-weight: 700;
    color: var(--ink);
    line-height: 1.3;
  }
  .markdown-body :global(h1) {
    font-size: 20px;
  }
  .markdown-body :global(h2) {
    font-size: 17px;
  }
  .markdown-body :global(h3) {
    font-size: 15px;
  }
  .markdown-body :global(ul),
  .markdown-body :global(ol) {
    margin: 8px 0;
    padding-left: 22px;
  }
  .markdown-body :global(li) {
    margin: 3px 0;
  }
  .markdown-body :global(blockquote) {
    border-left: 3px solid var(--accent);
    margin: 10px 0;
    padding: 6px 14px;
    background: var(--accent-tint);
    border-radius: 0 8px 8px 0;
    color: var(--ink-2);
  }
  .markdown-body :global(table) {
    border-collapse: collapse;
    margin: 10px 0;
    width: 100%;
    font-size: 12.5px;
  }
  .markdown-body :global(th),
  .markdown-body :global(td) {
    border: 1px solid var(--line);
    padding: 6px 9px;
    text-align: left;
  }
  .markdown-body :global(th) {
    background: var(--panel);
  }
  .markdown-body :global(a) {
    color: var(--accent-strong);
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .markdown-body :global(a:hover) {
    color: var(--warm);
  }
  .markdown-body :global(hr) {
    border: none;
    border-top: 1px solid var(--line);
    margin: 14px 0;
  }
  .markdown-body :global(pre) {
    background: #f2eee5;
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 0;
    overflow: hidden;
    margin: 10px 0;
    max-width: 100%;
  }
  .markdown-body :global(pre .code-head) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 10px;
    background: #eae4d8;
    border-bottom: 1px solid var(--line);
  }
  .markdown-body :global(.code-lang) {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .markdown-body :global(.code-copy) {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg-elev);
    color: var(--ink-2);
    padding: 3px 6px;
    cursor: pointer;
  }
  .markdown-body :global(.code-copy:hover) {
    border-color: var(--accent);
    color: var(--accent-strong);
  }
  .markdown-body :global(.code-copy[data-copied='true']) {
    color: var(--ok);
    border-color: var(--ok);
  }
  .markdown-body :global(pre code) {
    display: block;
    background: transparent;
    padding: 12px 14px;
    overflow-x: auto;
    color: var(--ink);
    font-size: 12.5px;
    line-height: 1.6;
  }
  .markdown-body :global(code) {
    background: var(--panel);
    border-radius: 5px;
    padding: 1px 5px;
    font-size: 0.9em;
    color: #7a4a2f;
  }

  @media (max-width: 1100px) {
    .messages {
      padding: 20px 5% 16px;
    }
    .composer-wrap {
      padding: 10px 5% 14px;
    }
  }
  @media (max-width: 820px) {
    .target-text small {
      display: none;
    }
    .live-badge {
      display: none;
    }
    .messages {
      padding: 16px 12px;
    }
    .composer-wrap {
      padding: 8px 10px 12px;
    }
    .composer-meta > span:first-child {
      display: none;
    }
  }
</style>
