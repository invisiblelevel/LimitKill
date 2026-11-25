<script lang="ts">
  import { getDictionary } from '../i18n/index.js';
  import {
    bridgeState,
    type WebCheckResult,
    type WebKind,
    type WebProfile,
  } from '../state/appState.svelte.js';
  import {
    Plus,
    ExternalLink,
    LogIn,
    RotateCcw,
    Trash2,
    Star,
    Check,
    Loader2,
    AlertTriangle,
    Copy,
    MoreHorizontal,
  } from '@lucide/svelte';

  const dict = $derived(getDictionary(bridgeState.config.locale));

  let showAdd = $state(false);
  let newName = $state('');
  let newKind = $state<WebKind>('deepseek');
  let newUrl = $state('https://chat.deepseek.com');
  let activeFilter = $state<'all' | WebKind>('all');
  let busyId = $state<string | null>(null);
  let status = $state<Record<string, WebCheckResult>>({});
  let accountMenu = $state<{ id: string; x: number; y: number } | null>(null);

  const menuProfile = $derived(
    accountMenu
      ? bridgeState.config.web_profiles.find((p) => p.id === accountMenu?.id) ?? null
      : null
  );

  function openAccountMenu(e: MouseEvent, profile: WebProfile) {
    e.stopPropagation();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    accountMenu = {
      id: profile.id,
      x: Math.max(8, Math.min(rect.right - 200, window.innerWidth - 210)),
      y: Math.min(rect.bottom + 4, window.innerHeight - 230),
    };
  }

  const PRESETS: { kind: WebKind; label: string; url: string }[] = [
    { kind: 'deepseek', label: 'DeepSeek', url: 'https://chat.deepseek.com' },
    { kind: 'gemini', label: 'Google Gemini', url: 'https://gemini.google.com/app' },
    { kind: 'qwen', label: 'Qwen', url: 'https://chat.qwen.ai/' },
    { kind: 'claude', label: 'Claude', url: 'https://claude.ai/new' },
  ];

  const filterTabs = $derived([
    { kind: 'all' as const, label: dict.web.all, count: bridgeState.config.web_profiles.length },
    ...PRESETS.map((p) => ({
      kind: p.kind,
      label: p.label,
      count: bridgeState.config.web_profiles.filter((w) => (w.kind ?? 'deepseek') === p.kind)
        .length,
    })),
  ]);

  const filteredProfiles = $derived(
    activeFilter === 'all'
      ? bridgeState.config.web_profiles
      : bridgeState.config.web_profiles.filter((p) => (p.kind ?? 'deepseek') === activeFilter)
  );

  function kindLabel(kind: WebKind | undefined): string {
    return PRESETS.find((p) => p.kind === (kind ?? 'deepseek'))?.label ?? 'DeepSeek';
  }

  function kindMark(kind: WebKind | undefined): string {
    if (kind === 'gemini') return 'G';
    if (kind === 'qwen') return 'Q';
    if (kind === 'claude') return 'C';
    return 'DS';
  }

  function openAdd(kind: WebKind = 'deepseek') {
    const preset = PRESETS.find((p) => p.kind === kind) ?? PRESETS[0];
    const count = bridgeState.config.web_profiles.filter((p) => (p.kind ?? 'deepseek') === kind).length;
    newKind = kind;
    newName = `${preset.label} #${count + 1}`;
    newUrl = preset.url;
    showAdd = true;
  }

  function applyKind(kind: WebKind) {
    const preset = PRESETS.find((p) => p.kind === kind);
    if (!preset) return;
    newKind = kind;
    newUrl = preset.url;
    const count = bridgeState.config.web_profiles.filter((p) => (p.kind ?? 'deepseek') === kind).length;
    newName = `${preset.label} #${count + 1}`;
  }

  function formatUntil(until: number | null): string {
    if (!until) return '';
    return new Date(until * 1000).toLocaleString();
  }

  function isLimited(p: WebProfile): boolean {
    if (p.limit_until) return p.limit_until * 1000 > Date.now();
    return p.limit_hit;
  }

  function statusLabel(value: string): string {
    if (value === 'ready') return dict.web.ready;
    if (value === 'login') return dict.web.login;
    if (value === 'timeout') return dict.web.timeout;
    return dict.web.lastError;
  }

  async function addProfile(e: Event) {
    e.preventDefault();
    if (!newName.trim()) return;
    const ok = await bridgeState.saveWebProfile({
      name: newName.trim(),
      kind: newKind,
      url: newUrl.trim() || 'https://chat.deepseek.com',
    });
    if (ok) {
      newName = '';
      newUrl = 'https://chat.deepseek.com';
      showAdd = false;
    }
  }

  async function useProfile(p: WebProfile) {
    accountMenu = null;
    await bridgeState.selectWebProfile(p.id);
  }

  async function openProfile(p: WebProfile) {
    busyId = p.id;
    await bridgeState.openWebProfile(p.id, false);
    busyId = null;
  }

  async function checkProfile(p: WebProfile) {
    busyId = p.id;
    const result = await bridgeState.checkWebProfile(p.id);
    status = { ...status, [p.id]: result };
    busyId = null;
  }

  async function toggleLimit(p: WebProfile) {
    accountMenu = null;
    await bridgeState.setWebProfileLimit(p.id, !isLimited(p));
  }

  async function resetProfile(p: WebProfile) {
    accountMenu = null;
    if (!confirm(dict.web.resetConfirm)) return;
    busyId = p.id;
    await bridgeState.resetWebProfile(p.id);
    status = {
      ...status,
      [p.id]: {
        status: 'login',
        detail: dict.web.resetHint,
        url: '',
        checked_at: Math.floor(Date.now() / 1000),
      },
    };
    busyId = null;
  }

  async function deleteProfile(p: WebProfile) {
    accountMenu = null;
    if (!confirm(dict.web.deleteConfirm)) return;
    await bridgeState.removeWebProfile(p.id);
  }

  async function copyDir(dir: string) {
    await navigator.clipboard.writeText(dir);
    bridgeState.notify(dict.common.copied);
  }
</script>

<svelte:window
  onmousedown={(e) => {
    const target = e.target as Element | null;
    if (
      accountMenu &&
      !target?.closest('.account-menu') &&
      !target?.closest('.account-more')
    ) {
      accountMenu = null;
    }
  }}
  onkeydown={(e) => {
    if (e.key === 'Escape') accountMenu = null;
  }}
/>

<section class="web-accounts">
  <div class="web-toolbar">
    <p class="web-tip">{dict.web.tip}</p>
    <button type="button" class="primary-btn" onclick={() => openAdd(activeFilter === 'all' ? 'deepseek' : activeFilter)}>
      <Plus size={15} /> {dict.web.add}
    </button>
  </div>

  {#if showAdd}
    <form class="add-form" onsubmit={addProfile}>
      <h3>{dict.web.addTitle}</h3>
      <div class="form-row">
        <label class="field">
          <span>{dict.web.service}</span>
          <select
            value={newKind}
            onchange={(e) => applyKind((e.currentTarget as HTMLSelectElement).value as WebKind)}
          >
            {#each PRESETS as preset (preset.kind)}
              <option value={preset.kind}>{preset.label}</option>
            {/each}
          </select>
        </label>
        <label class="field">
          <span>{dict.web.name}</span>
          <input type="text" bind:value={newName} placeholder="DeepSeek #5" required />
        </label>
        <label class="field">
          <span>{dict.web.url}</span>
          <input type="text" bind:value={newUrl} placeholder="https://chat.deepseek.com" />
        </label>
        <button type="submit" class="primary-btn" disabled={!newName.trim()}>
          <Plus size={15} /> {dict.web.add}
        </button>
      </div>
      <p class="hint">{dict.web.isolatedHint}</p>
    </form>
  {/if}

  {#if bridgeState.config.web_profiles.length === 0}
    <div class="empty-card">
      <p>{dict.web.noProfiles}</p>
      <button type="button" class="primary-btn" onclick={() => openAdd()}>
        <Plus size={15} /> {dict.web.add}
      </button>
    </div>
  {:else}
    <div class="web-tabs" role="tablist" aria-label={dict.web.title}>
      {#each filterTabs as tab (tab.kind)}
        <button
          type="button"
          class:active={activeFilter === tab.kind}
          onclick={() => (activeFilter = tab.kind)}
        >
          {tab.label}
          <span>{tab.count}</span>
        </button>
      {/each}
    </div>

    <div class="profiles">
      {#if filteredProfiles.length === 0}
        <div class="empty-card">
          <p>{dict.web.noFiltered}</p>
          <button type="button" class="primary-btn" onclick={() => openAdd(activeFilter === 'all' ? 'deepseek' : activeFilter)}>
            <Plus size={15} /> {dict.web.add}
          </button>
        </div>
      {/if}
      {#each filteredProfiles as p (p.id)}
        {@const limited = isLimited(p)}
        {@const active = p.id === bridgeState.config.active_web_profile_id}
        {@const check = status[p.id]}
        <article class="account-card {limited ? 'limited' : ''} {active ? 'active-card' : ''}">
          <div class="card-head">
            <div class="account-title">
              <span class="account-mark kind-{p.kind ?? 'deepseek'}">{kindMark(p.kind)}</span>
              <div class="account-name">
                <strong>
                  {p.name}
                  {#if active}<span class="active-tag"><Star size={10} /> {dict.web.active}</span>{/if}
                </strong>
                <span class="account-meta">
                  <span class="kind-badge">{kindLabel(p.kind)}</span>
                  <button type="button" class="dir-btn" onclick={() => copyDir(p.user_data_dir)} title={dict.common.copy}>
                    <Copy size={10} /> {p.user_data_dir}
                  </button>
                </span>
              </div>
            </div>
            <span
              class="status-chip"
              class:bad={limited || (check && check.status !== 'ready')}
              class:ok={!limited && check?.status === 'ready'}
            >
              {#if limited}
                <AlertTriangle size={11} /> {dict.web.limitHit}
              {:else if check?.status === 'ready'}
                <span class="dot"></span> {dict.web.ready}
              {:else if check?.status === 'login'}
                <AlertTriangle size={11} /> {dict.web.login}
              {:else if check?.status === 'timeout' || check?.status === 'error'}
                <AlertTriangle size={11} /> {dict.web.lastError}
              {:else if p.last_error}
                <AlertTriangle size={11} /> {dict.web.lastError}
              {:else}
                <span class="dot muted"></span> {dict.web.notChecked}
              {/if}
            </span>
          </div>

          {#if limited && p.limit_until}
            <div class="meta warn-text">{dict.web.limitUntil}: {formatUntil(p.limit_until)}</div>
          {/if}
          {#if p.last_error}
            <div class="meta err-text" title={p.last_error}>{p.last_error}</div>
          {/if}
          {#if check}
            <details class="diagnostics">
              <summary class="meta check-text" class:ok={check.status === 'ready'}>
                {statusLabel(check.status)}
                <span class="diag-arrow">›</span>
              </summary>
              <div class="diag-body">
                <div>{check.detail}</div>
                {#if check.url}<div class="diag-url" title={check.url}>{check.url}</div>{/if}
              </div>
            </details>
          {/if}

          <div class="card-actions">
            <button type="button" class="primary-btn small" onclick={() => openProfile(p)} disabled={busyId === p.id}>
              <ExternalLink size={14} /> {dict.web.open}
            </button>
            <button type="button" class="ghost-btn" onclick={() => checkProfile(p)} disabled={busyId === p.id}>
              {#if busyId === p.id}
                <Loader2 size={14} class="spin" /> {dict.web.checking}
              {:else}
                <LogIn size={14} /> {dict.web.check}
              {/if}
            </button>
            <button
              type="button"
              class="ghost-btn icon-only account-more"
              aria-label={dict.chat.manageProviders}
              onclick={(e) => openAccountMenu(e, p)}
            >
              <MoreHorizontal size={15} />
            </button>
          </div>
        </article>
      {/each}
    </div>

    {#if accountMenu && menuProfile}
      {@const target = menuProfile}
      {@const targetLimited = isLimited(target)}
      <div class="account-menu" role="menu" style="left: {accountMenu.x}px; top: {accountMenu.y}px">
        <button type="button" role="menuitem" onclick={() => useProfile(target)} disabled={target.id === bridgeState.config.active_web_profile_id}>
          {#if target.id === bridgeState.config.active_web_profile_id}<Check size={14} />{:else}<Star size={14} />{/if}
          {dict.web.use}
        </button>
        <button type="button" role="menuitem" onclick={() => toggleLimit(target)}>
          {targetLimited ? dict.web.clearLimit : dict.web.markLimit}
        </button>
        <button type="button" role="menuitem" onclick={() => resetProfile(target)} disabled={busyId === target.id}>
          <RotateCcw size={14} /> {dict.web.reset}
        </button>
        <div class="menu-sep"></div>
        <button type="button" role="menuitem" class="danger" onclick={() => deleteProfile(target)}>
          <Trash2 size={14} /> {dict.web.delete}
        </button>
      </div>
    {/if}
  {/if}
</section>

<style>
  .web-accounts {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .web-toolbar {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 14px;
  }
  .web-tip {
    margin: 0;
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--muted);
    max-width: 620px;
    padding: 9px 11px;
    background: var(--accent-tint);
    border: 1px solid rgba(79, 122, 90, 0.16);
    border-radius: 10px;
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
    white-space: nowrap;
  }
  .primary-btn:hover {
    background: var(--accent-strong);
  }
  .primary-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .primary-btn.small {
    padding: 6px 11px;
    font-size: 11.5px;
    box-shadow: none;
  }

  .ghost-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 6px 9px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg-elev);
    color: var(--ink-2);
    font-size: 11px;
    cursor: pointer;
    white-space: nowrap;
  }
  .ghost-btn:hover {
    border-color: var(--accent);
    color: var(--accent-strong);
    background: var(--accent-tint);
  }
  .ghost-btn:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .add-form {
    background: var(--bg-soft);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .add-form h3 {
    margin: 0;
    font-size: 13.5px;
  }
  .form-row {
    display: flex;
    gap: 10px;
    align-items: flex-end;
    flex-wrap: wrap;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 5px;
    font-size: 11.5px;
    color: var(--ink-2);
    flex: 1;
    min-width: 190px;
  }
  .field input,
  .field select {
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
  .hint {
    margin: 0;
    font-size: 10.5px;
    color: var(--muted);
  }

  .profiles {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: calc(100vh - 310px);
    min-height: 140px;
    overflow-y: auto;
    padding-right: 4px;
    scrollbar-width: thin;
  }
  .web-tabs {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    padding: 2px 0 8px;
    border-bottom: 1px solid var(--line);
  }
  .web-tabs button {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 5px 9px;
    border: 1px solid transparent;
    border-radius: 8px;
    background: transparent;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }
  .web-tabs button:hover {
    color: var(--ink-2);
    background: var(--panel);
  }
  .web-tabs button.active {
    color: var(--accent-strong);
    background: var(--accent-soft);
    border-color: rgba(79, 122, 90, 0.18);
  }
  .web-tabs button span {
    font-size: 9px;
    padding: 1px 5px;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.05);
  }
  .account-card {
    background: var(--bg-elev);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    box-shadow: var(--shadow-sm);
  }
  .account-card.active-card {
    border-color: rgba(79, 122, 90, 0.35);
  }
  .account-card.limited {
    background: linear-gradient(0deg, rgba(184, 117, 69, 0.04), rgba(184, 117, 69, 0.04)), var(--bg-elev);
  }

  .card-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
  }
  .account-title {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .account-mark {
    width: 32px;
    height: 32px;
    border-radius: 9px;
    flex-shrink: 0;
    background: var(--warm-soft);
    color: var(--warm);
    font-size: 10px;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .account-mark.kind-deepseek {
    background: var(--accent-soft);
    color: var(--accent-strong);
  }
  .account-mark.kind-gemini {
    background: #e8e3f1;
    color: #5d547a;
  }
  .account-mark.kind-qwen {
    background: #f3e3d2;
    color: #8a5a35;
  }
  .account-mark.kind-kimi {
    background: #e1e8e3;
    color: #496457;
  }
  .account-mark.kind-claude {
    background: #f0e4d8;
    color: #8a5a35;
  }
  .account-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 2px;
    min-width: 0;
  }
  .kind-badge {
    font-size: 9px;
    font-weight: 600;
    color: var(--muted);
    background: var(--panel);
    border-radius: 999px;
    padding: 1px 6px;
    white-space: nowrap;
  }
  .account-name {
    min-width: 0;
  }
  .account-name strong {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .active-tag {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 9px;
    font-weight: 600;
    color: var(--accent-strong);
    background: var(--accent-soft);
    border-radius: 999px;
    padding: 1px 6px;
  }
  .dir-btn {
    margin-top: 2px;
    border: none;
    background: transparent;
    color: var(--muted);
    font-size: 10px;
    cursor: pointer;
    padding: 0;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .dir-btn:hover {
    color: var(--ink-2);
  }

  .status-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 10px;
    font-weight: 600;
    padding: 3px 8px;
    border-radius: 999px;
    background: var(--panel);
    color: var(--muted);
    white-space: nowrap;
    flex-shrink: 0;
  }
  .status-chip.ok {
    background: var(--ok-soft);
    color: var(--ok);
  }
  .status-chip.bad {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .status-chip .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
  }
  .status-chip .dot.muted {
    background: var(--muted);
  }

  .meta {
    font-size: 10.5px;
    line-height: 1.4;
    word-break: break-word;
  }
  .warn-text {
    color: var(--warm);
  }
  .err-text {
    color: var(--danger);
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .check-text {
    color: var(--muted);
  }
  .check-text.ok {
    color: var(--ok);
  }

  .card-actions {
    display: flex;
    gap: 5px;
    flex-wrap: wrap;
    align-items: center;
  }

  .diagnostics summary {
    list-style: none;
  }
  .diagnostics summary::-webkit-details-marker {
    display: none;
  }
  .diagnostics summary {
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .diag-arrow {
    font-size: 13px;
    transition: transform 0.15s ease;
  }
  .diagnostics[open] .diag-arrow {
    transform: rotate(90deg);
  }
  .diag-body {
    margin-top: 4px;
    padding: 7px 9px;
    border-radius: 7px;
    background: var(--bg-soft);
    border: 1px solid var(--line);
    font-size: 10px;
    line-height: 1.45;
    color: var(--muted);
    word-break: break-word;
  }
  .diag-url {
    margin-top: 4px;
    font-family: ui-monospace, monospace;
    font-size: 9.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ghost-btn.icon-only {
    width: 30px;
    padding: 6px 0;
    justify-content: center;
  }
  .account-menu {
    position: fixed;
    z-index: 150;
    min-width: 195px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 5px;
    background: var(--bg-elev);
    border: 1px solid var(--line);
    border-radius: 9px;
    box-shadow: var(--shadow-md);
  }
  .account-menu button {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    padding: 7px 8px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--ink-2);
    font-size: 11.5px;
    text-align: left;
    cursor: pointer;
  }
  .account-menu button:hover {
    background: var(--panel);
    color: var(--ink);
  }
  .account-menu button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .account-menu button.danger {
    color: var(--danger);
  }
  .account-menu button.danger:hover {
    background: var(--danger-soft);
  }
  .account-menu .menu-sep {
    height: 1px;
    margin: 3px 4px;
    background: var(--line);
  }

  .empty-card {
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius);
    padding: 32px 20px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    color: var(--muted);
    font-size: 12.5px;
  }
  .empty-card p {
    margin: 0;
  }

  @media (max-width: 720px) {
    .profiles {
      grid-template-columns: 1fr;
    }
    .web-toolbar {
      flex-direction: column;
    }
  }
</style>
