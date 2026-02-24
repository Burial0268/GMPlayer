import { NIcon } from "naive-ui";
import { h, type VNode } from "vue";

export const renderIcon = (icon: VNode) => {
  return () => h(NIcon, { style: { transform: "translateX(2px)" } }, { default: () => icon });
};
