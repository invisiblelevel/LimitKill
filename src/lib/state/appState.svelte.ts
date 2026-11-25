import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Locale } from '../i18n/index.js';
import { translateError } from '../errors.js';

export type ContextMode = 'none' | 'skeleton' | 'full';
export type ProviderKind = 'open_router' | 'gemini' | 'groq' | 'custom_open_ai' | 'deep_seek_web';

export interface ApiProfile {
  id: string;
  name: string;
  kind: ProviderKind;
  api_key: string;
  base_url: string;
  model: string;
  enabled: boolean;
  cooldown_until: number | null;
  last_error: string | null;
  requests_count: number;
}

export type WebKind = 'deepseek' | 'gemini' | 'qwen' | 'claude' | 'chatgpt' | 'kimi';

export interface WebProfile {
  id: string;
  name: string;
  kind: WebKind;
  url: string;
  user_data_dir: string;
  limit_hit: boolean;
  notes: string;
  limit_until: number | null;
  last_error: string | null;
  last_used_at: number | null;
}

export interface ProjectFileNode {
  path: string;
  name: string;
  is_dir: boolean;
  size_bytes: number;
  estimated_tokens: number;
  skeleton_tokens: number;
  language: string;
  context_mode: ContextMode;
}

export interface ChatAttachment {
  name: string;
  path: string;
  size_bytes: number;
  text: string;
  truncated: boolean;
}

export interface ChatMessage {
  id: string;
  role: string;
  content: string;
  provider_name: string | null;
  model_name: string | null;
  timestamp: number;
  tokens_approx: number;
  attachments?: ChatAttachment[];
}

export interface ChatThread {
  id: string;
  title: string;
  updated_at: number;
  messages: ChatMessage[];
  archived?: boolean;
}

export interface ThreadsStore {
  active_thread_id: string;
  threads: ChatThread[];
}

export interface BuiltContextResult {
  system_prompt: string;
  total_tokens: number;
  full_files_count: number;
  skeleton_files_count: number;
  truncated: boolean;
}

export interface AppConfig {
  locale: Locale;
  active_mode: 'api' | 'web';
  project_root: string | null;
  active_profile_id: string | null;
  active_web_profile_id: string | null;
  auto_rotate_on_limit: boolean;
  max_context_tokens_budget: number;
  chat_context_budget: number;
  api_profiles: ApiProfile[];
  web_profiles: WebProfile[];
}

export interface ProviderTestResult {
  ok: boolean;
  profile_id: string;
  profile_name: string;
  model: string;
  reply: string;
  elapsed_ms: number;
  error: string | null;
}

export interface WebCheckResult {
  status: 'ready' | 'login' | 'timeout' | 'error';
  detail: string;
  url: string;
  checked_at: number;
}

export interface LocalModelServer {
  name: string;
  base_url: string;
  models: string[];
}

interface BootstrapPayload {
  config: AppConfig;
  threads_store: ThreadsStore;
  files: ProjectFileNode[];
}

interface PartialEvent {
  text: string;
  profile?: string;
}

function formatError(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}

class LimitKillState {
  config = $state<AppConfig>({
    locale: 'ru',
    active_mode: 'api',
    project_root: null,
    active_profile_id: null,
    active_web_profile_id: null,
    auto_rotate_on_limit: true,
    max_context_tokens_budget: 60000,
    chat_context_budget: 128000,
    api_profiles: [],
    web_profiles: [],
  });

  threadsStore = $state<ThreadsStore>({
    active_thread_id: '',
    threads: [],
  });

  files = $state<ProjectFileNode[]>([]);
  contextPreview = $state<BuiltContextResult>({
    system_prompt: '',
    total_tokens: 0,
    full_files_count: 0,
    skeleton_files_count: 0,
    truncated: false,
  });

  sendingPrompt = $state(false);
  scanningFiles = $state(false);
  lastError = $state<string | null>(null);
  toastMessage = $state<string | null>(null);
  /** Живой текст стриминга (API или Web) для пузыря «печатает». */
  partialText = $state('');
  partialProfile = $state<string | null>(null);
  initialized = $state(false);

  activeThread = $derived(
    this.threadsStore.threads.find((t) => t.id === this.threadsStore.active_thread_id) ??
      this.threadsStore.threads[0] ??
      null
  );

  messages = $derived(this.activeThread?.messages ?? []);

  activeApiProfile = $derived(
    this.config.api_profiles.find((p) => p.id === this.config.active_profile_id) ??
      this.config.api_profiles.find((p) => p.enabled) ??
      null
  );

  activeWebProfile = $derived(
    this.config.web_profiles.find((p) => p.id === this.config.active_web_profile_id) ??
      this.config.web_profiles[0] ??
      null
  );

  streaming = $derived(this.sendingPrompt && this.partialText.length > 0);

  private listenersReady = false;

  private errorText(e: unknown): string {
    return translateError(formatError(e), this.config.locale);
  }

  notify(msg: string) {
    this.toastMessage = msg;
    setTimeout(() => {
      if (this.toastMessage === msg) {
        this.toastMessage = null;
      }
    }, 2800);
  }

  private async setupListeners() {
    if (this.listenersReady) return;
    this.listenersReady = true;
    const subscribe = async <T,>(event: string, handler: (payload: T) => void) => {
      try {
        const un: UnlistenFn = await listen<T>(event, (e) => handler(e.payload));
        void un;
      } catch {
        // Браузерный dev-режим без Tauri: события просто недоступны.
      }
    };

    await subscribe<PartialEvent>('api_partial', (p) => {
      this.partialText = p.text;
      this.partialProfile = p.profile ?? 'API';
    });
    await subscribe<PartialEvent>('web_partial', (p) => {
      this.partialText = p.text;
      this.partialProfile = p.profile ?? 'DeepSeek Web';
    });
    await subscribe<ThreadsStore>('threads_updated', (store) => {
      this.threadsStore = store;
    });
  }

  async init() {
    if (this.initialized) return;
    this.initialized = true;
    this.lastError = null;
    await this.setupListeners();
    try {
      const data = await invoke<BootstrapPayload>('get_bootstrap_state');
      this.config = data.config;
      this.threadsStore = data.threads_store;
      this.files = data.files;
      await this.refreshContextPreview();
      // Стартовое сканирование не блокирует запуск окна — запускаем после bootstrap.
      if (this.config.project_root && this.files.length === 0) {
        void this.rescanFiles();
      }
    } catch (e) {
      this.lastError = this.errorText(e);
    }
  }

  async refreshContextPreview() {
    try {
      this.contextPreview = await invoke<BuiltContextResult>('build_context_preview', {
        budget: this.config.max_context_tokens_budget,
      });
    } catch {
      // До выбора папки превью пустое — это нормально.
    }
  }

  async changeLocale(locale: Locale) {
    this.lastError = null;
    try {
      this.config = await invoke<AppConfig>('set_locale', { locale });
    } catch (e) {
      this.lastError = this.errorText(e);
    }
  }

  async createThread() {
    this.lastError = null;
    try {
      this.threadsStore = await invoke<ThreadsStore>('create_chat_thread');
    } catch (e) {
      this.lastError = this.errorText(e);
    }
  }

  async selectThread(threadId: string) {
    this.lastError = null;
    this.partialText = '';
    try {
      this.threadsStore = await invoke<ThreadsStore>('select_chat_thread', { threadId });
    } catch (e) {
      this.lastError = this.errorText(e);
    }
  }

  async deleteThread(threadId: string) {
    this.lastError = null;
    try {
      this.threadsStore = await invoke<ThreadsStore>('delete_chat_thread', { threadId });
    } catch (e) {
      this.lastError = this.errorText(e);
    }
  }

  async renameThread(threadId: string, title: string): Promise<boolean> {
    this.lastError = null;
    try {
      this.threadsStore = await invoke<ThreadsStore>('rename_chat_thread', { threadId, title });
      return true;
    } catch (e) {
      this.lastError = this.errorText(e);
      return false;
    }
  }

  async archiveThread(threadId: string, archived: boolean): Promise<boolean> {
    this.lastError = null;
    try {
      this.threadsStore = await invoke<ThreadsStore>('archive_chat_thread', {
        threadId,
        archived,
      });
      return true;
    } catch (e) {
      this.lastError = this.errorText(e);
      return false;
    }
  }

  async truncateThread(threadId: string, messageId: string): Promise<boolean> {
    this.lastError = null;
    try {
      this.threadsStore = await invoke<ThreadsStore>('truncate_chat_thread', {
        threadId,
        messageId,
      });
      return true;
    } catch (e) {
      this.lastError = this.errorText(e);
      return false;
    }
  }

  async exportThread(threadId: string, path: string, format: 'json' | 'markdown'): Promise<boolean> {
    this.lastError = null;
    try {
      await invoke('export_chat_thread', { threadId, path, format });
      return true;
    } catch (e) {
      this.lastError = this.errorText(e);
      return false;
    }
  }

  async importThread(path: string): Promise<boolean> {
    this.lastError = null;
    try {
      this.threadsStore = await invoke<ThreadsStore>('import_chat_thread', { path });
      return true;
    } catch (e) {
      this.lastError = this.errorText(e);
      return false;
    }
  }

  async clearActiveThread() {
    this.lastError = null;
    try {
      this.threadsStore = await invoke<ThreadsStore>('clear_active_thread');
    } catch (e) {
      this.lastError = this.errorText(e);
    }
  }

  async setProjectRoot(rootPath: string | null) {
    this.scanningFiles = true;
    this.lastError = null;
    try {
      this.files = await invoke<ProjectFileNode[]>('set_project_root', { rootPath });
      this.config = { ...this.config, project_root: rootPath };
      await this.refreshContextPreview();
    } catch (e) {
      this.lastError = this.errorText(e);
    } finally {
      this.scanningFiles = false;
    }
  }

  async rescanFiles() {
    this.scanningFiles = true;
    this.lastError = null;
    try {
      this.files = await invoke<ProjectFileNode[]>('rescan_project');
      await this.refreshContextPreview();
    } catch (e) {
      this.lastError = this.errorText(e);
    } finally {
      this.scanningFiles = false;
    }
  }

  async cycleFileMode(file: ProjectFileNode) {
    const next: ContextMode =
      file.context_mode === 'none' ? 'skeleton' : file.context_mode === 'skeleton' ? 'full' : 'none';
    const previous = file.context_mode;
    file.context_mode = next;
    this.files = [...this.files];
    this.lastError = null;
    try {
      this.files = await invoke<ProjectFileNode[]>('set_file_context_mode', {
        filePath: file.path,
        mode: next,
      });
      await this.refreshContextPreview();
    } catch (e) {
      file.context_mode = previous;
      this.files = [...this.files];
      this.lastError = this.errorText(e);
    }
  }

  async updateBridgeConfig(partial: Partial<AppConfig>) {
    this.lastError = null;
    const merged: AppConfig = { ...this.config, ...partial };
    try {
      this.config = await invoke<AppConfig>('update_bridge_settings', {
        activeMode: merged.active_mode,
        activeProfileId: merged.active_profile_id,
        activeWebProfileId: merged.active_web_profile_id,
        autoRotateOnLimit: merged.auto_rotate_on_limit,
        maxContextTokensBudget: merged.max_context_tokens_budget,
      });
      await this.refreshContextPreview();
    } catch (e) {
      this.lastError = this.errorText(e);
    }
  }

  async sendChatPrompt(
    prompt: string,
    attachments: ChatAttachment[] = []
  ): Promise<'ok' | 'cancelled' | 'error'> {
    this.sendingPrompt = true;
    this.lastError = null;
    this.partialText = '';
    this.partialProfile = null;
    try {
      this.threadsStore = await invoke<ThreadsStore>('send_prompt', {
        prompt,
        attachments,
      });
      const data = await invoke<BootstrapPayload>('get_bootstrap_state');
      this.config = data.config;
      await this.refreshContextPreview();
      return 'ok';
    } catch (e) {
      const raw = formatError(e);
      if (raw.includes('CANCELLED')) {
        // Пользователь остановил генерацию — это не ошибка.
        this.lastError = null;
        return 'cancelled';
      }
      this.lastError = translateError(raw, this.config.locale);
      return 'error';
    } finally {
      this.sendingPrompt = false;
      this.partialText = '';
      this.partialProfile = null;
    }
  }

  async readChatAttachments(paths: string[]): Promise<ChatAttachment[]> {
    this.lastError = null;
    try {
      return await invoke<ChatAttachment[]>('read_chat_attachments', { paths });
    } catch (e) {
      this.lastError = this.errorText(e);
      return [];
    }
  }

  async cancelPrompt() {
    try {
      await invoke('cancel_prompt');
    } catch {
      // Если запрос уже завершился — отменять нечего.
    }
  }

  async handoffToNewThread(note?: string): Promise<boolean> {
    this.lastError = null;
    try {
      this.threadsStore = await invoke<ThreadsStore>('handoff_to_new_thread', {
        note: note ?? null,
      });
      return true;
    } catch (e) {
      this.lastError = this.errorText(e);
      return false;
    }
  }

  async buildHandoffPacket(extraNote?: string): Promise<string | null> {
    this.lastError = null;
    try {
      return await invoke<string>('create_handoff_packet', {
        extraNote: extraNote ?? null,
      });
    } catch (e) {
      this.lastError = this.errorText(e);
      return null;
    }
  }

  async openWebProfile(profileId: string, background = false) {
    this.lastError = null;
    try {
      this.config = await invoke<AppConfig>('open_web_profile', { profileId, background });
    } catch (e) {
      this.lastError = this.errorText(e);
    }
  }

  async closeWebProfile(profileId: string) {
    try {
      await invoke('close_web_profile', { profileId });
    } catch (e) {
      this.lastError = this.errorText(e);
    }
  }

  async saveApiProfile(payload: {
    id?: string;
    name: string;
    kind: ProviderKind;
    apiKey: string;
    baseUrl: string;
    model: string;
    enabled: boolean;
  }): Promise<boolean> {
    this.lastError = null;
    try {
      this.config = await invoke<AppConfig>('upsert_api_profile', {
        id: payload.id ?? null,
        name: payload.name,
        kind: payload.kind,
        apiKey: payload.apiKey,
        baseUrl: payload.baseUrl,
        model: payload.model,
        enabled: payload.enabled,
      });
      return true;
    } catch (e) {
      this.lastError = this.errorText(e);
      return false;
    }
  }

  async removeApiProfile(id: string) {
    this.lastError = null;
    try {
      this.config = await invoke<AppConfig>('delete_api_profile', { id });
    } catch (e) {
      this.lastError = this.errorText(e);
    }
  }

  async setApiProfileModel(id: string, model: string): Promise<boolean> {
    this.lastError = null;
    try {
      this.config = await invoke<AppConfig>('set_api_profile_model', { id, model });
      return true;
    } catch (e) {
      this.lastError = this.errorText(e);
      return false;
    }
  }

  async selectApiProfile(id: string) {
    await this.updateBridgeConfig({ active_profile_id: id, active_mode: 'api' });
  }

  async selectWebProfile(id: string) {
    await this.updateBridgeConfig({ active_web_profile_id: id, active_mode: 'web' });
  }

  async testApiProfile(id: string): Promise<ProviderTestResult | null> {
    this.lastError = null;
    try {
      const result = await invoke<ProviderTestResult>('test_api_profile', { id });
      if (result.error) {
        result.error = translateError(result.error, this.config.locale);
      }
      const data = await invoke<BootstrapPayload>('get_bootstrap_state');
      this.config = data.config;
      return result;
    } catch (e) {
      this.lastError = this.errorText(e);
      return null;
    }
  }

  async discoverLocalModels(): Promise<LocalModelServer[]> {
    this.lastError = null;
    try {
      return await invoke<LocalModelServer[]>('discover_local_models');
    } catch (e) {
      this.lastError = this.errorText(e);
      return [];
    }
  }

  async listProviderModels(payload: {
    kind: ProviderKind;
    baseUrl: string;
    apiKey: string;
    model: string;
  }): Promise<string[]> {
    this.lastError = null;
    try {
      return await invoke<string[]>('list_provider_models', {
        kind: payload.kind,
        baseUrl: payload.baseUrl,
        apiKey: payload.apiKey,
        model: payload.model,
      });
    } catch (e) {
      this.lastError = this.errorText(e);
      return [];
    }
  }

  async saveWebProfile(payload: {
    id?: string;
    name: string;
    kind?: WebKind;
    url: string;
    notes?: string;
  }): Promise<boolean> {
    this.lastError = null;
    try {
      this.config = await invoke<AppConfig>('upsert_web_profile', {
        id: payload.id ?? null,
        name: payload.name,
        kind: payload.kind ?? 'deepseek',
        url: payload.url,
        notes: payload.notes ?? '',
      });
      return true;
    } catch (e) {
      this.lastError = this.errorText(e);
      return false;
    }
  }

  async removeWebProfile(id: string) {
    this.lastError = null;
    try {
      this.config = await invoke<AppConfig>('delete_web_profile', { id });
    } catch (e) {
      this.lastError = this.errorText(e);
    }
  }

  async setWebProfileLimit(profileId: string, limitHit: boolean) {
    this.lastError = null;
    try {
      this.config = await invoke<AppConfig>('set_web_profile_limit', { profileId, limitHit });
    } catch (e) {
      this.lastError = this.errorText(e);
    }
  }

  async resetWebProfile(profileId: string) {
    this.lastError = null;
    try {
      this.config = await invoke<AppConfig>('reset_web_profile', { profileId });
    } catch (e) {
      this.lastError = this.errorText(e);
    }
  }

  async checkWebProfile(profileId: string): Promise<WebCheckResult> {
    this.lastError = null;
    try {
      const result = await invoke<WebCheckResult>('check_web_profile', { profileId });
      const data = await invoke<BootstrapPayload>('get_bootstrap_state');
      this.config = data.config;
      return result;
    } catch (e) {
      const message = translateError(formatError(e), this.config.locale);
      this.lastError = message;
      return {
        status: 'error',
        detail: message,
        url: '',
        checked_at: Math.floor(Date.now() / 1000),
      };
    }
  }
}

export const bridgeState = new LimitKillState();
