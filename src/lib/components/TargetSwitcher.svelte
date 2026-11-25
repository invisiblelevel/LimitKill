<script lang="ts">
  import { getDictionary } from '../i18n/index.js';
  import {
    bridgeState,
    type ApiProfile,
    type WebCheckResult,
    type WebProfile,
  } from '../state/appState.svelte.js';
  import ModelList from './ModelList.svelte';
  import { focusTrap } from '../actions/focusTrap.js';
  import {
    X,
    Check,
    ChevronRight,
    ChevronDown,
    Settings2,
    LogIn,
    ExternalLink,
    AlertTriangle,
    Loader2,
  } from '@lucide/svelte';

  interface Props {
    open: boolean;
    onClose: () => void;
    onManage: () => void;
  }

  let { open, onClose, onManage }: Props = $props();
  const dict = $derived(getDictionary(bridgeState.config.locale));

  let checkingId = $state<string | null>(null);
  let status = $state<Record<string, WebCheckResult>>({});
  let showModels = $state(false);

  let wasOpen = false;
  $effect(() => {
    if (open && !wasOpen) showModels = false;
    wasOpen = open;
  });

  function apiCooling(p: ApiProfile): boolean {
    return !!p.cooldown_until && p.cooldown_until * 1000 > Date.now();
  }

  function webLimited(p: WebProfile): boolean {
    if (p.limit_until) return p.limit_until * 1000 > Date.now();
    return p.limit_hit;
  }

  function markOf(name: string): string {
    const clean = name.replace(/[^\p{L}\p{N}]+/gu, ' ').trim();
    const parts = clean.split(/\s+/).filter(Boolean);
    if (parts.length === 0) return 'AI';
    return (parts[0][0] + (parts[1]?.[0] ?? '')).toUpperCase();
  }

  function statusLabel(value: string): string {
    if (value === 'ready') return dict.web.ready;
    if (value === 'login') return dict.web.login;
    if (value === 'timeout') return dict.web.timeout;
    return dict.web.lastError;
  }

  async function chooseApi(p: ApiProfile) {
    await bridgeState.selectApiProfile(p.id);
  }

  async function chooseWeb(p: WebProfile) {
    await bridgeState.selectWebProfile(p.id);
  }

  async function checkWeb(p: WebProfile) {
    checkingId = p.id;
    const res = await bridgeState.checkWebProfile(p.id);
    status = { ...status, [p.id]: res };
    checkingId = null;
  }

  function manage() {
    onClose();
    onManage();
  }
</script>

{#if open}
  <div class="switcher-backdrop" onclick={onClose} role="presentation"></div>
  <div class="switcher" role="dialog" aria-label={dict.chat.targetTitle} tabindex="-1" use:focusTrap>
    <header class="switcher-head">
      <div>
        <strong>{dict.chat.targetTitle}</strong>
        <small>{dict.chat.targetSubtitle}</small>
      </div>
      <button type="button" class="icon-btn" onclick={onClose} aria-label={dict.common.close}>
        <X size={16} />
      </button>
    </header>

    <div class="switcher-body">
      <section class="group">
        <div class="group-head">
          <span>{dict.chat.apiSection}</span>
          <span class="group-count">{bridgeState.config.api_profiles.length}</span>
        </div>
        {#if bridgeState.config.api_profiles.length === 0}
          <button type="button" class="empty-action" onclick={manage}>{dict.chat.manageProviders}</button>
        {:else}
          {#each bridgeState.config.api_profiles as p (p.id)}
            {@const active = bridgeState.config.active_profile_id === p.id && bridgeState.config.active_mode === 'api'}
            {@const cooling = apiCooling(p)}
            <button
              type="button"
              class="target-row"
              class:active
              class:dim={!p.enabled}
              onclick={() => chooseApi(p)}
            >
              <span class="mark">{markOf(p.name)}</span>
              <span class="info">
                <strong>{p.name}</strong>
                <small>{p.model === 'auto' ? 'auto · free' : p.model}</small>
              </span>
              {#if cooling}
                <span class="state warn"><AlertTriangle size={12} /> {dict.settings.cooldown}</span>
              {:else if !p.api_key && p.kind !== 'custom_open_ai'}
                <span class="state muted">{dict.settings.keyMissing}</span>
              {:else if p.last_error}
                <span class="state bad" title={p.last_error}>{dict.web.lastError}</span>
              {:else}
                <span class="state ok"><span class="dot"></span></span>
              {/if}
              {#if active}<Check size={15} class="active-check" />{/if}
            </button>
          {/each}
        {/if}
      </section>

      {#if bridgeState.config.active_mode === 'web' && bridgeState.activeWebProfile}
        <section class="group web-current">
          <div class="group-head">
            <span>{dict.chat.webSection}</span>
            <small>{bridgeState.activeWebProfile.name}</small>
          </div>
          <div class="web-state">
            <span class="state" class:bad={webLimited(bridgeState.activeWebProfile)}>
              {#if webLimited(bridgeState.activeWebProfile)}
                <AlertTriangle size={12} /> {dict.web.limitHit}
              {:else}
                <span class="dot ok"></span> {dict.web.limitOk}
              {/if}
            </span>
            <div class="web-actions">
              <button type="button" class="mini-btn" onclick={() => bridgeState.openWebProfile(bridgeState.activeWebProfile!.id)}>
                <ExternalLink size={13} /> {dict.web.open}
              </button>
              <button
                type="button"
                class="mini-btn"
                onclick={() => checkWeb(bridgeState.activeWebProfile!)}
                disabled={checkingId === bridgeState.activeWebProfile.id}
              >
                {#if checkingId === bridgeState.activeWebProfile.id}
                  <Loader2 size={13} class="spin" /> {dict.web.checking}
                {:else}
                  <LogIn size={13} /> {dict.web.check}
                {/if}
              </button>
            </div>
            {#if status[bridgeState.activeWebProfile.id]}
              <small class="check-result" title={status[bridgeState.activeWebProfile.id].detail}>
                {statusLabel(status[bridgeState.activeWebProfile.id].status)}
              </small>
            {/if}
          </div>
        </section>
      {/if}

      {#if bridgeState.config.active_mode === 'api' && bridgeState.activeApiProfile}
        <section class="group model-group">
          <div class="group-head">
            <span>{dict.chat.chooseModel}</span>
          </div>
          <button type="button" class="current-model-row" onclick={() => (showModels = !showModels)}>
            <code>{bridgeState.activeApiProfile.model || 'auto'}</code>
            <ChevronDown size={13} class="row-chevron {showModels ? 'open' : ''}" />
          </button>
          {#if showModels}
            {#key bridgeState.activeApiProfile.id}
              <ModelList
                profile={bridgeState.activeApiProfile}
                compact
                onPicked={() => onClose()}
              />
            {/key}
          {/if}
        </section>
      {/if}

      <section class="group">
        <div class="group-head">
          <span>{dict.chat.webSection}</span>
          <span class="group-count">{bridgeState.config.web_profiles.length}</span>
        </div>
        {#each bridgeState.config.web_profiles as p (p.id)}
          {@const active = bridgeState.config.active_web_profile_id === p.id && bridgeState.config.active_mode === 'web'}
          <button type="button" class="target-row" class:active onclick={() => chooseWeb(p)}>
            <span class="mark web">{markOf(p.name)}</span>
            <span class="info">
              <strong>{p.name}</strong>
              <small>{p.user_data_dir}</small>
            </span>
            {#if webLimited(p)}
              <span class="state warn"><AlertTriangle size={12} /> {dict.web.limitHit}</span>
            {:else if p.last_error}
              <span class="state bad" title={p.last_error}>{dict.web.lastError}</span>
            {:else}
              <span class="state ok"><span class="dot"></span></span>
            {/if}
            {#if active}<Check size={15} class="active-check" />{/if}
          </button>
        {/each}
        {#if bridgeState.config.web_profiles.length === 0}
          <button type="button" class="empty-action" onclick={manage}>{dict.web.add}</button>
        {/if}
      </section>
    </div>

    <footer class="switcher-foot">
      <button type="button" class="manage-btn" onclick={manage}>
        <Settings2 size={15} /> {dict.chat.manageProviders} <ChevronRight size={14} />
      </button>
    </footer>
  </div>
{/if}

<style>
  .switcher-backdrop {
    position: fixed;
    inset: 0;
    z-index: 25;
    background: rgba(47, 43, 35, 0.16);
    backdrop-filter: blur(1px);
  }

  .switcher {
    position: absolute;
    top: 58px;
    left: 18px;
    width: min(480px, calc(100vw - 36px));
    max-height: min(76vh, 720px);
    display: flex;
    flex-direction: column;
    background: var(--bg-elev);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    box-shadow: var(--shadow-pop);
    z-index: 26;
    overflow: hidden;
  }

  .switcher-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
    padding: 14px 16px 12px;
    border-bottom: 1px solid var(--line);
  }
  .switcher-head strong {
    display: block;
    font-size: 14px;
  }
  .switcher-head small {
    display: block;
    margin-top: 2px;
    font-size: 11.5px;
    color: var(--muted);
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: 1px solid transparent;
    border-radius: 8px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .icon-btn:hover {
    background: var(--panel);
    color: var(--ink);
  }

  .switcher-body {
    overflow-y: auto;
    padding: 6px 10px 10px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .group-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 6px 4px;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .group-head small {
    text-transform: none;
    letter-spacing: 0;
    font-weight: 500;
    font-size: 11px;
    max-width: 60%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .group-count {
    font-size: 10px;
    background: var(--panel);
    padding: 1px 6px;
    border-radius: 999px;
    color: var(--ink-2);
  }

  .target-row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    text-align: left;
    padding: 8px 9px;
    border: 1px solid transparent;
    border-radius: 10px;
    background: transparent;
    cursor: pointer;
    color: var(--ink);
  }
  .target-row:hover {
    background: var(--panel);
  }
  .target-row.active {
    background: var(--accent-tint);
    border-color: rgba(79, 122, 90, 0.25);
  }
  .target-row.dim {
    opacity: 0.55;
  }

  .mark {
    flex-shrink: 0;
    width: 30px;
    height: 30px;
    border-radius: 9px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 700;
    background: var(--accent-soft);
    color: var(--accent-strong);
  }
  .mark.web {
    background: var(--warm-soft);
    color: var(--warm);
  }

  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .info strong {
    font-size: 12.5px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .info small {
    font-size: 11px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .state {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 10.5px;
    white-space: nowrap;
    color: var(--muted);
  }
  .state.ok,
  .state .dot {
    color: var(--ok);
  }
  .state .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ok);
    display: inline-block;
  }
  .state.warn {
    color: var(--warm);
  }
  .state.bad {
    color: var(--danger);
  }
  .state.muted {
    color: var(--muted);
  }
  :global(.active-check) {
    color: var(--accent);
    flex-shrink: 0;
  }

  .model-group {
    border-top: 1px dashed var(--line);
    padding-top: 4px;
  }
  .current-model-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    padding: 7px 9px;
    background: var(--bg-soft);
    border: 1px solid var(--line);
    border-radius: 8px;
    cursor: pointer;
    text-align: left;
  }
  .current-model-row:hover {
    border-color: var(--accent);
    background: var(--accent-tint);
  }
  .current-model-row code {
    font-size: 11px;
    color: var(--ink);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  :global(.row-chevron) {
    color: var(--muted);
    flex-shrink: 0;
    transition: transform 0.15s ease;
  }
  :global(.row-chevron.open) {
    transform: rotate(180deg);
  }

  .web-current {
    padding: 10px;
    background: var(--bg-soft);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
  }
  .web-state {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 4px 2px 2px;
  }
  .web-actions {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .mini-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11.5px;
    padding: 5px 9px;
    border-radius: 7px;
    border: 1px solid var(--line);
    background: var(--bg-elev);
    color: var(--ink-2);
    cursor: pointer;
  }
  .mini-btn:hover {
    border-color: var(--accent);
    color: var(--accent-strong);
  }
  .mini-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .check-result {
    font-size: 11px;
    color: var(--muted);
  }

  .empty-action {
    padding: 10px;
    border: 1px dashed var(--line);
    border-radius: 9px;
    background: transparent;
    color: var(--accent-strong);
    font-size: 12px;
    cursor: pointer;
  }
  .empty-action:hover {
    background: var(--accent-tint);
  }

  .switcher-foot {
    border-top: 1px solid var(--line);
    padding: 8px;
  }
  .manage-btn {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    padding: 8px 10px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--ink-2);
    font-size: 12px;
    cursor: pointer;
  }
  .manage-btn:hover {
    background: var(--panel);
    color: var(--ink);
  }
  :global(.manage-btn svg:last-child) {
    margin-left: auto;
  }
</style>
