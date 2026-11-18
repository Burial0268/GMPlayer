// 应用元信息。构建期由 vite.config.ts 的 define 注入，避免把整份
// package.json（含全部 dependencies / devDependencies 版本表）打进产物。
declare const __APP_INFO__: {
  version: string;
  author: string;
  home: string;
  github: string;
};

export const appInfo = __APP_INFO__;

export default appInfo;
