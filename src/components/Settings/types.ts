/**
 * 统一设置面板的声明式 schema 类型
 */
export type SettingsControl = "switch" | "select" | "slider" | "number" | "button" | "custom";

export interface SettingsOption {
  /** i18n key 或原始文本 */
  label: string;
  value: string | number | boolean;
  disabled?: boolean;
}

export interface SettingsItem {
  /** settingStore 字段名；对 button/custom 为唯一 id */
  key: string;
  /** i18n key 或原始文本 */
  label: string;
  /** i18n key 或原始文本 */
  tip?: string;
  control: SettingsControl;
  /** select 选项，可为函数以延迟求值 */
  options?: SettingsOption[] | (() => SettingsOption[]);
  min?: number;
  max?: number;
  step?: number;
  marks?: Record<number, string>;
  /** button 文案（i18n key 或原始文本） */
  buttonText?: string;
  buttonType?: "default" | "primary" | "error" | "warning";
  /** 写入 settingStore 后触发的副作用 */
  onUpdate?: (value: unknown) => void;
  /** control:"custom" 时，SettingsPanel 渲染的具名插槽 */
  slot?: string;
  /** 在标签旁显示 DEV 标记 */
  dev?: boolean;
  /** 可见性判定（默认可见） */
  show?: () => boolean;
  /** 禁用判定（默认不禁用） */
  disabled?: () => boolean;
}

export interface SettingsSection {
  key: string;
  /** i18n key 或原始文本 */
  label: string;
  /** 用于搜索的补充关键字（i18n key 或原始文本） */
  searchText?: string;
  items: SettingsItem[];
}
