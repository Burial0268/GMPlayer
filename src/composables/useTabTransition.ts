import { ref } from "vue";

export function useTabTransition(tabNames: string[]) {
  const transitionName = ref<"slide-left" | "slide-right">("slide-right");
  let currentIndex = 0;

  function updateDirection(newTabName: string) {
    const newIndex = tabNames.indexOf(newTabName);
    if (newIndex < 0 || newIndex === currentIndex) return;
    transitionName.value = newIndex > currentIndex ? "slide-left" : "slide-right";
    currentIndex = newIndex;
  }

  function syncIndex(tabName: string) {
    updateDirection(tabName);
  }

  return { transitionName, updateDirection, syncIndex };
}
