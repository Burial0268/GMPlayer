import { ref } from "vue";

// App.vue 的 BackTop 是否正显示。同列的「定位到正在播放」据此决定叠在它上面还是落进它的位置。
export const backTopShown = ref(false);
