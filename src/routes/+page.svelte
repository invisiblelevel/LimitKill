<script lang="ts">
  import { onMount } from 'svelte';
  import 'highlight.js/styles/github.css';
  import '../app.css';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { getDictionary } from '$lib/i18n/index.js';
  import { bridgeState } from '$lib/state/appState.svelte.js';
  import type { SettingsTab } from '$lib/types.js';
  import TopBar from '$lib/components/TopBar.svelte';
  import RagSidebar from '$lib/components/RagSidebar.svelte';
  import ChatPane from '$lib/components/ChatPane.svelte';
  import SettingsModal from '$lib/components/SettingsModal.svelte';
  import { AlertTriangle, X } from '@lucide/svelte';

  const dict = $derived(getDictionary(bridgeState.config.locale));
  let settingsOpen = $state(false);
  let settingsTab = $state<SettingsTab>('providers');
  let sidebarOpen = $state(true);
  const appWindow = getCurrentWindow();
  type ResizeDirection =
    | 'East'
    | 'North'
    | 'NorthEast'
    | 'NorthWest'
    | 'South'
    | 'SouthEast'
    | 'SouthWest'
    | 'West';

  function startResize(direction: ResizeDirection) {
    void appWindow.startResizeDragging(direction);
  }

  function openSettings(tab: SettingsTab = 'providers') {
    settingsTab = tab;
    settingsOpen = true;
  }

  onMount(() => {
    void bridgeState.init();
    if (window.innerWidth < 900) sidebarOpen = false;
  });
</script>

<div class="app-shell">
  <button type="button" tabindex="-1" aria-hidden="true" class="resize-handle n" onmousedown={(e) => { e.preventDefault(); startResize('North'); }}></button>
  <button type="button" tabindex="-1" aria-hidden="true" class="resize-handle s" onmousedown={(e) => { e.preventDefault(); startResize('South'); }}></button>
  <button type="button" tabindex="-1" aria-hidden="true" class="resize-handle e" onmousedown={(e) => { e.preventDefault(); startResize('East'); }}></button>
  <button type="button" tabindex="-1" aria-hidden="true" class="resize-handle w" onmousedown={(e) => { e.preventDefault(); startResize('West'); }}></button>
  <button type="button" tabindex="-1" aria-hidden="true" class="resize-handle ne" onmousedown={(e) => { e.preventDefault(); startResize('NorthEast'); }}></button>
  <button type="button" tabindex="-1" aria-hidden="true" class="resize-handle nw" onmousedown={(e) => { e.preventDefault(); startResize('NorthWest'); }}></button>
  <button type="button" tabindex="-1" aria-hidden="true" class="resize-handle se" onmousedown={(e) => { e.preventDefault(); startResize('SouthEast'); }}></button>
  <button type="button" tabindex="-1" aria-hidden="true" class="resize-handle sw" onmousedown={(e) => { e.preventDefault(); startResize('SouthWest'); }}></button>

  <TopBar onOpenSettings={openSettings} />

  {#if bridgeState.lastError}
    <div class="error-banner">
      <AlertTriangle size={15} />
      <span>{bridgeState.lastError}</span>
      <button type="button" onclick={() => (bridgeState.lastError = null)} aria-label={dict.common.close}>
        <X size={15} />
      </button>
    </div>
  {/if}

  <main class="app-main">
    <RagSidebar collapsed={!sidebarOpen} onOpenSettings={openSettings} />
    <ChatPane onToggleSidebar={() => (sidebarOpen = !sidebarOpen)} onOpenSettings={openSettings} />
  </main>

  {#if bridgeState.toastMessage}
    <div class="toast">{bridgeState.toastMessage}</div>
  {/if}

  <SettingsModal open={settingsOpen} initialTab={settingsTab} onClose={() => (settingsOpen = false)} />
</div>

<style>
  .app-shell {
    display: flex;
    flex-direction: column;
    height: 100vh;
    width: 100vw;
    overflow: hidden;
    background: transparent;
  }

  .resize-handle {
    position: fixed;
    z-index: 80;
    padding: 0;
    border: none;
    background: transparent;
  }
  .resize-handle.n {
    top: 0;
    left: 10px;
    right: 10px;
    height: 4px;
    cursor: n-resize;
  }
  .resize-handle.s {
    bottom: 0;
    left: 10px;
    right: 10px;
    height: 5px;
    cursor: s-resize;
  }
  .resize-handle.e {
    top: 10px;
    bottom: 10px;
    right: 0;
    width: 5px;
    cursor: e-resize;
  }
  .resize-handle.w {
    top: 10px;
    bottom: 10px;
    left: 0;
    width: 5px;
    cursor: w-resize;
  }
  .resize-handle.ne {
    top: 0;
    right: 0;
    width: 10px;
    height: 10px;
    cursor: ne-resize;
  }
  .resize-handle.nw {
    top: 0;
    left: 0;
    width: 10px;
    height: 10px;
    cursor: nw-resize;
  }
  .resize-handle.se {
    bottom: 0;
    right: 0;
    width: 12px;
    height: 12px;
    cursor: se-resize;
  }
  .resize-handle.sw {
    bottom: 0;
    left: 0;
    width: 12px;
    height: 12px;
    cursor: sw-resize;
  }

  .app-main {
    flex: 1;
    display: flex;
    min-height: 0;
    overflow: hidden;
  }

  .error-banner {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 9px 16px;
    background: var(--danger-soft);
    border-bottom: 1px solid rgba(173, 74, 63, 0.22);
    color: var(--danger);
    font-size: 12px;
    line-height: 1.45;
    flex-shrink: 0;
  }
  .error-banner span {
    flex: 1;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 96px;
    overflow-y: auto;
  }
  .error-banner button {
    border: none;
    background: transparent;
    color: inherit;
    cursor: pointer;
    padding: 1px;
    display: flex;
  }

  .toast {
    position: fixed;
    top: 64px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 60;
    max-width: 480px;
    padding: 9px 14px;
    border-radius: 10px;
    background: var(--ink);
    color: #fbf8f2;
    font-size: 12.5px;
    line-height: 1.4;
    box-shadow: var(--shadow-md);
    animation: toast-in 0.15s ease-out;
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translate(-50%, -4px);
    }
    to {
      opacity: 1;
      transform: translate(-50%, 0);
    }
  }
</style>
