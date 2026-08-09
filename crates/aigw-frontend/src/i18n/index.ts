import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";

/**
 * i18n bootstrap.
 *
 * TD-008a (lazy-load): the DEFAULT language bundle (en) is imported
 * synchronously here so the first paint (login page, 401 redirect) is never
 * blocked on an async chunk. The non-default language (zh-CN) is loaded on
 * demand via `import()` + `addResourceBundle` when the user switches, so Vite
 * emits it as a separate lazy chunk instead of shipping both locales in the
 * initial bundle.
 */
import en from "./locales/en.json";

const resources = {
  en: { translation: en },
} as const;

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources,
    fallbackLng: "en",
    defaultNS: "translation",
    detection: {
      order: ["localStorage", "navigator"],
      lookupLocalStorage: "aigw-language",
      caches: ["localStorage"],
    },
    interpolation: {
      escapeValue: false,
    },
    returnObjects: true,
  });

// TD-008a: eagerly load the DETECTED language's bundle so first paint matches the
// browser locale. en is bundled synchronously; a zh-CN first visit triggers the
// lazy import before the app renders its initial tree.
// Normalize the detected language to the two known bundles: navigator.language
// can be "en-US"/"en-GB" (English) or "zh" / "zh-Hans-CN" — only "en" and
// "zh-CN" bundles exist, so map anything that isn't an exact zh-CN to the eager
// en bundle (fallback) and only lazy-import zh-CN.
const detected = i18n.language || "en";
if (detected === "zh-CN" || detected === "zh") {
  const lng = "zh-CN";
  if (!i18n.hasResourceBundle(lng, "translation")) {
    import("./locales/zh-CN.json").then((mod) => {
      const data = (mod as { default: typeof en }).default;
      i18n.addResourceBundle(lng, "translation", data, true, true);
    });
  }
}

// Intercept language changes: when the user switches to a language whose bundle
// is not yet loaded, dynamically import() it and add it to i18next's resources.
// (TD-008a: zh-CN lives in its own lazy chunk, fetched only on first switch.)
i18n.on("languageChanged", (lng) => {
  document.documentElement.lang = lng;
  if (lng === "zh-CN" && !i18n.hasResourceBundle(lng, "translation")) {
    import("./locales/zh-CN.json").then((mod) => {
      const data = (mod as { default: typeof en }).default;
      i18n.addResourceBundle(lng, "translation", data, true, true);
    });
  }
});

export default i18n;
