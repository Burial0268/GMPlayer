<template>
  <n-dropdown
    v-if="!asSheet && manual"
    v-bind="$attrs"
    trigger="manual"
    :options="options"
    :show="open"
    @select="onSelect"
  />
  <n-dropdown v-else-if="!asSheet" v-bind="$attrs" :options="options" @select="onSelect">
    <slot />
  </n-dropdown>
  <template v-else>
    <!-- display: contents keeps the trigger where the layout put it; clicks still bubble here. -->
    <span v-if="!manual" class="action-menu-trigger" @click="open = true"><slot /></span>
    <GlassSheet v-model:show="open" :label="label ?? $t('player.controls.more')">
      <template v-if="$slots.header" #header><slot name="header" /></template>
      <section v-for="(group, index) in groups" :key="index" class="sheet-group">
        <button
          v-for="option in group"
          :key="option.key"
          type="button"
          class="sheet-row"
          :disabled="option.disabled"
          @click="run(option)"
        >
          <span v-if="option.icon" class="row-icon action-menu-icon">
            <component :is="option.icon" />
          </span>
          <span class="row-label text-hidden">{{ option.label }}</span>
        </button>
      </section>
    </GlassSheet>
  </template>
</template>

<script setup lang="ts">
/**
 * A "more" menu that is a dropdown on desktop and a GlassSheet tray at mobile
 * width, like the song list's action sheet and the big player's "more".
 *
 * Takes naive's `DropdownOption`s as they are written for `n-dropdown` —
 * `show`, `disabled`, `icon`, `props.onClick` and `divider`s (which split the
 * sheet into groups) — so a call site only swaps the tag. Other attributes
 * (`placement`, `trigger`, …) go to the dropdown.
 *
 * `manual` is the right-click form: no trigger slot, the caller owns `v-model:show`
 * and passes `x`/`y`/`on-clickoutside` for the dropdown. `sheet` picks the form per
 * opening (a long press asks for the tray, see `isTouchContextMenu`); left unset, the
 * mobile width decides.
 *
 * Submenus (`children`) become one tray section each, so a nested right-click menu
 * offers everything on a phone too; a grandchild list (a submenu inside a submenu)
 * is spelled out as rows prefixed with its parent's label.
 */
import { computed } from "vue";
import type { DropdownOption } from "naive-ui";
import GlassSheet from "@/components/Common/GlassSheet.vue";
import { useResponsiveLayout } from "@/composables/useResponsiveLayout";

defineOptions({ inheritAttrs: false });
const props = withDefaults(
  defineProps<{
    options: DropdownOption[] | null;
    label?: string;
    manual?: boolean;
    /** Tray (true) or dropdown (false) for this opening; unset follows the mobile width. */
    sheet?: boolean;
  }>(),
  { label: undefined, manual: false, sheet: undefined },
);
const emit = defineEmits<{ select: [key: string | number, option: DropdownOption] }>();

// GlassSheet's own bottom-sheet breakpoint (PLAYLIST_SHEET_MEDIA_QUERY, 768px).
const { isMobile } = useResponsiveLayout(768);
// Local when the caller does not bind it (the trigger form), the caller's in manual form.
const open = defineModel<boolean>("show", { default: false });

const asSheet = computed(() => props.sheet ?? isMobile.value);

const visible = (options: DropdownOption[] | undefined) =>
  (options ?? []).filter((option) => option.show !== false);

// Rows of one submenu; a nested submenu is spelled out under its own label.
const rowsOf = (options: DropdownOption[] | undefined): DropdownOption[] =>
  visible(options).flatMap((option) =>
    option.children
      ? visible(option.children as DropdownOption[]).map((child) => ({
          ...child,
          icon: child.icon ?? option.icon,
          label: `${option.label} · ${child.label}`,
        }))
      : [option],
  );

const groups = computed(() => {
  const result: DropdownOption[][] = [[]];
  for (const option of visible(props.options ?? [])) {
    if (option.type === "divider") result.push([]);
    else if (option.children) result.push(rowsOf(option.children as DropdownOption[]), []);
    else result[result.length - 1].push(option);
  }
  return result.filter((group) => group.length);
});

const onSelect = (key: string | number, option: DropdownOption) => emit("select", key, option);

// Close first, then act, so the closing animation starts in the same frame as a navigation.
const run = (option: DropdownOption) => {
  open.value = false;
  (option.props as { onClick?: () => void } | undefined)?.onClick?.();
  emit("select", option.key as string | number, option);
};
</script>

<style lang="scss" scoped>
.action-menu-trigger {
  display: contents;
}

.action-menu-icon {
  display: flex;
  font-size: 20px;
}
</style>
