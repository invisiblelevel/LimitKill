<script lang="ts">
  import { open, save } from '@tauri-apps/plugin-dialog';
  import { tick } from 'svelte';
  import { getDictionary } from '../i18n/index.js';
  import { bridgeState, type ChatThread, type ProjectFileNode } from '../state/appState.svelte.js';
  import type { SettingsTab } from '../types.js';
  import {
    FolderOpen,
    Search,
    FileCode2,
    Plus,
    Trash2,
    MessageSquare,
    RefreshCw,
    X,
    AlertTriangle,
    Loader2,
    SlidersHorizontal,
    MoreHorizontal,
    Pencil,
    Archive,
    ArchiveRestore,
    Download,
    Upload,
  } from '@lucide/svelte';

  interface Props {
    collapsed?: boolean;
    onOpenSettings: (tab?: SettingsTab) => void;
  }
  let { collapsed = false, onOpenSettings }: Props = $props();

  const dict = $derived(getDictionary(bridgeState.config.locale));
  let tab = $state<'chats' | 'context'>('chats');
  let filter = $state('');
  let showArchived = $state(false);
  let menu = $state<{ id: string; x: number; y: number } | null>(null);
  let editingId = $state<string | null>(null);
  let editTitle = $state('');
  let editInput: HTMLInputElement | undefined = $state();

  const MAX_VISIBLE_FILES = 1200;

  const activeThreads = $derived(
    bridgeState.threadsStore.threads.filter((t) => !t.archived)
  );
  const archivedThreads = $derived(
    bridgeState.threadsStore.threads.filter((t) => !!t.archived)
  );
  const visibleThreads = $derived(showArchived ? archivedThreads : activeThreads);
  const menuThread = $derived(
    menu ? bridgeState.threadsStore.threads.find((t) => t.id === menu?.id) ?? null : null
  );

  const selectedCount = $derived(
    bridgeState.files.filter((f) => f.context_mode !== 'none').length
  );
  const contextPercent = $derived.by(() => {
    const budget = bridgeState.config.max_context_tokens_budget || 1;
    return Math.min(100, Math.round((bridgeState.contextPreview.total_tokens / budget) * 100));
  });
  const projectName = $derived(
    bridgeState.config.project_root
      ? bridgeState.config.project_root.split(/[\\/]/).pop() ?? bridgeState.config.project_root
      : null
  );

  const visibleFiles = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    const list = q
      ? bridgeState.files.filter((f) => f.path.toLowerCase().includes(q))
      : bridgeState.files;
    return list.slice(0, MAX_VISIBLE_FILES);
  });

  const hiddenCount = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    const total = q
      ? bridgeState.files.filter((f) => f.path.toLowerCase().includes(q)).length
      : bridgeState.files.length;
    return Math.max(0, total - visibleFiles.length);
  });

  function threadPreview(title: string, messages: { content: string }[]): string {
    const last = messages[messages.length - 1]?.content ?? '';
    const clean = last
      .replace(/^>\s*\*?Авто-переключение.*$/gim, '')
      .replace(/^#+\s*/gm, '')
      .replace(/```[a-z]*/gi, '')
      .replace(/```/g, '')
      .replace(/\s+/g, ' ')
      .trim();
    return clean || title;
  }

  function relativeTime(ts: number): string {
    const diff = Math.max(0, Math.floor(Date.now() / 1000) - ts);
    const rtf = new Intl.RelativeTimeFormat(bridgeState.config.locale, { numeric: 'auto' });
    if (diff < 60) return rtf.format(-diff, 'second');
    if (diff < 3600) return rtf.format(-Math.floor(diff / 60), 'minute');
    if (diff < 86400) return rtf.format(-Math.floor(diff / 3600), 'hour');
    if (diff < 86400 * 30) return rtf.format(-Math.floor(diff / 86400), 'day');
    return new Date(ts * 1000).toLocaleDateString();
  }

  function openThreadMenu(e: MouseEvent, thread: ChatThread) {
    e.preventDefault();
    e.stopPropagation();
    const x = Math.min(e.clientX, window.innerWidth - 210);
    const y = Math.min(e.clientY, window.innerHeight - 290);
    menu = { id: thread.id, x: Math.max(8, x), y: Math.max(8, y) };
  }

  function openThreadMenuFromButton(e: MouseEvent, thread: ChatThread) {
    e.stopPropagation();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menu = {
      id: thread.id,
      x: Math.max(8, Math.min(rect.right - 200, window.innerWidth - 210)),
      y: Math.min(rect.bottom + 4, window.innerHeight - 290),
    };
  }

  function selectThread(id: string) {
    menu = null;
    void bridgeState.selectThread(id);
  }

  async function beginRename(thread: ChatThread) {
    menu = null;
    editingId = thread.id;
    editTitle = thread.title;
    await tick();
    editInput?.focus();
    editInput?.select();
  }

  async function saveRename(thread: ChatThread) {
    if (editingId !== thread.id) return;
    const title = editTitle.trim();
    editingId = null;
    if (!title || title === thread.title) return;
    await bridgeState.renameThread(thread.id, title);
  }

  function cancelRename() {
    editingId = null;
    editTitle = '';
  }

  async function toggleArchive(thread: ChatThread) {
    menu = null;
    await bridgeState.archiveThread(thread.id, !thread.archived);
  }

  async function deleteThread(thread: ChatThread) {
    menu = null;
    if (!confirm(dict.threads.deleteConfirm)) return;
    await bridgeState.deleteThread(thread.id);
  }

  async function exportThread(thread: ChatThread, format: 'json' | 'markdown') {
    menu = null;
    const safe = (thread.title || 'chat').replace(/[\\/:*?"<>|]/g, '_').slice(0, 60);
    const path = await save({
      defaultPath: `${safe}.${format === 'json' ? 'json' : 'md'}`,
      filters:
        format === 'json'
          ? [{ name: 'LimitKill JSON', extensions: ['json'] }]
          : [{ name: 'Markdown', extensions: ['md'] }],
    });
    if (typeof path !== 'string') return;
    if (await bridgeState.exportThread(thread.id, path, format)) {
      bridgeState.notify(dict.threads.exported);
    }
  }

  async function importThread() {
    const path = await open({
      multiple: false,
      filters: [{ name: 'LimitKill JSON', extensions: ['json'] }],
      title: dict.threads.import,
    });
    if (typeof path !== 'string') return;
    if (await bridgeState.importThread(path)) {
      bridgeState.notify(dict.threads.imported);
      showArchived = false;
    }
  }

  async function pickFolder() {
    const sel = await open({ directory: true, multiple: false, title: dict.rag.pickFolder });
    if (typeof sel === 'string') await bridgeState.setProjectRoot(sel);
  }

  function modeText(mode: ProjectFileNode['context_mode']): string {
    if (mode === 'full') return dict.rag.full;
    if (mode === 'skeleton') return dict.rag.skel;
    return dict.rag.off;
  }

  function displayTokens(file: ProjectFileNode): number {
    return file.context_mode === 'skeleton' ? file.skeleton_tokens : file.estimated_tokens;
  }
</script>

<svelte:window
  onmousedown={(e) => {
    const target = e.target as Element | null;
    if (
      menu &&
      !target?.closest('.thread-menu') &&
      !target?.closest('.thread-more')
    ) {
      menu = null;
    }
  }}
  onkeydown={(e) => {
    if (e.key === 'Escape') {
      menu = null;
      if (editingId) cancelRename();
    }
  }}
/>

<aside class="sidebar" class:collapsed>
  <div class="side-tabs">
    <button type="button" class:active={tab === 'chats'} onclick={() => (tab = 'chats')}>
      {dict.rag.chatsTab}
      <span class="count">{activeThreads.length}</span>
    </button>
    <button type="button" class:active={tab === 'context'} onclick={() => (tab = 'context')}>
      {dict.rag.contextTab}
      <span class="count">{selectedCount}/{bridgeState.files.length}</span>
    </button>
  </div>

  {#if tab === 'chats'}
    <div class="pane">
      <div class="thread-actions">
        <button type="button" class="new-chat" onclick={() => bridgeState.createThread()}>
          <Plus size={15} /> {dict.threads.newChat}
        </button>
        <button type="button" class="thread-tool" onclick={importThread} title={dict.threads.import}>
          <Upload size={14} />
        </button>
      </div>

      <div class="thread-list" role="list">
        {#each visibleThreads as t (t.id)}
          <div
            class="thread-item"
            role="listitem"
            class:active={t.id === bridgeState.threadsStore.active_thread_id}
            class:archived={t.archived}
            oncontextmenu={(e) => openThreadMenu(e, t)}
          >
            {#if editingId === t.id}
              <input
                class="thread-rename"
                bind:this={editInput}
                bind:value={editTitle}
                maxlength="120"
                onkeydown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault();
                    void saveRename(t);
                  } else if (e.key === 'Escape') {
                    e.preventDefault();
                    cancelRename();
                  }
                }}
                onblur={() => void saveRename(t)}
              />
            {:else}
              <button type="button" class="thread-select" onclick={() => selectThread(t.id)}>
                {#if t.archived}
                  <Archive size={14} class="thread-icon" />
                {:else}
                  <MessageSquare size={14} class="thread-icon" />
                {/if}
                <span class="thread-content">
                  <span class="thread-title">{t.title}</span>
                  <small class="thread-preview">{threadPreview(t.title, t.messages)}</small>
                </span>
                <span class="thread-meta">
                  <small>{relativeTime(t.updated_at)}</small>
                </span>
              </button>
              <button
                type="button"
                class="thread-more"
                title={dict.threads.menu}
                aria-label={dict.threads.menu}
                onclick={(e) => openThreadMenuFromButton(e, t)}
              >
                <MoreHorizontal size={15} />
              </button>
            {/if}
          </div>
        {/each}
        {#if visibleThreads.length === 0}
          <div class="pane-empty">
            {showArchived ? dict.threads.noArchived : dict.threads.empty}
          </div>
        {/if}
      </div>

      <div class="archive-row">
        <button
          type="button"
          class="archive-toggle"
          class:active={showArchived}
          onclick={() => (showArchived = !showArchived)}
          disabled={archivedThreads.length === 0 && !showArchived}
        >
          <Archive size={13} />
          {showArchived ? dict.threads.backToChats : dict.threads.archive}
          {#if archivedThreads.length}<span>{archivedThreads.length}</span>{/if}
        </button>
      </div>

      {#if menu && menuThread}
        {@const target = menuThread}
        <div class="thread-menu" role="menu" style="left: {menu.x}px; top: {menu.y}px">
          <button type="button" role="menuitem" onclick={() => selectThread(target.id)}>
            <MessageSquare size={14} /> {dict.threads.open}
          </button>
          <button type="button" role="menuitem" onclick={() => beginRename(target)}>
            <Pencil size={14} /> {dict.threads.rename}
          </button>
          <button type="button" role="menuitem" onclick={() => toggleArchive(target)}>
            {#if target.archived}
              <ArchiveRestore size={14} /> {dict.threads.unarchive}
            {:else}
              <Archive size={14} /> {dict.threads.toArchive}
            {/if}
          </button>
          <div class="menu-sep"></div>
          <button type="button" role="menuitem" onclick={() => exportThread(target, 'json')}>
            <Download size={14} /> {dict.threads.exportJson}
          </button>
          <button type="button" role="menuitem" onclick={() => exportThread(target, 'markdown')}>
            <Download size={14} /> {dict.threads.exportMarkdown}
          </button>
          <div class="menu-sep"></div>
          <button type="button" role="menuitem" class="danger" onclick={() => deleteThread(target)}>
            <Trash2 size={14} /> {dict.threads.deleteChat}
          </button>
        </div>
      {/if}

      <button type="button" class="context-summary" onclick={() => (tab = 'context')}>
        <span class="summary-top">
          <span class="summary-project">
            <FolderOpen size={14} />
            {projectName ?? dict.rag.noFolder.split(',')[0]}
          </span>
          <span class="summary-percent">{contextPercent}%</span>
        </span>
        <span class="progress" aria-hidden="true">
          <span style="width: {contextPercent}%"></span>
        </span>
        <span class="summary-meta">
          <span>{selectedCount}/{bridgeState.files.length} {dict.rag.files}</span>
          <span>{bridgeState.contextPreview.total_tokens} {dict.rag.tokens}</span>
        </span>
      </button>
    </div>
  {:else}
    <div class="pane">
      <div class="context-top">
        <div class="folder-row">
          <button type="button" class="folder-btn" onclick={pickFolder} title={dict.rag.pickFolder}>
            <FolderOpen size={15} />
            <span>{bridgeState.config.project_root ? dict.rag.changeFolder : dict.rag.pickFolder}</span>
          </button>
          {#if bridgeState.config.project_root}
            <button
              type="button"
              class="icon-mini"
              onclick={() => bridgeState.rescanFiles()}
              title={dict.rag.rescan}
            >
              {#if bridgeState.scanningFiles}
                <Loader2 size={14} class="spin" />
              {:else}
                <RefreshCw size={14} />
              {/if}
            </button>
            <button
              type="button"
              class="icon-mini danger"
              onclick={() => bridgeState.setProjectRoot(null)}
              title={dict.rag.clearFolder}
            >
              <X size={14} />
            </button>
          {/if}
        </div>
        {#if bridgeState.config.project_root}
          <div class="root-path" title={bridgeState.config.project_root}>{bridgeState.config.project_root}</div>
        {/if}
      </div>

      {#if bridgeState.config.project_root}
        <div class="search-box">
          <Search size={14} />
          <input type="text" bind:value={filter} placeholder={dict.rag.filter} />
          {#if filter}
            <button type="button" class="clear-filter" onclick={() => (filter = '')}><X size={12} /></button>
          {/if}
        </div>

        <div class="metrics">
          <span class="metric"><b>{bridgeState.contextPreview.full_files_count}</b> {dict.rag.fullCount}</span>
          <span class="metric"><b>{bridgeState.contextPreview.skeleton_files_count}</b> {dict.rag.skelCount}</span>
          <span class="metric"><b>{bridgeState.contextPreview.total_tokens}</b> {dict.rag.tokens}</span>
        </div>

        {#if bridgeState.contextPreview.truncated}
          <div class="warn">
            <AlertTriangle size={13} />
            <span>{dict.rag.truncated}</span>
          </div>
        {/if}

        <div class="hint">{dict.rag.hint}</div>

        <div class="file-list">
          {#if bridgeState.scanningFiles && bridgeState.files.length === 0}
            <div class="pane-empty"><Loader2 size={16} class="spin" /> {dict.rag.rescan}…</div>
          {:else if visibleFiles.length === 0}
            <div class="pane-empty">{dict.rag.noneFound}</div>
          {:else}
            {#each visibleFiles as file (file.path)}
              <button
                type="button"
                class="file-row mode-{file.context_mode}"
                onclick={() => bridgeState.cycleFileMode(file)}
                title={file.path}
              >
                <FileCode2 size={13} class="file-icon" />
                <span class="file-name">{file.path}</span>
                <span class="file-tokens">{displayTokens(file)}</span>
                <span class="mode-tag tag-{file.context_mode}">{modeText(file.context_mode)}</span>
              </button>
            {/each}
            {#if hiddenCount > 0}
              <div class="more-files">+{hiddenCount} {dict.rag.files}</div>
            {/if}
          {/if}
        </div>

        <button type="button" class="budget-link" onclick={() => onOpenSettings('general')}>
          <SlidersHorizontal size={14} />
          {dict.rag.budget}: {bridgeState.config.max_context_tokens_budget}
        </button>
      {:else}
        <div class="folder-empty">
          <div class="folder-art"><FolderOpen size={28} /></div>
          <p>{dict.rag.noFolder}</p>
          <button type="button" class="primary-btn" onclick={pickFolder}>
            <FolderOpen size={15} /> {dict.rag.pickFolder}
          </button>
        </div>
      {/if}
    </div>
  {/if}
</aside>

<style>
  .sidebar {
    width: 272px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: #f2ede3;
    border-right: 1px solid var(--line);
    transition: width 0.18s ease, opacity 0.18s ease;
    overflow: hidden;
  }
  .sidebar.collapsed {
    width: 0;
    opacity: 0;
    border-right: none;
    pointer-events: none;
  }

  .side-tabs {
    display: flex;
    gap: 4px;
    padding: 10px 10px 8px;
  }
  .side-tabs button {
    flex: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 7px 8px;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
    background: transparent;
    border: 1px solid transparent;
    border-radius: 9px;
    cursor: pointer;
    white-space: nowrap;
  }
  .side-tabs button:hover {
    color: var(--ink-2);
    background: var(--panel);
  }
  .side-tabs button.active {
    color: var(--accent-strong);
    background: var(--accent-soft);
    border-color: rgba(79, 122, 90, 0.18);
  }
  .count {
    font-size: 10px;
    font-weight: 600;
    background: rgba(0, 0, 0, 0.05);
    border-radius: 999px;
    padding: 1px 6px;
  }

  .pane {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 0 10px 10px;
    gap: 8px;
  }

  .thread-actions {
    display: flex;
    gap: 5px;
  }
  .new-chat {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    flex: 1;
    padding: 9px;
    border: 1px dashed var(--line-strong);
    border-radius: 10px;
    background: transparent;
    color: var(--accent-strong);
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .thread-tool {
    width: 36px;
    border: 1px dashed var(--line-strong);
    border-radius: 10px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .thread-tool:hover {
    color: var(--accent-strong);
    background: var(--accent-tint);
    border-style: solid;
  }
  .new-chat:hover {
    background: var(--accent-tint);
    border-style: solid;
  }

  .thread-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-right: 2px;
  }
  .thread-item {
    display: flex;
    align-items: stretch;
    gap: 2px;
    border-radius: 10px;
    border: 1px solid transparent;
    padding: 1px;
  }
  .thread-item:hover {
    background: var(--bg-elev);
  }
  .thread-item.active {
    background: var(--accent-soft);
    border-color: rgba(79, 122, 90, 0.16);
  }
  .thread-select {
    flex: 1;
    display: flex;
    align-items: flex-start;
    gap: 7px;
    min-width: 0;
    padding: 8px 7px;
    border: none;
    background: transparent;
    color: var(--ink-2);
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .thread-icon {
    margin-top: 2px;
    flex-shrink: 0;
    color: var(--muted);
  }
  .thread-item.active .thread-icon {
    color: var(--accent-strong);
  }
  .thread-content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .thread-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ink);
    font-weight: 600;
    font-size: 12px;
  }
  .thread-item.active .thread-title {
    color: var(--accent-strong);
  }
  .thread-preview {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
    font-size: 10.5px;
    line-height: 1.3;
    font-weight: 400;
  }
  .thread-meta {
    flex-shrink: 0;
    padding-top: 2px;
  }
  .thread-meta small {
    font-size: 9.5px;
    color: var(--muted);
    white-space: nowrap;
  }
  .thread-more {
    border: none;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    padding: 5px;
    border-radius: 7px;
    opacity: 0.55;
    align-self: center;
    flex-shrink: 0;
  }
  .thread-item:hover .thread-more,
  .thread-more:focus-visible {
    opacity: 1;
  }
  .thread-more:hover {
    color: var(--ink);
    background: var(--panel);
  }
  .thread-rename {
    flex: 1;
    min-width: 0;
    margin: 4px;
    padding: 6px 7px;
    border: 1px solid var(--accent);
    border-radius: 7px;
    background: var(--bg-elev);
    color: var(--ink);
    font-size: 12px;
    outline: none;
  }
  .thread-item.archived .thread-title {
    color: var(--muted);
  }

  .archive-row {
    display: flex;
    justify-content: flex-start;
  }
  .archive-toggle {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: none;
    background: transparent;
    color: var(--muted);
    font-size: 10.5px;
    cursor: pointer;
    padding: 3px 4px;
  }
  .archive-toggle:hover,
  .archive-toggle.active {
    color: var(--accent-strong);
  }
  .archive-toggle:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .archive-toggle span {
    background: var(--panel);
    border-radius: 999px;
    padding: 0 5px;
    font-size: 9px;
  }

  .thread-menu {
    position: fixed;
    z-index: 120;
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
  .thread-menu button {
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
  .thread-menu button:hover {
    background: var(--panel);
    color: var(--ink);
  }
  .thread-menu button.danger {
    color: var(--danger);
  }
  .thread-menu button.danger:hover {
    background: var(--danger-soft);
  }
  .menu-sep {
    height: 1px;
    margin: 3px 4px;
    background: var(--line);
  }

  .context-summary {
    display: flex;
    flex-direction: column;
    gap: 6px;
    border: 1px solid var(--line);
    background: var(--bg-elev);
    border-radius: 10px;
    padding: 9px 10px;
    color: var(--ink-2);
    cursor: pointer;
    text-align: left;
  }
  .context-summary:hover {
    border-color: var(--accent);
  }
  .summary-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
  }
  .summary-project {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--ink);
  }
  .summary-percent {
    font-size: 10px;
    color: var(--muted);
    flex-shrink: 0;
  }
  .progress {
    display: block;
    height: 4px;
    background: var(--panel);
    border-radius: 999px;
    overflow: hidden;
  }
  .progress span {
    display: block;
    height: 100%;
    background: var(--accent);
    border-radius: 999px;
    transition: width 0.2s ease;
  }
  .summary-meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    font-size: 9.5px;
    color: var(--muted);
  }

  .pane-empty {
    padding: 20px 12px;
    text-align: center;
    color: var(--muted);
    font-size: 12px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }

  .context-top {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .folder-row {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .folder-btn {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    padding: 8px 9px;
    border: 1px solid var(--line);
    border-radius: 9px;
    background: var(--bg-elev);
    color: var(--ink-2);
    font-size: 12px;
    cursor: pointer;
  }
  .folder-btn:hover {
    border-color: var(--accent);
    color: var(--accent-strong);
  }
  .folder-btn span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .icon-mini {
    width: 30px;
    height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg-elev);
    color: var(--ink-2);
    cursor: pointer;
    flex-shrink: 0;
  }
  .icon-mini:hover {
    border-color: var(--accent);
    color: var(--accent-strong);
  }
  .icon-mini.danger:hover {
    border-color: var(--danger);
    color: var(--danger);
    background: var(--danger-soft);
  }
  .root-path {
    font-size: 10px;
    color: var(--muted);
    padding: 0 2px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .search-box {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 7px 9px;
    border: 1px solid var(--line);
    border-radius: 9px;
    background: var(--bg-elev);
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
    font-size: 12px;
    color: var(--ink);
  }
  .clear-filter {
    border: none;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    display: flex;
    padding: 0;
  }

  .metrics {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .metric {
    flex: 1;
    text-align: center;
    background: var(--bg-elev);
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 5px 4px;
    font-size: 10px;
    color: var(--muted);
    white-space: nowrap;
  }
  .metric b {
    display: block;
    color: var(--accent-strong);
    font-size: 12px;
  }

  .warn {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    padding: 7px 9px;
    border-radius: 8px;
    background: var(--warm-soft);
    border: 1px solid rgba(184, 117, 69, 0.25);
    color: var(--warm);
    font-size: 10.5px;
    line-height: 1.4;
  }
  .hint {
    padding: 0 2px;
    font-size: 10px;
    color: var(--muted);
  }

  .file-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding-right: 2px;
  }
  .file-row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 6px 7px;
    border: 1px solid transparent;
    border-radius: 7px;
    background: transparent;
    color: var(--ink-2);
    font-size: 11px;
    cursor: pointer;
    text-align: left;
  }
  .file-row:hover {
    background: var(--panel);
  }
  .file-icon {
    flex-shrink: 0;
    color: var(--muted);
  }
  .file-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .file-tokens {
    font-size: 9.5px;
    color: var(--muted);
    flex-shrink: 0;
  }
  .mode-tag {
    font-size: 9px;
    font-weight: 600;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--panel);
    color: var(--muted);
    flex-shrink: 0;
  }
  .file-row.mode-skeleton {
    background: rgba(184, 117, 69, 0.06);
  }
  .file-row.mode-skeleton .mode-tag {
    background: var(--warm-soft);
    color: var(--warm);
  }
  .file-row.mode-full {
    background: var(--accent-soft);
    color: var(--accent-strong);
  }
  .file-row.mode-full .mode-tag {
    background: var(--accent);
    color: #fbfdf9;
  }
  .more-files {
    text-align: center;
    font-size: 10px;
    color: var(--muted);
    padding: 6px;
  }

  .budget-link {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: transparent;
    color: var(--muted);
    font-size: 10.5px;
    padding: 6px;
    cursor: pointer;
  }
  .budget-link:hover {
    border-color: var(--accent);
    color: var(--accent-strong);
  }

  .folder-empty {
    margin: auto;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 20px 14px;
  }
  .folder-art {
    width: 52px;
    height: 52px;
    border-radius: 15px;
    background: var(--panel);
    color: var(--muted);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .folder-empty p {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--ink-2);
  }
  .primary-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 8px 13px;
    border: none;
    border-radius: 9px;
    background: var(--accent);
    color: #fbfdf9;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .primary-btn:hover {
    background: var(--accent-strong);
  }
</style>
