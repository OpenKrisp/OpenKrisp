import type { ApiError } from "./api";
import { en, type MessageId } from "./locales/en";
import { es } from "./locales/es";
import { ptBR } from "./locales/pt-BR";
import { fr } from "./locales/fr";
import { de } from "./locales/de";

/** This registry owns the supported locales, native names and frontend catalogs. */
export const languages = {
  en: { name: "English", shortName: "English", catalog: en, base: "en" },
  es: { name: "Español", shortName: "Español", catalog: es, base: "es" },
  "pt-BR": { name: "Português (Brasil)", shortName: "Português", catalog: ptBR, base: "pt" },
  fr: { name: "Français", shortName: "Français", catalog: fr, base: "fr" },
  de: { name: "Deutsch", shortName: "Deutsch", catalog: de, base: "de" },
} as const;

export type Language = keyof typeof languages;
export type LanguagePreference = Language | "system";
export type Translate = (id: MessageId, values?: Record<string, string | number>) => string;
export type { MessageId };
export const LANGUAGE_STORAGE_KEY = "openkrisp.language";
export const languageOptions = (Object.keys(languages) as Language[]).map((id) => ({ id, ...languages[id] }));
export const catalogs = Object.fromEntries(languageOptions.map(({ id, catalog }) => [id, catalog])) as Record<Language, Record<MessageId, string>>;

export function isLanguagePreference(value: unknown): value is LanguagePreference {
  return value === "system" || (typeof value === "string" && Object.prototype.hasOwnProperty.call(languages, value));
}

export function resolveLanguage(preference: LanguagePreference, locales: readonly string[]): Language {
  if (preference !== "system") return preference;
  for (const locale of locales) {
    const normalized = locale.trim().replace(/_/g, "-").toLowerCase();
    const exact = languageOptions.find(({ id }) => id.toLowerCase() === normalized);
    if (exact) return exact.id;
    const base = normalized.split("-")[0];
    const match = languageOptions.find((option) => option.base === base);
    if (match) return match.id;
  }
  return "en";
}

/** Match the locale's grammatical category; languages such as French also use “one” for zero. */
export function pluralMessageId(language: Language, count: number,
  forms: Partial<Record<Intl.LDMLPluralRule, MessageId>> & { other: MessageId }): MessageId {
  return forms[new Intl.PluralRules(language).select(count)] ?? forms.other;
}

export function messageTemplate(language: Language, id: MessageId): string {
  return catalogs[language][id] ?? en[id];
}

export function translator(language: Language): Translate {
  return (id, values = {}) => messageTemplate(language, id).replace(/\{(\w+)\}/g,
    (placeholder, name: string) => String(values[name] ?? placeholder));
}

export const englishTranslator = translator("en");

// Native diagnostics keep their original messages. UI recovery guidance uses stable error codes,
// so arbitrary OS/driver strings are never used as translation keys.
const errorMessages: Record<string, [MessageId, MessageId?]> = {
  invalid_identifier: ["error.configuration", "error.setup"],
  invalid_configuration: ["error.configuration", "error.setup"],
  invalid_state: ["error.state"],
  command_queue_full: ["error.busy"],
  device_not_found: ["error.device", "error.reconnect"],
  device_disconnected: ["error.device", "error.reconnect"],
  unsupported_format: ["error.format", "error.setup"],
  audio_io: ["error.audio", "error.reconnect"],
  dsp: ["error.audio", "error.restart"],
  denoise: ["error.audio", "error.restart"],
  model_asset_missing: ["error.model", "error.reinstall"],
  model_hash_mismatch: ["error.model", "error.reinstall"],
  model_load_failed: ["error.model", "error.reinstall"],
  invalid_model_frame: ["error.audio", "error.restart"],
  non_finite_audio: ["error.audio", "error.restart"],
  stream_error: ["error.audio", "error.reconnect"],
  routing: ["error.routing", "error.setup"],
  worker_terminated: ["error.engine", "error.restart"],
  shutdown_in_progress: ["error.shutdown"],
  internal: ["error.generic", "error.restart"],
  desktop_bridge_unavailable: ["error.bridge", "error.restart"],
};

export function localizedError(error: ApiError, language: Language): ApiError {
  if (language === "en") return error;
  const t = translator(language);
  const [message, action] = errorMessages[error.code] ?? ["error.generic", "error.restart"];
  return { ...error, message: t(message), suggestedAction: action ? t(action) : null };
}
