import { ref, onMounted, onBeforeUnmount, nextTick } from "vue";

export function useResponsiveLayout(breakpoint = 768) {
  const isMobile = ref(false);

  const update = () => {
    isMobile.value = window.innerWidth <= breakpoint;
  };

  onMounted(() => {
    update();
    window.addEventListener("resize", update);
  });

  onBeforeUnmount(() => {
    window.removeEventListener("resize", update);
  });

  return { isMobile, updateDeviceStatus: update };
}
