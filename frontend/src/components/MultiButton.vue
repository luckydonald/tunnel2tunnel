<script setup lang="ts">
// Thin wrapper only — slot in plain buttons with their own click handlers.
// This just zeroes out the border/radius/spacing on the inner-facing sides
// so a row of buttons reads as one attached segmented control.
</script>

<template>
  <div class="multi-button">
    <slot />
  </div>
</template>

<style lang="scss" scoped>
.multi-button {
  display: inline-flex;

  :deep(> *) {
    &:not(:first-child) {
      margin-left: -1px;
      border-top-left-radius: 0;
      border-bottom-left-radius: 0;
    }
    &:not(:last-child) {
      border-top-right-radius: 0;
      border-bottom-right-radius: 0;
    }
  }
}
</style>
