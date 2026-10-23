/**
 * 是否开启了系统的「减弱动态效果」。
 *
 * CSS 侧由 `src/style/global.scss` 的全局 `@media (prefers-reduced-motion: reduce)`
 * 统一兜底，但 GSAP / motion-v 这类脚本驱动的动画浏览器不会替我们收敛，
 * 必须在调用处显式查询后跳过或改为瞬时。
 *
 * 每次读取而不是缓存：用户可能在应用运行期间改系统设置。
 */
export const prefersReducedMotion = (): boolean =>
  typeof window !== "undefined" &&
  (window.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches ?? false);
