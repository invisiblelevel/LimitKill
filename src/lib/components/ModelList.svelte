<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { getDictionary } from '../i18n/index.js';
  import { bridgeState, type ApiProfile } from '../state/appState.svelte.js';
  import { Check, RefreshCw, Search, Loader2, AlertTriangle } from '@lucide/svelte';

  interface Props {
    profile: ApiProfile;
    compact?: boolean;
    onPicked?: (model: string) => void;
  }

  let { profile, compact = false, onPicked }: Props = $props();
  const dict = $derived(getDictionary(bridgeState.config.locale));

  let models = $state<string[]>([]);
  let loading = $state(false);
  let error = $state('');
  let filter = $state('');
  let picked = $state<string | null>(null);
  let scrollEl: HTMLDivElement | undefined = $state();

  const filtered = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return models;
    return models.filter((m) => m.toLowerCase().includes(q));
  });

  async function load() {
    loading = true;
    error = '';
    try {
      models = await bridgeState.listProviderModels({
        kind: profile.kind,
        baseUrl: profile.base_url,
        apiKey: profile.api_key,
        model: profile.model,
      });
      if (models.length === 0) error = bridgeState.lastError || dict.chat.noModels;
      await tick();
      // Текущая модель может быть далеко в списке — сразу показываем её.
      // Скроллим только внутренний список, чтобы не утаскивать весь поповер.
      const activeRow = scrollEl?.querySelector<HTMLElement>('.model-row.active');
      if (scrollEl && activeRow) {
        const box = scrollEl.getBoundingClientRect();
        const row = activeRow.getBoundingClientRect();
        scrollEl.scrollTop += row.top - box.top - (box.height - row.height) / 2;
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function useModel(model: string) {
    const ok = await bridgeState.setApiProfileModel(profile.id, model);
    if (ok) {
      picked = model;
      onPicked?.(model);
    }
  }

  onMount(load);
</script>

<div class="model-list" class:compact>
  <div class="list-head">
    <div class="list-title">
      <span>{dict.chat.chooseModel}</span>
      {#if models.length}<small>{models.length} {dict.chat.modelsCount}</small>{/if}
    </div>
    <button type="button" class="icon-mini" onclick={load} disabled={loading} title={dict.chat.refreshModels}>
      {#if loading}<Loader2 size={14} class="spin" />{:else}<RefreshCw size={14} />{/if}
    </button>
  </div>

  <div class="search-box">
    <Search size={14} />
    <input type="text" bind:value={filter} placeholder={dict.chat.searchModel} />
  </div>

  {#if error && models.length === 0}
    <div class="list-error"><AlertTriangle size={14} /><span>{error}</span></div>
  {:else if loading && models.length === 0}
    <div class="list-empty"><Loader2 size={16} class="spin" /> {dict.settings.detecting}</div>
  {:else if filtered.length === 0}
    <div class="list-empty">{dict.chat.noModels}</div>
  {:else}
    <div class="model-scroll" bind:this={scrollEl}>
      {#each filtered as m (m)}
        <button
          type="button"
          class="model-row"
          class:active={profile.model === m}
          onclick={() => useModel(m)}
          title={m}
        >
          <span class="model-name">{m}</span>
          {#if profile.model === m || picked === m}
            <Check size={14} />
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .model-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--bg-soft);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    padding: 12px;
  }
  .model-list.compact {
    border: none;
    background: transparent;
    padding: 0;
  }

  .list-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .list-title {
    display: flex;
    align-items: baseline;
    gap: 8px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--ink);
  }
  .list-title small {
    font-weight: 500;
    font-size: 11px;
    color: var(--muted);
  }

  .icon-mini {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: 7px;
    border: 1px solid var(--line);
    background: var(--bg-elev);
    color: var(--ink-2);
    cursor: pointer;
  }
  .icon-mini:hover {
    border-color: var(--accent);
    color: var(--accent);
  }
  .icon-mini:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .search-box {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 6px 9px;
    background: var(--bg-elev);
    border: 1px solid var(--line);
    border-radius: 8px;
    color: var(--muted);
  }
  .search-box:focus-within {
    border-color: var(--accent);
  }
  .search-box input {
    flex: 1;
    min-width: 0;
    border: none;
    outline: none;
    background: transparent;
    font-size: 12.5px;
    color: var(--ink);
  }

  .model-scroll {
    max-height: 240px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-right: 2px;
  }
  .model-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    text-align: left;
    padding: 7px 9px;
    border: 1px solid transparent;
    border-radius: 7px;
    background: transparent;
    color: var(--ink-2);
    font-size: 12px;
    cursor: pointer;
  }
  .model-row:hover {
    background: var(--accent-tint);
    color: var(--ink);
  }
  .model-row.active {
    background: var(--accent-soft);
    border-color: rgba(79, 122, 90, 0.28);
    color: var(--accent-strong);
    font-weight: 600;
  }
  .model-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .list-empty,
  .list-error {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    padding: 10px 6px;
    color: var(--muted);
  }
  .list-error {
    color: var(--danger);
    align-items: flex-start;
  }
</style>
