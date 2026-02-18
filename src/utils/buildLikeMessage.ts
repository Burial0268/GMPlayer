export const buildLikeMessage = (
  t: (key: string, params?: Record<string, string>) => string,
  entityName: string,
  type: number,
  result: "success" | "failed",
  language: string
) => {
  const space = language === "zh-CN" ? "" : " ";
  const resultKey = result === "success" ? "general.dialog.success" : "general.dialog.failed";
  const actionKey = type === 1 ? "menu.collection" : "menu.cancelCollection";
  return `${entityName}${space}${t(actionKey, { name: t(resultKey) })}`;
};
