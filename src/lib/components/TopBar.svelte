<script lang="ts">
  import { getDictionary, type Locale } from '../i18n/index.js';
  import { bridgeState } from '../state/appState.svelte.js';
  import type { SettingsTab } from '../types.js';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { Settings, Heart, Minus, Maximize2, X } from '@lucide/svelte';

  interface Props {
    onOpenSettings: (tab?: SettingsTab) => void;
  }
  let { onOpenSettings }: Props = $props();
  const dict = $derived(getDictionary(bridgeState.config.locale));
  const appWindow = getCurrentWindow();

  const locales: { code: Locale; label: string }[] = [
    { code: 'ru', label: 'RU' },
    { code: 'en', label: 'EN' },
    { code: 'zh', label: '中文' },
  ];

  function minimize() {
    void appWindow.minimize();
  }
  function maximize() {
    void appWindow.toggleMaximize();
  }
  function closeWindow() {
    void appWindow.close();
  }
</script>

<header class="topbar">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="drag-layer"
    onmousedown={(e) => {
      if (e.button === 0) void appWindow.startDragging();
    }}
    ondblclick={maximize}
  ></div>

  <div class="brand">
    <div class="mark">LK</div>
    <div class="brand-text">
      <strong>{dict.app.name}</strong>
      <small>{dict.app.tagline}</small>
    </div>
    <span class="version">v1.0.0</span>
  </div>

  <div class="right">
    <div class="langs">
      {#each locales as l (l.code)}
        <button
          type="button"
          class:active={bridgeState.config.locale === l.code}
          onclick={() => bridgeState.changeLocale(l.code)}
        >
          {l.label}
        </button>
      {/each}
    </div>

    <button type="button" class="icon-btn heart" onclick={() => onOpenSettings('donate')} title={dict.app.donate}>
      <Heart size={16} />
    </button>
    <button type="button" class="settings-btn" onclick={() => onOpenSettings('providers')}>
      <Settings size={15} />
      <span>{dict.app.settings}</span>
    </button>
  </div>

  <div class="window-controls">
    <button type="button" onclick={minimize} title={dict.common.minimize} aria-label={dict.common.minimize}>
      <Minus size={14} />
    </button>
    <button type="button" onclick={maximize} title={dict.common.maximize} aria-label={dict.common.maximize}>
      <Maximize2 size={12} />
    </button>
    <button type="button" class="close" onclick={closeWindow} title={dict.common.closeWindow} aria-label={dict.common.closeWindow}>
      <X size={14} />
    </button>
  </div>
</header>

<style>
  .topbar {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    height: 48px;
    padding: 0 0 0 16px;
    background: rgba(255, 253, 249, 0.82);
    border-bottom: 1px solid var(--line);
    backdrop-filter: blur(12px);
    flex-shrink: 0;
  }
  .drag-layer {
    position: absolute;
    inset: 0;
    z-index: 0;
  }

  .brand {
    position: relative;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    margin-right: auto;
    pointer-events: none;
  }
  .mark {
    width: 32px;
    height: 32px;
    border-radius: 9px;
    background: var(--accent);
    color: #fbfdf9;
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0.02em;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 4px 12px rgba(79, 122, 90, 0.25);
  }
  .brand-text {
    display: flex;
    flex-direction: column;
    line-height: 1.15;
    min-width: 0;
  }
  .brand-text strong {
    font-size: 15px;
    font-weight: 700;
    color: var(--ink);
    letter-spacing: -0.01em;
  }
  .brand-text small {
    font-size: 10.5px;
    color: var(--muted);
    white-space: nowrap;
  }
  .version {
    font-size: 9.5px;
    font-weight: 700;
    color: var(--accent-strong);
    background: var(--accent-soft);
    border-radius: 999px;
    padding: 2px 7px;
    margin-left: 2px;
  }

  .right {
    position: relative;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .langs {
    display: flex;
    gap: 1px;
    padding: 2px;
    border: 1px solid var(--line);
    background: var(--bg-elev);
    border-radius: 8px;
  }
  .langs button {
    border: none;
    background: transparent;
    color: var(--muted);
    font-size: 10.5px;
    font-weight: 700;
    padding: 4px 7px;
    border-radius: 6px;
    cursor: pointer;
  }
  .langs button:hover {
    color: var(--ink-2);
  }
  .langs button.active {
    background: var(--accent-soft);
    color: var(--accent-strong);
  }

  .icon-btn {
    width: 32px;
    height: 32px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--line);
    border-radius: 9px;
    background: var(--bg-elev);
    color: var(--ink-2);
    cursor: pointer;
  }
  .icon-btn:hover {
    border-color: var(--warm);
    color: var(--warm);
    background: var(--warm-soft);
  }

  .settings-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 7px 12px;
    border: 1px solid var(--line);
    border-radius: 9px;
    background: var(--bg-elev);
    color: var(--ink);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    box-shadow: var(--shadow-sm);
  }
  .settings-btn:hover {
    border-color: var(--accent);
    color: var(--accent-strong);
  }

  .window-controls {
    position: relative;
    z-index: 1;
    display: flex;
    align-self: stretch;
    margin-left: 4px;
  }
  .window-controls button {
    width: 42px;
    border: none;
    background: transparent;
    color: var(--muted);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
  }
  .window-controls button:hover {
    background: var(--panel);
    color: var(--ink);
  }
  .window-controls button.close:hover {
    background: var(--danger);
    color: #fff8f5;
  }

  @media (max-width: 640px) {
    .brand-text small,
    .version,
    .settings-btn span {
      display: none;
    }
    .settings-btn {
      padding: 7px 8px;
    }
    .window-controls button {
      width: 36px;
    }
  }
</style>
