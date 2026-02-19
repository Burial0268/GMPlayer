import { ref } from "vue";

export function useTabTransition(tabNames: string[]) {
  const transitionName = ref<"slide-left" | "slide-right">("slide-right");
  let currentIndex = 0;

  function updateDirection(newTabName: string) {
    const newIndex = tabNames.indexOf(newTabName);
    if (newIndex < 0) return;
    transitionName.value =
      newIndex > currentIndex ? "slide-left" : "slide-right";
    currentIndex = newIndex;
  }

  function syncIndex(tabName: string) {
    const idx = tabNames.indexOf(tabName);
    if (idx >= 0) currentIndex = idx;
  }

  return { transitionName, updateDirection, syncIndex };
}
