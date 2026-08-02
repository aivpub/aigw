import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import en from "./locales/en.json";
import zhCN from "./locales/zh-CN.json";

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      "zh-CN": { translation: zhCN },
    },
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

// Sync <html lang> attribute on language change
i18n.on("languageChanged", (lng) => {
  document.documentElement.lang = lng;
});

export default i18n;
