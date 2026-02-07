/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  // eslint-disable-next-line @typescript-eslint/no-empty-object-type
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
