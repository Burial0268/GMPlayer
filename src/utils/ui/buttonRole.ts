import type { Directive } from "vue";

/**
 * 给带 `@click` 的 div/span 补上按钮的键盘语义：可聚焦，回车 / 空格触发它自己的 click。
 * 只响应元素自身的按键，里面嵌的输入框、按钮保持原样。
 */
export const vButton: Directive<HTMLElement> = {
  mounted(el) {
    if (!el.hasAttribute("role")) el.setAttribute("role", "button");
    if (!el.hasAttribute("tabindex")) el.tabIndex = 0;
    el.addEventListener("keydown", (event) => {
      if (event.target !== el || (event.key !== "Enter" && event.key !== " ")) return;
      event.preventDefault();
      el.click();
    });
  },
};
