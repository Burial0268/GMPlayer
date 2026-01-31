/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<{}, {}, any>
  export default component
}

// Pinia persist plugin type augmentation
import 'pinia'

declare module 'pinia' {
  export interface DefineStoreOptionsBase<S, Store> {
    persist?: import('pinia-plugin-persistedstate').PersistedStateOptions | import('pinia-plugin-persistedstate').PersistedStateOptions[] | boolean
  }
}
