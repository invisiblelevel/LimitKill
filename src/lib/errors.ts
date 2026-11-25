import { getDictionary, type Locale } from './i18n/index.js';

/**
 * Backend errors are intentionally thin strings (some contain provider HTTP details).
 * This maps the common user-facing cases to the selected UI language while keeping
 * technical details visible.
 */
export function translateError(raw: string, locale: Locale): string {
  const dict = getDictionary(locale);
  const message = raw.trim();
  if (!message) return dict.errors.generic;

  if (message.includes('Нет ни одного включённого API-профиля') || message.includes('No enabled API profiles')) {
    return dict.errors.noProfiles;
  }
  if (message.includes('Все Web-аккаунты') || message.includes('All Web accounts')) {
    return dict.errors.allWebLimited;
  }
  if (message.includes('не залогинен') || message.includes('Нужен логин') || message.includes('login required')) {
    return dict.errors.webLogin;
  }
  if (message.includes('не загрузился') || message.includes('таймаут ожидания') || message.includes('did not load')) {
    return dict.errors.webLoad;
  }
  if (message.includes('Предыдущий запрос') || message.includes('already running')) {
    return dict.errors.busy;
  }
  if (message === 'CANCELLED' || message.includes('Генерация остановлена')) {
    return dict.errors.cancelled;
  }
  if (message === 'Профиль не найден' || message === 'Web-профиль не найден') {
    return dict.errors.modelNotFound;
  }
  if (message.startsWith('Ни один API-профиль не ответил')) {
    const details = message.split('\n').slice(1).join('\n').trim();
    return details ? `${dict.errors.allApiFailed}\n\n${details}` : dict.errors.allApiFailed;
  }
  return message;
}
