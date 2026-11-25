<script lang="ts">
  import { getDictionary } from '../i18n/index.js';
  import {
    bridgeState,
    type ApiProfile,
    type LocalModelServer,
    type ProviderKind,
    type ProviderTestResult,
  } from '../state/appState.svelte.js';
  import type { SettingsTab } from '../types.js';
  import { APP_VERSION } from '../types.js';
  import WebBridge from './WebBridge.svelte';
  import ModelList from './ModelList.svelte';
  import { focusTrap } from '../actions/focusTrap.js';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import {
    X,
    Plus,
    Trash2,
    Copy,
    Check,
    Settings2,
    Globe2,
    SlidersHorizontal,
    Heart,
    FlaskConical,
    Pencil,
    Loader2,
    RefreshCw,
    AlertTriangle,
    ChevronDown,
    Eye,
    EyeOff,
    HardDrive,
  } from '@lucide/svelte';

  interface Props {
    open: boolean;
    initialTab?: SettingsTab;
    onClose: () => void;
  }
  let { open, initialTab = 'providers', onClose }: Props = $props();

  const dict = $derived(getDictionary(bridgeState.config.locale));

  let activeTab = $state<SettingsTab>('providers');
  let showForm = $state(false);
  let editingId = $state<string | undefined>(undefined);
  let name = $state('');
  let kind = $state<ProviderKind>('gemini');
  let apiKey = $state('');
  let baseUrl = $state('https://generativelanguage.googleapis.com/v1beta');
  let model = $state('gemini-3.5-flash-lite');
  let enabled = $state(true);
  let formModels = $state<string[]>([]);
  let detecting = $state(false);
  let formError = $state('');
  let testingId = $state<string | null>(null);
  let testResults = $state<Record<string, ProviderTestResult>>({});
  let testingAll = $state(false);
  let showKey = $state(false);
  let localServers = $state<LocalModelServer[]>([]);
  let discoveringLocal = $state(false);
  let localScanned = $state(false);

  async function discoverLocal() {
    discoveringLocal = true;
    localScanned = true;
    localServers = await bridgeState.discoverLocalModels();
    discoveringLocal = false;
  }

  async function addLocalModel(server: LocalModelServer, model: string) {
    const ok = await bridgeState.saveApiProfile({
      name: `${server.name} · ${model}`,
      kind: 'custom_open_ai',
      apiKey: '',
      baseUrl: server.base_url,
      model,
      enabled: true,
    });
    if (ok) bridgeState.notify(dict.settings.localAdded);
  }
  let expandedModels = $state<string | null>(null);
  let copiedWallet = $state<string | null>(null);
  let budget = $state(60000);
  let chatBudget = $state(128000);
  let savingBudget = $state(false);

  const wallets = $derived([
    {
      key: 'btc',
      label: dict.settings.btc,
      address: 'bc1q2ka70s4vtmrskandqj8l4d6n3kdxyxa7kf3wf7',
      note: 'Bitcoin',
    },
    {
      key: 'usdt',
      label: dict.settings.usdtTrc20,
      address: 'TUjY9p6oxKmeCQwNZwMfHqHdXuabaHpgT7',
      note: dict.settings.trc20Note,
    },
  ]);

  const tabs = $derived([
    { id: 'providers' as SettingsTab, label: dict.settings.providersTab, icon: Settings2 },
    { id: 'web' as SettingsTab, label: dict.settings.webTab, icon: Globe2 },
    { id: 'general' as SettingsTab, label: dict.settings.generalTab, icon: SlidersHorizontal },
    { id: 'donate' as SettingsTab, label: dict.settings.donateTab, icon: Heart },
  ]);

  const kindTip = $derived(
    kind === 'gemini'
      ? dict.settings.geminiTip
      : kind === 'open_router'
        ? dict.settings.orTip
        : kind === 'groq'
          ? dict.settings.groqTip
          : dict.settings.customTip
  );

  const activeTabTitle = $derived(
    activeTab === 'providers'
      ? dict.settings.providersTitle
      : activeTab === 'web'
        ? dict.web.title
        : activeTab === 'general'
          ? dict.settings.generalTab
          : dict.settings.donateTitle
  );

  const activeTabSubtitle = $derived(
    activeTab === 'providers'
      ? dict.settings.providersSubtitle
      : activeTab === 'web'
        ? dict.web.subtitle
        : activeTab === 'general'
          ? dict.settings.contextBudgetHint
          : dict.settings.donateText
  );

  let wasOpen = false;
  $effect(() => {
    const isOpen = open;
    if (isOpen && !wasOpen) {
      activeTab = initialTab;
      showForm = false;
      resetForm();
      budget = bridgeState.config.max_context_tokens_budget;
      chatBudget = bridgeState.config.chat_context_budget ?? 128000;
    }
    wasOpen = isOpen;
  });

  $effect(() => {
    if (open) activeTab = initialTab;
  });

  function kindLabel(k: ProviderKind): string {
    if (k === 'gemini') return 'Gemini';
    if (k === 'open_router') return 'OpenRouter';
    if (k === 'groq') return 'Groq';
    if (k === 'custom_open_ai') return dict.settings.presetCustom;
    return 'DeepSeek Web';
  }

  function markOf(value: string): string {
    const clean = value.replace(/[^\p{L}\p{N}]+/gu, ' ').trim();
    const parts = clean.split(/\s+/).filter(Boolean);
    if (parts.length === 0) return 'AI';
    return (parts[0][0] + (parts[1]?.[0] ?? '')).toUpperCase();
  }

  function blankForm() {
    editingId = undefined;
    name = '';
    kind = 'gemini';
    apiKey = '';
    baseUrl = 'https://generativelanguage.googleapis.com/v1beta';
    model = 'gemini-3.5-flash-lite';
    enabled = true;
    formModels = [];
    formError = '';
    showKey = false;
  }

  function resetForm() {
    blankForm();
    showForm = false;
  }

  function applyKindDefaults(next: ProviderKind) {
    formModels = [];
    formError = '';
    if (next === 'gemini') {
      if (!editingId) name = 'Gemini';
      baseUrl = 'https://generativelanguage.googleapis.com/v1beta';
      model = 'gemini-3.5-flash-lite';
    } else if (next === 'open_router') {
      if (!editingId) name = 'OpenRouter';
      baseUrl = 'https://openrouter.ai/api/v1';
      model = 'auto';
    } else if (next === 'groq') {
      if (!editingId) name = 'Groq';
      baseUrl = 'https://api.groq.com/openai/v1';
      model = 'llama-3.3-70b-versatile';
    } else if (next === 'custom_open_ai') {
      if (!editingId) name = 'Custom API';
      baseUrl = 'http://localhost:11434/v1';
      model = 'llama3.1';
    }
  }

  function startAdd(preset: ProviderKind = 'gemini') {
    blankForm();
    showForm = true;
    kind = preset;
    applyKindDefaults(preset);
  }

  function startEdit(p: ApiProfile) {
    showForm = true;
    editingId = p.id;
    name = p.name;
    kind = p.kind;
    apiKey = p.api_key;
    baseUrl = p.base_url;
    model = p.model;
    enabled = p.enabled;
    formModels = [];
    formError = '';
    showKey = false;
  }

  async function handleSave(e: Event) {
    e.preventDefault();
    formError = '';
    if (!name.trim()) {
      formError = dict.settings.name;
      return;
    }
    if (!model.trim()) {
      formError = dict.settings.model;
      return;
    }
    if (kind !== 'custom_open_ai' && !apiKey.trim()) {
      formError = dict.settings.keyMissing;
      return;
    }
    const wasNew = !editingId;
    const ok = await bridgeState.saveApiProfile({
      id: editingId,
      name: name.trim(),
      kind,
      apiKey: apiKey.trim(),
      baseUrl: baseUrl.trim(),
      model: model.trim(),
      enabled,
    });
    if (!ok) {
      formError = bridgeState.lastError ?? dict.chat.error;
      return;
    }
    if (wasNew) bridgeState.notify(dict.settings.providerAdded);
    resetForm();
  }

  async function detectFormModels() {
    detecting = true;
    formError = '';
    formModels = [];
    formModels = await bridgeState.listProviderModels({
      kind,
      baseUrl: baseUrl.trim(),
      apiKey: apiKey.trim(),
      model: model.trim(),
    });
    if (formModels.length === 0) {
      formError = bridgeState.lastError ?? dict.settings.noModels;
    } else if (!model.trim()) {
      model = formModels[0];
    }
    detecting = false;
  }

  async function testProfile(p: ApiProfile) {
    testingId = p.id;
    const result = await bridgeState.testApiProfile(p.id);
    if (result) testResults = { ...testResults, [p.id]: result };
    testingId = null;
  }

  function providerStatus(p: ApiProfile): 'ok' | 'warn' | 'bad' | 'idle' | 'unverified' {
    const tested = testResults[p.id];
    if (tested) return tested.ok ? 'ok' : 'bad';
    if (!p.enabled) return 'idle';
    if (p.cooldown_until && p.cooldown_until * 1000 > Date.now()) return 'warn';
    if (!p.api_key && p.kind !== 'custom_open_ai') return 'idle';
    if (p.last_error) return 'bad';
    if (p.requests_count > 0) return 'ok';
    return 'unverified';
  }

  function statusText(p: ApiProfile): string {
    const status = providerStatus(p);
    if (status === 'warn') return dict.settings.cooldown;
    if (status === 'bad') return dict.settings.lastError;
    if (status === 'idle') return dict.settings.keyMissing;
    if (status === 'unverified') return dict.settings.notTested;
    return dict.web.limitOk;
  }

  function cooldownText(until: number | null): string {
    if (!until) return '';
    const date = new Date(until * 1000);
    if (date.getTime() <= Date.now()) return '';
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }

  function removeProfile(p: ApiProfile) {
    if (!confirm(`${dict.settings.delete} «${p.name}»?`)) return;
    void bridgeState.removeApiProfile(p.id);
    if (expandedModels === p.id) expandedModels = null;
  }

  async function testAll() {
    testingAll = true;
    for (const p of bridgeState.config.api_profiles) {
      if (!p.enabled || (!p.api_key && p.kind !== 'custom_open_ai')) continue;
      testingId = p.id;
      const result = await bridgeState.testApiProfile(p.id);
      if (result) testResults = { ...testResults, [p.id]: result };
    }
    testingId = null;
    testingAll = false;
  }

  async function saveBudget() {
    savingBudget = true;
    await bridgeState.updateBridgeConfig({
      max_context_tokens_budget: budget,
      chat_context_budget: chatBudget,
    });
    savingBudget = false;
    bridgeState.notify(dict.settings.contextBudget);
  }

  async function copyWallet(key: string, address: string) {
    await navigator.clipboard.writeText(address);
    copiedWallet = key;
    setTimeout(() => {
      if (copiedWallet === key) copiedWallet = null;
    }, 1800);
  }

  function onBackdrop(e: MouseEvent) {
    if (e.target === e.currentTarget) onClose();
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if (e.key === 'Escape' && open) onClose();
  }}
/>

{#if open}
  <div class="settings-backdrop" role="presentation" onclick={onBackdrop}>
    <div class="settings-window" role="dialog" aria-modal="true" aria-label={dict.app.settings} tabindex="-1" use:focusTrap>
      <aside class="settings-nav">
        <div class="nav-head">
          <div class="nav-mark">LK</div>
          <div>
            <strong>LimitKill</strong>
            <small>{dict.app.settings}</small>
          </div>
        </div>

        <nav>
          {#each tabs as tab (tab.id)}
            {@const Icon = tab.icon}
            <button
              type="button"
              class="nav-btn"
              class:active={activeTab === tab.id}
              onclick={() => (activeTab = tab.id)}
            >
              <Icon size={16} />
              <span>{tab.label}</span>
            </button>
          {/each}
        </nav>

        <div class="nav-foot">
          <span>v{APP_VERSION}</span>
          <span>MIT</span>
        </div>
      </aside>

      <section class="settings-main">
        <header class="settings-head">
          <div class="head-text">
            <h2>{activeTabTitle}</h2>
            <p>{activeTabSubtitle}</p>
          </div>
          <div class="head-actions">
            {#if activeTab === 'providers'}
              <button type="button" class="ghost-btn" onclick={testAll} disabled={testingAll}>
                {#if testingAll}<Loader2 size={14} class="spin" />{:else}<FlaskConical size={14} />{/if}
                {testingAll ? dict.settings.testingAll : dict.settings.testAll}
              </button>
              <button type="button" class="primary-btn" onclick={() => startAdd('gemini')}>
                <Plus size={15} /> {dict.settings.addProvider}
              </button>
            {/if}
            <button type="button" class="close-btn" onclick={onClose} aria-label={dict.common.close}>
              <X size={18} />
            </button>
          </div>
        </header>

        {#if bridgeState.lastError}
          <div class="error-strip">
            <AlertTriangle size={14} />
            <span>{bridgeState.lastError}</span>
            <button type="button" onclick={() => (bridgeState.lastError = null)}>×</button>
          </div>
        {/if}

        <div class="settings-content">
          {#if activeTab === 'providers'}
            {#if showForm}
              <form class="provider-form" onsubmit={handleSave}>
                <div class="form-head">
                  <h3>{editingId ? dict.settings.editProvider : dict.settings.addProvider}</h3>
                  <button type="button" class="link-btn" onclick={resetForm}>{dict.settings.cancel}</button>
                </div>

                <div class="preset-row">
                  <button type="button" class="preset" class:active={kind === 'gemini'} onclick={() => { kind = 'gemini'; applyKindDefaults('gemini'); }}>
                    {dict.settings.presetGemini}
                  </button>
                  <button type="button" class="preset" class:active={kind === 'open_router'} onclick={() => { kind = 'open_router'; applyKindDefaults('open_router'); }}>
                    {dict.settings.presetOpenRouter}
                  </button>
                  <button type="button" class="preset" class:active={kind === 'groq'} onclick={() => { kind = 'groq'; applyKindDefaults('groq'); }}>
                    {dict.settings.presetGroq}
                  </button>
                  <button type="button" class="preset" class:active={kind === 'custom_open_ai'} onclick={() => { kind = 'custom_open_ai'; applyKindDefaults('custom_open_ai'); }}>
                    {dict.settings.presetCustom}
                  </button>
                  <button type="button" class="preset" onclick={() => { kind = 'custom_open_ai'; name = 'Qwen (DashScope)'; baseUrl = 'https://dashscope.aliyuncs.com/compatible-mode/v1'; model = 'qwen-plus'; }}>
                    {dict.settings.presetQwen}
                  </button>
                  <button type="button" class="preset" onclick={() => { kind = 'custom_open_ai'; name = 'Kimi (Moonshot)'; baseUrl = 'https://api.moonshot.cn/v1'; model = 'moonshot-v1-8k'; }}>
                    {dict.settings.presetKimi}
                  </button>
                  <button type="button" class="preset" onclick={() => { kind = 'custom_open_ai'; name = 'OpenAI'; baseUrl = 'https://api.openai.com/v1'; model = 'gpt-4o-mini'; }}>
                    {dict.settings.presetOpenAI}
                  </button>
                </div>

                <div class="form-grid">
                  <label class="field">
                    <span>{dict.settings.name}</span>
                    <input type="text" bind:value={name} placeholder="My provider" required />
                  </label>
                  <label class="field">
                    <span>{dict.settings.kind}</span>
                    <select
                      bind:value={kind}
                      onchange={(e) => {
                        kind = (e.currentTarget as HTMLSelectElement).value as ProviderKind;
                        applyKindDefaults(kind);
                      }}
                    >
                      <option value="gemini">{dict.settings.presetGemini}</option>
                      <option value="open_router">{dict.settings.presetOpenRouter}</option>
                      <option value="groq">{dict.settings.presetGroq}</option>
                      <option value="custom_open_ai">{dict.settings.presetCustom}</option>
                    </select>
                  </label>
                  <label class="field wide">
                    <span>{kind === 'custom_open_ai' ? dict.settings.apiKeyOptional : dict.settings.apiKey}</span>
                    <div class="key-row">
                      <input
                        type={showKey ? 'text' : 'password'}
                        bind:value={apiKey}
                        placeholder={kind === 'open_router' ? 'sk-or-v1-...' : kind === 'gemini' ? 'AIza...' : 'optional'}
                        autocomplete="off"
                      />
                      <button
                        type="button"
                        class="ghost-btn key-toggle"
                        onclick={() => (showKey = !showKey)}
                        title={showKey ? dict.settings.hideKey : dict.settings.showKey}
                        aria-label={showKey ? dict.settings.hideKey : dict.settings.showKey}
                      >
                        {#if showKey}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
                      </button>
                    </div>
                  </label>
                  <label class="field wide">
                    <span>{dict.settings.baseUrl}</span>
                    <input type="text" bind:value={baseUrl} required />
                  </label>
                  <label class="field wide">
                    <span>{dict.settings.model}</span>
                    <div class="model-input-row">
                      <input type="text" bind:value={model} list="form-model-options" required placeholder="auto" />
                      <button type="button" class="ghost-btn" onclick={detectFormModels} disabled={detecting}>
                        {#if detecting}<Loader2 size={14} class="spin" />{:else}<RefreshCw size={14} />{/if}
                        {dict.settings.detectModels}
                      </button>
                    </div>
                    <datalist id="form-model-options">
                      {#each formModels as m (m)}
                        <option value={m}></option>
                      {/each}
                    </datalist>
                    {#if formModels.length}
                      <select
                        class="detected-select"
                        value={formModels.includes(model) ? model : ''}
                        onchange={(e) => {
                          const value = (e.currentTarget as HTMLSelectElement).value;
                          if (value) model = value;
                        }}
                      >
                        <option value="">{dict.settings.modelsFound}: {formModels.length} — {dict.chat.chooseModel}</option>
                        {#each formModels as m (m)}
                          <option value={m}>{m}</option>
                        {/each}
                      </select>
                    {/if}
                  </label>
                </div>

                <label class="check-row">
                  <input type="checkbox" bind:checked={enabled} />
                  <span>{dict.settings.enabled}</span>
                </label>

                <p class="kind-tip">{kindTip}</p>
                {#if formError}<p class="form-error">{formError}</p>{/if}

                <div class="form-actions">
                  <button type="submit" class="primary-btn">
                    <Check size={15} /> {dict.settings.save}
                  </button>
                  <button type="button" class="ghost-btn" onclick={resetForm}>{dict.settings.cancel}</button>
                </div>
              </form>
            {/if}

            <div class="provider-list">
              {#each bridgeState.config.api_profiles as p (p.id)}
                {@const status = providerStatus(p)}
                {@const cool = cooldownText(p.cooldown_until)}
                {@const test = testResults[p.id]}
                <article class="provider-card">
                  <div class="provider-head">
                    <div class="provider-title">
                      <span class="provider-mark">{markOf(p.name)}</span>
                      <div class="provider-name">
                        <strong>{p.name}</strong>
                        <small>
                          {kindLabel(p.kind)}
                          {#if p.api_key}· {dict.settings.keyPresent}{/if}
                        </small>
                      </div>
                    </div>
                    <span class="status-chip {status}">
                      {#if status === 'ok'}<span class="dot"></span>{/if}
                      {statusText(p)}
                    </span>
                  </div>

                  <div class="model-line">
                    <span class="line-label">{dict.settings.model}</span>
                    <code class="model-code">{p.model || 'auto'}</code>
                    <button
                      type="button"
                      class="text-btn"
                      onclick={() => (expandedModels = expandedModels === p.id ? null : p.id)}
                    >
                      {expandedModels === p.id ? dict.common.close : dict.chat.changeModel}
                      <ChevronDown size={12} />
                    </button>
                  </div>

                  <div class="meta-line">
                    <span>{dict.settings.requests}: {p.requests_count}</span>
                    {#if cool}<span class="warn-text">{dict.settings.cooldown}: {cool}</span>{/if}
                    {#if p.last_error}
                      <span class="err-text" title={p.last_error}>{p.last_error}</span>
                    {:else}
                      <span class="muted-text">{dict.settings.noError}</span>
                    {/if}
                  </div>

                  {#if test}
                    <div class="test-line" class:ok={test.ok}>
                      <FlaskConical size={13} />
                      <span>
                        {test.ok ? dict.settings.testOk : dict.settings.testFailed}
                        {#if test.model}· {test.model}{/if}
                        · {test.elapsed_ms} ms
                        {#if test.reply}· {test.reply}{/if}
                        {#if test.error}· {test.error.slice(0, 180)}{/if}
                      </span>
                    </div>
                  {/if}

                  <div class="card-actions">
                    <button type="button" class="ghost-btn" onclick={() => testProfile(p)} disabled={testingId === p.id}>
                      {#if testingId === p.id}<Loader2 size={14} class="spin" />{:else}<FlaskConical size={14} />{/if}
                      {testingId === p.id ? dict.settings.testing : dict.settings.test}
                    </button>
                    <button type="button" class="ghost-btn" onclick={() => startEdit(p)}>
                      <Pencil size={14} /> {dict.settings.edit}
                    </button>
                    <button type="button" class="ghost-btn danger" onclick={() => removeProfile(p)}>
                      <Trash2 size={14} /> {dict.settings.delete}
                    </button>
                  </div>

                  {#if expandedModels === p.id}
                    <div class="models-panel">
                      <ModelList profile={p} onPicked={() => (expandedModels = null)} />
                    </div>
                  {/if}
                </article>
              {/each}

              {#if bridgeState.config.api_profiles.length === 0}
                <div class="empty-card">
                  <p>{dict.chat.noProfiles}</p>
                  <button type="button" class="primary-btn" onclick={() => startAdd('gemini')}>
                    <Plus size={15} /> {dict.settings.addProvider}
                  </button>
                </div>
              {/if}
            </div>

            <section class="local-models">
              <div class="local-head">
                <div>
                  <h3><HardDrive size={15} /> {dict.settings.localTitle}</h3>
                  <p>{dict.settings.localHint}</p>
                  <div class="local-links">
                    <button type="button" onclick={() => openUrl('https://ollama.com/download')}>Ollama</button>
                    <button type="button" onclick={() => openUrl('https://lmstudio.ai/')}>LM Studio</button>
                  </div>
                </div>
                <button type="button" class="ghost-btn" onclick={discoverLocal} disabled={discoveringLocal}>
                  {#if discoveringLocal}<Loader2 size={14} class="spin" />{:else}<RefreshCw size={14} />{/if}
                  {discoveringLocal ? dict.settings.localScanning : dict.settings.localScan}
                </button>
              </div>
              {#if localServers.length > 0}
                <div class="local-list">
                  {#each localServers as server (server.base_url)}
                    <div class="local-server">
                      <div class="local-server-name">
                        <strong>{server.name}</strong>
                        <small>{server.base_url}</small>
                      </div>
                      <div class="local-model-buttons">
                        {#each server.models as model (model)}
                          <button type="button" class="local-model" onclick={() => addLocalModel(server, model)}>
                            <Plus size={12} /> {model}
                          </button>
                        {/each}
                      </div>
                    </div>
                  {/each}
                </div>
              {:else if localScanned && !discoveringLocal}
                <p class="local-empty">{dict.settings.localNone}</p>
              {/if}
            </section>
          {:else if activeTab === 'web'}
            <WebBridge />
          {:else if activeTab === 'general'}
            <div class="general-grid">
              <section class="setting-card">
                <h3>{dict.settings.contextBudget}</h3>
                <p>{dict.settings.contextBudgetHint}</p>
                <div class="budget-row">
                  <input type="number" min="2048" max="1000000" step="1000" bind:value={budget} />
                  <span>{dict.rag.tokens}</span>
                </div>
                <input type="range" min="4096" max="200000" step="1000" bind:value={budget} />
                <div class="budget-stats">
                  <span>{dict.rag.fullCount}: <b>{bridgeState.contextPreview.full_files_count}</b></span>
                  <span>{dict.rag.skelCount}: <b>{bridgeState.contextPreview.skeleton_files_count}</b></span>
                  <span><b>{bridgeState.contextPreview.total_tokens}</b> {dict.rag.tokens}</span>
                </div>
                {#if bridgeState.contextPreview.truncated}
                  <p class="warn-text small">{dict.rag.truncated}</p>
                {/if}
                <button type="button" class="primary-btn" onclick={saveBudget} disabled={savingBudget}>
                  {#if savingBudget}<Loader2 size={14} class="spin" />{:else}<Check size={14} />{/if}
                  {dict.settings.save}
                </button>
              </section>

              <section class="setting-card">
                <h3>{dict.settings.chatContextBudget}</h3>
                <p>{dict.settings.chatContextBudgetHint}</p>
                <div class="budget-row">
                  <input type="number" min="8000" max="1000000" step="1000" bind:value={chatBudget} />
                  <span>{dict.rag.tokens}</span>
                </div>
                <input type="range" min="8000" max="1000000" step="1000" bind:value={chatBudget} />
                <button type="button" class="primary-btn" onclick={saveBudget} disabled={savingBudget}>
                  {#if savingBudget}<Loader2 size={14} class="spin" />{:else}<Check size={14} />{/if}
                  {dict.settings.save}
                </button>
              </section>

              <section class="setting-card">
                <h3>{dict.app.autoRotate}</h3>
                <p>{dict.settings.autoRotateHint}</p>
                <label class="check-row">
                  <input
                    type="checkbox"
                    checked={bridgeState.config.auto_rotate_on_limit}
                    onchange={(e) =>
                      bridgeState.updateBridgeConfig({
                        auto_rotate_on_limit: (e.currentTarget as HTMLInputElement).checked,
                      })}
                  />
                  <span>{dict.settings.enabled}</span>
                </label>
              </section>

              <section class="setting-card about-card">
                <h3>{dict.settings.about}</h3>
                <div class="about-row"><span>LimitKill</span><b>v{APP_VERSION}</b></div>
                <div class="about-row"><span>{dict.settings.version}</span><b>{APP_VERSION}</b></div>
                <div class="about-row"><span>License</span><b>MIT</b></div>
              </section>
            </div>
          {:else}
            <div class="donate-box">
              <div class="wallet-list">
                {#each wallets as w (w.key)}
                  <div class="wallet-card">
                    <div class="wallet-head">
                      <span class="wallet-label">{w.label}</span>
                      <button type="button" class="ghost-btn" onclick={() => copyWallet(w.key, w.address)}>
                        {#if copiedWallet === w.key}<Check size={14} />{:else}<Copy size={14} />{/if}
                        {copiedWallet === w.key ? dict.settings.copied : dict.settings.copy}
                      </button>
                    </div>
                    <code>{w.address}</code>
                    {#if w.key === 'usdt'}<small class="warn-text">{w.note}</small>{/if}
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      </section>
    </div>
  </div>
{/if}

<style>
  .settings-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: rgba(47, 43, 35, 0.38);
    backdrop-filter: blur(3px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
  }

  .settings-window {
    width: min(1080px, 100%);
    height: min(88vh, 820px);
    display: flex;
    background: var(--bg-elev);
    border: 1px solid var(--line);
    border-radius: 18px;
    box-shadow: var(--shadow-md);
    overflow: hidden;
  }

  .settings-nav {
    width: 218px;
    flex-shrink: 0;
    background: var(--bg-soft);
    border-right: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    padding: 14px 10px;
  }
  .nav-head {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 4px 8px 16px;
  }
  .nav-mark {
    width: 30px;
    height: 30px;
    border-radius: 9px;
    background: var(--accent);
    color: #fbfdf9;
    font-weight: 800;
    font-size: 11px;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .nav-head strong {
    display: block;
    font-size: 13px;
  }
  .nav-head small {
    display: block;
    font-size: 10.5px;
    color: var(--muted);
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .nav-btn {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 9px 10px;
    border: 1px solid transparent;
    border-radius: 9px;
    background: transparent;
    color: var(--ink-2);
    font-size: 12.5px;
    font-weight: 500;
    cursor: pointer;
    text-align: left;
  }
  .nav-btn:hover {
    background: var(--panel);
    color: var(--ink);
  }
  .nav-btn.active {
    background: var(--accent-soft);
    border-color: rgba(79, 122, 90, 0.18);
    color: var(--accent-strong);
    font-weight: 600;
  }

  .nav-foot {
    margin-top: auto;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 10px 2px;
    font-size: 10px;
    color: var(--muted);
  }

  .settings-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .settings-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 14px;
    padding: 18px 20px 14px;
    border-bottom: 1px solid var(--line);
  }
  .head-text h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }
  .head-text p {
    margin: 4px 0 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--muted);
    max-width: 560px;
  }
  .head-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .close-btn {
    width: 32px;
    height: 32px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--line);
    border-radius: 9px;
    background: var(--bg-soft);
    color: var(--muted);
    cursor: pointer;
  }
  .close-btn:hover {
    color: var(--ink);
    border-color: var(--line-strong);
  }

  .error-strip {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 10px 20px 0;
    padding: 8px 10px;
    border-radius: 9px;
    background: var(--danger-soft);
    border: 1px solid rgba(173, 74, 63, 0.25);
    color: var(--danger);
    font-size: 11.5px;
    line-height: 1.45;
  }
  .error-strip span {
    flex: 1;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 100px;
    overflow-y: auto;
  }
  .error-strip button {
    border: none;
    background: transparent;
    color: inherit;
    cursor: pointer;
    font-size: 15px;
  }

  .settings-content {
    flex: 1;
    overflow-y: auto;
    padding: 18px 20px 24px;
  }

  .primary-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 8px 13px;
    border: none;
    border-radius: 9px;
    background: var(--accent);
    color: #fbfdf9;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
    box-shadow: 0 3px 10px rgba(79, 122, 90, 0.22);
  }
  .primary-btn:hover {
    background: var(--accent-strong);
  }
  .primary-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .ghost-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 6px 10px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg-elev);
    color: var(--ink-2);
    font-size: 11.5px;
    cursor: pointer;
  }
  .ghost-btn:hover {
    border-color: var(--accent);
    color: var(--accent-strong);
    background: var(--accent-tint);
  }
  .ghost-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .ghost-btn.danger:hover {
    border-color: var(--danger);
    color: var(--danger);
    background: var(--danger-soft);
  }
  .text-btn {
    border: none;
    background: transparent;
    color: var(--accent-strong);
    font-size: 11.5px;
    cursor: pointer;
    padding: 2px 4px;
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .text-btn:hover {
    text-decoration: underline;
  }
  .link-btn {
    border: none;
    background: transparent;
    color: var(--muted);
    font-size: 11.5px;
    cursor: pointer;
  }

  .local-models {
    margin-top: 16px;
    padding: 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--bg-elev);
  }
  .local-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }
  .local-head h3 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: 13px;
    color: var(--ink);
  }
  .local-head p {
    margin: 4px 0 0;
    max-width: 620px;
    font-size: 10.5px;
    line-height: 1.45;
    color: var(--muted);
  }
  .local-links {
    display: flex;
    gap: 8px;
    margin-top: 5px;
  }
  .local-links button {
    border: none;
    background: transparent;
    padding: 0;
    color: var(--accent-strong);
    font-size: 10px;
    font-weight: 600;
    cursor: pointer;
    text-decoration: underline;
  }
  .local-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 10px;
  }
  .local-server {
    padding: 9px 10px;
    border-radius: 8px;
    background: var(--bg-soft);
    border: 1px solid var(--line);
  }
  .local-server-name {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }
  .local-server-name strong {
    font-size: 11.5px;
    color: var(--ink);
  }
  .local-server-name small {
    font-size: 9.5px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .local-model-buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    margin-top: 6px;
  }
  .local-model {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 7px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg-elev);
    color: var(--ink-2);
    font-size: 10px;
    cursor: pointer;
  }
  .local-model:hover {
    border-color: var(--accent);
    color: var(--accent-strong);
  }
  .local-empty {
    margin: 9px 0 0;
    font-size: 10.5px;
    color: var(--muted);
  }

  .provider-form {
    background: var(--bg-soft);    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 16px;
    margin-bottom: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .form-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .form-head h3 {
    margin: 0;
    font-size: 14px;
  }
  .preset-row {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .preset {
    padding: 6px 11px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--bg-elev);
    color: var(--ink-2);
    font-size: 11.5px;
    cursor: pointer;
  }
  .preset:hover {
    border-color: var(--accent);
    color: var(--accent-strong);
  }
  .preset.active {
    background: var(--accent-soft);
    border-color: rgba(79, 122, 90, 0.3);
    color: var(--accent-strong);
    font-weight: 600;
  }

  .form-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 5px;
    font-size: 11.5px;
    color: var(--ink-2);
  }
  .field.wide {
    grid-column: 1 / -1;
  }
  .field input,
  .field select {
    width: 100%;
    padding: 8px 10px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg-elev);
    color: var(--ink);
    font-size: 12.5px;
    outline: none;
  }
  .field input:focus,
  .field select:focus {
    border-color: var(--accent);
  }
  .model-input-row {
    display: flex;
    gap: 6px;
  }
  .model-input-row input {
    flex: 1;
    min-width: 0;
  }
  .key-row {
    display: flex;
    gap: 6px;
  }
  .key-row input {
    flex: 1;
    min-width: 0;
  }
  .key-toggle {
    flex-shrink: 0;
  }
  .detected-select {
    width: 100%;
    padding: 7px 9px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg-elev);
    color: var(--ink);
    font-size: 12px;
  }
  .check-row {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 12px;
    color: var(--ink-2);
  }
  .check-row input {
    accent-color: var(--accent);
  }
  .kind-tip {
    margin: 0;
    font-size: 11px;
    line-height: 1.45;
    color: var(--muted);
  }
  .form-error {
    margin: 0;
    font-size: 11.5px;
    color: var(--danger);
  }
  .form-actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .ok-text {
    font-size: 10.5px;
    color: var(--ok);
  }

  .provider-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .provider-card {
    background: var(--bg-elev);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 9px;
    box-shadow: var(--shadow-sm);
  }
  .provider-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
  }
  .provider-title {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .provider-mark {
    width: 34px;
    height: 34px;
    flex-shrink: 0;
    border-radius: 10px;
    background: var(--accent-soft);
    color: var(--accent-strong);
    font-size: 11px;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .provider-name {
    min-width: 0;
  }
  .provider-name strong {
    display: block;
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .provider-name small {
    display: block;
    font-size: 10.5px;
    color: var(--muted);
    margin-top: 1px;
  }
  .status-chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 10px;
    font-weight: 600;
    padding: 3px 8px;
    border-radius: 999px;
    white-space: nowrap;
    background: var(--panel);
    color: var(--muted);
  }
  .status-chip.ok {
    background: var(--ok-soft);
    color: var(--ok);
  }
  .status-chip.warn {
    background: var(--warm-soft);
    color: var(--warm);
  }
  .status-chip.bad {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .status-chip.unverified {
    background: var(--panel);
    color: var(--muted);
  }
  .status-chip .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
  }

  .model-line {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding: 7px 9px;
    background: var(--bg-soft);
    border: 1px solid var(--line);
    border-radius: 8px;
  }
  .line-label {
    font-size: 10.5px;
    color: var(--muted);
    flex-shrink: 0;
  }
  .model-code {
    flex: 1;
    min-width: 0;
    font-size: 11px;
    color: var(--ink);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta-line {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    font-size: 10.5px;
    color: var(--muted);
  }
  .meta-line .err-text {
    color: var(--danger);
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .warn-text {
    color: var(--warm);
  }
  .muted-text {
    color: var(--muted);
  }
  .small {
    font-size: 10.5px;
  }
  .test-line {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    font-size: 10.5px;
    color: var(--warm);
    background: var(--warm-soft);
    border-radius: 8px;
    padding: 7px 9px;
    line-height: 1.4;
  }
  .test-line.ok {
    color: var(--ok);
    background: var(--ok-soft);
  }
  .card-actions {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .models-panel {
    border-top: 1px dashed var(--line);
    padding-top: 10px;
  }

  .empty-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 36px 20px;
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: 12.5px;
  }
  .empty-card p {
    margin: 0;
  }

  .general-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .setting-card {
    background: var(--bg-soft);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .setting-card h3 {
    margin: 0;
    font-size: 14px;
  }
  .setting-card p {
    margin: 0;
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--muted);
  }
  .budget-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .budget-row input {
    flex: 1;
    padding: 8px 10px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg-elev);
    color: var(--ink);
    font-size: 13px;
  }
  .budget-row span {
    font-size: 11px;
    color: var(--muted);
  }
  .setting-card input[type='range'] {
    accent-color: var(--accent);
    width: 100%;
  }
  .budget-stats {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    font-size: 10.5px;
    color: var(--muted);
  }
  .budget-stats b {
    color: var(--accent-strong);
  }
  .about-card .about-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 12px;
    color: var(--ink-2);
    padding: 4px 0;
    border-bottom: 1px dashed var(--line);
  }
  .about-card .about-row:last-child {
    border-bottom: none;
  }
  .about-card b {
    color: var(--ink);
  }

  .donate-box {
    max-width: 680px;
  }
  .wallet-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .wallet-card {
    background: var(--bg-soft);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .wallet-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .wallet-label {
    font-size: 12.5px;
    font-weight: 600;
  }
  .wallet-card code {
    font-size: 11.5px;
    color: var(--ink-2);
    word-break: break-all;
    background: var(--bg-elev);
    border: 1px solid var(--line);
    border-radius: 7px;
    padding: 7px 9px;
  }
  .wallet-card small {
    font-size: 10.5px;
  }

  @media (max-width: 780px) {
    .settings-window {
      flex-direction: column;
      height: 92vh;
    }
    .settings-nav {
      width: 100%;
      border-right: none;
      border-bottom: 1px solid var(--line);
      flex-direction: row;
      align-items: center;
      padding: 8px;
    }
    .nav-head,
    .nav-foot {
      display: none;
    }
    nav {
      flex-direction: row;
      overflow-x: auto;
      gap: 2px;
    }
    .nav-btn {
      white-space: nowrap;
      padding: 7px 10px;
    }
    .form-grid,
    .general-grid {
      grid-template-columns: 1fr;
    }
    .settings-backdrop {
      padding: 10px;
    }
  }
</style>
