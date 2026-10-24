import { isTauri, invoke } from "@tauri-apps/api/core";
import { createContext, Fragment, useContext, useEffect, useMemo, useState, type Context, type ReactNode } from "react";
import {
  englishTranslator, isLanguagePreference, languages, languageOptions, LANGUAGE_STORAGE_KEY, localizedError, messageTemplate,
  resolveLanguage, translator, type Language, type LanguagePreference, type MessageId,
} from "./translations";
import type { ApiError } from "./api";

function systemLanguages() {
  return typeof navigator === "undefined" ? [] : navigator.languages.length ? navigator.languages : [navigator.language];
}

function savedPreference(): LanguagePreference {
  try {
    const saved = localStorage.getItem(LANGUAGE_STORAGE_KEY);
    return isLanguagePreference(saved) ? saved : "system";
  } catch { return "system"; }
}

const fallbackI18n = {
  language: "en" as Language,
  preference: "system" as LanguagePreference,
  setPreference: (_preference: LanguagePreference) => {},
  t: englishTranslator,
  error: (error: ApiError) => error,
  saveError: false,
  menuError: false,
};

// Catalog edits can reload this module while mounted consumers survive Fast Refresh.
const I18nContext: Context<typeof fallbackI18n> = import.meta.hot?.data?.i18nContext ?? createContext(fallbackI18n);
if (import.meta.hot?.data) import.meta.hot.data.i18nContext = I18nContext;

export function I18nProvider({ children }: { children: ReactNode }) {
  const [preference, setPreferenceState] = useState(savedPreference);
  const [languages, setLanguages] = useState(systemLanguages);
  const [saveError, setSaveError] = useState(false);
  const [menuError, setMenuError] = useState(false);
  const language = resolveLanguage(preference, languages);

  useEffect(() => {
    const update = () => setLanguages(systemLanguages());
    const sync = (event: StorageEvent) => {
      if (event.key === LANGUAGE_STORAGE_KEY || event.key === null) setPreferenceState(savedPreference());
    };
    window.addEventListener("languagechange", update);
    window.addEventListener("storage", sync);
    return () => { window.removeEventListener("languagechange", update); window.removeEventListener("storage", sync); };
  }, []);

  useEffect(() => {
    document.documentElement.lang = language;
    if (!isTauri()) return;
    let disposed = false;
    setMenuError(false);
    void invoke("set_app_language", { language }).catch(() => { if (!disposed) setMenuError(true); });
    return () => { disposed = true; };
  }, [language]);

  const value = useMemo(() => ({
    language, preference, t: translator(language), saveError, menuError,
    error: (error: ApiError) => localizedError(error, language),
    setPreference(next: LanguagePreference) {
      setPreferenceState(next);
      try { localStorage.setItem(LANGUAGE_STORAGE_KEY, next); setSaveError(false); }
      catch { setSaveError(true); }
    },
  }), [language, preference, saveError, menuError]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n() { return useContext(I18nContext); }

/** Interpolate React nodes inside complete translated sentences without HTML injection. */
export function Message({ id, values }: { id: MessageId; values: Record<string, ReactNode> }) {
  const { language } = useI18n();
  return messageTemplate(language, id).split(/(\{\w+\})/g).map((part, index) =>
    <Fragment key={index}>{part.startsWith("{") ? values[part.slice(1, -1)] ?? part : part}</Fragment>);
}

export function LanguageSelector() {
  const { t, language, preference, setPreference, saveError, menuError } = useI18n();
  const languageName = languages[language].name;
  const feedback = saveError || menuError;
  return <div className="language-setting">
    <label htmlFor="app-language" className="sr-only">{t("language.label")}</label>
    <div className="language-control">
      <span className="language-display" aria-hidden="true">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a18 18 0 0 1 0 18 18 18 0 0 1 0-18Z" />
        </svg>
        <span lang={language}>{languages[language].shortName}</span>
        <svg className="language-chevron" width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d="m3 4.5 3 3 3-3" /></svg>
      </span>
      <select id="app-language" value={preference} aria-describedby={`app-language-current${feedback ? " app-language-feedback" : ""}`} onChange={(event) => {
        if (isLanguagePreference(event.target.value)) setPreference(event.target.value);
      }}>
        <option value="system">{t("language.system")}</option>
        {languageOptions.map(({ id, name }) => <option key={id} value={id} lang={id}>{name}</option>)}
      </select>
    </div>
    <span id="app-language-current" className="sr-only">{t("language.current", { language: languageName })}</span>
    {feedback ? <p id="app-language-feedback" className="language-feedback" role="status">{t(saveError ? "language.saveError" : "language.menuError")}</p> : null}
  </div>;
}

/** Keep actionable native diagnostics available when a translated summary is necessarily broader. */
export function ErrorDetails({ error }: { error: ApiError }) {
  const { t, language } = useI18n();
  if (language === "en") return null;
  return <details className="error-details"><summary>{t("error.details")}</summary>
    <p lang="en">{error.message}</p>
    {error.suggestedAction ? <p lang="en">{error.suggestedAction}</p> : null}
  </details>;
}
