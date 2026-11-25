import { en } from './en.js';
import { ru } from './ru.js';
import { zh } from './zh.js';

export type Locale = 'ru' | 'en' | 'zh';

type Widen<T> = {
  [K in keyof T]: T[K] extends string ? string : Widen<T[K]>;
};

/** RU — эталон структуры; EN/ZH обязаны иметь те же ключи, но любые строки. */
export type Dictionary = Widen<typeof ru>;

const DICTS: Record<Locale, Dictionary> = {
  ru,
  en,
  zh,
};

export function getDictionary(locale: Locale): Dictionary {
  return DICTS[locale] ?? DICTS.ru;
}
