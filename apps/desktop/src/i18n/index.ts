import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { resources } from "./resources";

export const LANGUAGE_STORAGE_KEY = "evetools.language";

export const supportedLanguages = [
  { code: "zh-CN", labelKey: "language.zhCN" },
  { code: "en-US", labelKey: "language.enUS" }
] as const;

export type SupportedLanguage = (typeof supportedLanguages)[number]["code"];

function isSupportedLanguage(value: string | null | undefined): value is SupportedLanguage {
  return value === "zh-CN" || value === "en-US";
}

export function resolveSupportedLanguage(language: string | null | undefined): SupportedLanguage {
  if (!language) {
    return "zh-CN";
  }

  if (isSupportedLanguage(language)) {
    return language;
  }

  const normalized = language.toLowerCase();
  if (normalized.startsWith("zh")) {
    return "zh-CN";
  }
  if (normalized.startsWith("en")) {
    return "en-US";
  }
  return "zh-CN";
}

function readStoredLanguage(): SupportedLanguage | null {
  if (typeof window === "undefined") {
    return null;
  }

  const storedLanguage = window.localStorage.getItem(LANGUAGE_STORAGE_KEY);
  return isSupportedLanguage(storedLanguage) ? storedLanguage : null;
}

function detectInitialLanguage(): SupportedLanguage {
  const storedLanguage = readStoredLanguage();
  if (storedLanguage) {
    return storedLanguage;
  }

  if (typeof navigator !== "undefined") {
    return resolveSupportedLanguage(navigator.language);
  }

  return "zh-CN";
}

void i18n.use(initReactI18next).init({
  resources,
  lng: detectInitialLanguage(),
  fallbackLng: "zh-CN",
  interpolation: {
    escapeValue: false
  },
  returnNull: false
});

i18n.on("languageChanged", (language) => {
  if (typeof window === "undefined") {
    return;
  }

  window.localStorage.setItem(LANGUAGE_STORAGE_KEY, resolveSupportedLanguage(language));
});

export function translateCode(prefix: string, code: string, translate: (key: string) => string): string {
  const key = `${prefix}.${code}`;
  const translated = translate(key);
  return translated === key ? code : translated;
}

export default i18n;
