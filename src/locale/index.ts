import type { App } from "vue";
import { createI18n } from "vue-i18n";
import { settingStore } from "@/store";

// 引入语言文件
import en from "./lang/en";
import zhCN from "./lang/zh-CN";

const messages = {
  en,
  "zh-CN": zhCN,
} as Record<string, Record<string, unknown>>;

// 注册 i8n 实例
export const useI18n = (app: App) => {
  const setting = settingStore();
  const i18n = createI18n({
    legacy: false,
    globalInjection: true,
    locale: setting.language,
    fallbackLocale: "zh-CN",
    messages,
  });
  app.use(i18n);
  return i18n;
};
