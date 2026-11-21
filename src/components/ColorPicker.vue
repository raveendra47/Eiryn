<script setup lang="ts">
import { ref, watch } from 'vue'
import { normalizeHexColor } from '../lib/color'

const props = defineProps<{
  modelValue: string | null
  presets: readonly string[]
  ariaLabel: string
  hexLabel: string
}>()

const emit = defineEmits<{
  (e: 'update:model-value', value: string | null): void
}>()

const hexInput = ref(props.modelValue ?? '')

watch(() => props.modelValue, (v) => { hexInput.value = v ?? '' })

function pick(color: string) {
  const normalized = normalizeHexColor(color)
  if (normalized) emit('update:model-value', normalized)
}

function commitHex() {
  const normalized = normalizeHexColor(hexInput.value)
  if (normalized) {
    emit('update:model-value', normalized)
  } else {
    hexInput.value = props.modelValue ?? ''
  }
}
</script>

<template>
  <div class="flex items-center gap-2">
    <label
      class="relative flex h-7 w-7 items-center justify-center rounded-full border-2 border-dashed border-foreground/30 transition-all hover:scale-110 cursor-pointer overflow-hidden"
      :aria-label="ariaLabel"
      title="Custom color">
      <span class="text-sm leading-none text-foreground/70">+</span>
      <input
        type="color"
        class="absolute inset-0 opacity-0 cursor-pointer"
        :value="modelValue ?? '#E11D48'"
        @input="pick(($event.target as HTMLInputElement).value)" />
    </label>
    <input
      v-model="hexInput"
      :placeholder="hexLabel"
      spellcheck="false"
      maxlength="7"
      @change="commitHex"
      @keyup.enter="commitHex"
      class="w-20 px-2 py-1 rounded-lg text-xs font-mono uppercase bg-foreground/[0.04] border border-foreground/10 text-foreground/80 focus:outline-none focus:border-foreground/30" />
  </div>
</template>
