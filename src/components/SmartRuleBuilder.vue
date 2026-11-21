<script setup lang="ts">
import { computed } from 'vue'
import { Plus, Trash2, RefreshCw, Save } from '@lucide/vue'

export interface SmartRuleSimple {
  field: string
  operator: string
  value: string | number
}

export type SmartRule = SmartRuleSimple

export interface SmartRuleGroup {
  operator: 'and' | 'or'
  rules: SmartRule[]
}

const props = defineProps<{
  modelValue: SmartRuleGroup
  lastUpdated?: string | null
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: SmartRuleGroup): void
  (e: 'save'): void
  (e: 'refresh'): void
}>()

const fields = [
  { value: 'title', label: 'Title' },
  { value: 'artist', label: 'Artist' },
  { value: 'album', label: 'Album' },
  { value: 'album_artist', label: 'Album Artist' },
  { value: 'genre', label: 'Genre' },
  { value: 'year', label: 'Year' },
  { value: 'bpm', label: 'BPM' },
  { value: 'musical_key', label: 'Key' },
  { value: 'energy', label: 'Energy' },
  { value: 'duration', label: 'Duration (sec)' },
  { value: 'play_count', label: 'Plays' },
]

const operators = [
  { value: 'contains', label: 'contains' },
  { value: 'not_contains', label: 'does not contain' },
  { value: 'is', label: 'is' },
  { value: 'is_not', label: 'is not' },
  { value: 'starts_with', label: 'starts with' },
  { value: 'ends_with', label: 'ends with' },
  { value: 'greater_than', label: 'greater than' },
  { value: 'less_than', label: 'less than' },
]

const currentRules = computed(() => props.modelValue.rules || [])

function addRule() {
  const updated = {
    ...props.modelValue,
    rules: [
      ...currentRules.value,
      { field: 'artist', operator: 'contains', value: '' },
    ],
  }
  emit('update:modelValue', updated)
}

function removeRule(index: number) {
  const updatedRules = [...currentRules.value]
  updatedRules.splice(index, 1)
  emit('update:modelValue', { ...props.modelValue, rules: updatedRules })
}

function updateRule(index: number, key: keyof SmartRuleSimple, val: any) {
  const updatedRules = [...currentRules.value]
  updatedRules[index] = { ...updatedRules[index], [key]: val }
  emit('update:modelValue', { ...props.modelValue, rules: updatedRules })
}

function toggleOperator() {
  const next = props.modelValue.operator === 'and' ? 'or' : 'and'
  emit('update:modelValue', { ...props.modelValue, operator: next })
}
</script>

<template>
  <div class="rounded-xl border border-white/10 bg-white/[0.03] p-4 flex flex-col gap-3">
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-2">
        <span class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Match</span>
        <button
          type="button"
          class="px-2 py-0.5 rounded text-xs font-bold uppercase tracking-wider transition-colors bg-primary/20 text-primary hover:bg-primary/30"
          @click="toggleOperator"
        >
          {{ modelValue.operator === 'and' ? 'ALL (AND)' : 'ANY (OR)' }}
        </button>
        <span class="text-xs text-muted-foreground">rules</span>
      </div>
      <div class="flex items-center gap-2">
        <button
          type="button"
          class="flex items-center gap-1.5 px-3 py-1 rounded-md text-xs font-medium bg-white/5 hover:bg-white/10 text-foreground transition-colors"
          @click="emit('refresh')"
        >
          <RefreshCw class="w-3.5 h-3.5" />
          <span>Refresh</span>
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 px-3 py-1 rounded-md text-xs font-medium bg-primary text-primary-foreground hover:bg-primary/90 transition-colors shadow"
          @click="emit('save')"
        >
          <Save class="w-3.5 h-3.5" />
          <span>Save</span>
        </button>
      </div>
    </div>

    <!-- Rule rows -->
    <div class="flex flex-col gap-2">
      <div
        v-for="(rule, idx) in currentRules"
        :key="idx"
        class="flex items-center gap-2 flex-wrap"
      >
        <select
          :value="rule.field"
          class="bg-white/5 border border-white/10 rounded-md px-2 py-1 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          @change="updateRule(idx, 'field', ($event.target as HTMLSelectElement).value)"
        >
          <option v-for="f in fields" :key="f.value" :value="f.value" class="bg-card text-foreground">
            {{ f.label }}
          </option>
        </select>

        <select
          :value="rule.operator"
          class="bg-white/5 border border-white/10 rounded-md px-2 py-1 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          @change="updateRule(idx, 'operator', ($event.target as HTMLSelectElement).value)"
        >
          <option v-for="o in operators" :key="o.value" :value="o.value" class="bg-card text-foreground">
            {{ o.label }}
          </option>
        </select>

        <input
          type="text"
          :value="rule.value"
          placeholder="Value..."
          class="flex-1 min-w-[120px] bg-white/5 border border-white/10 rounded-md px-2 py-1 text-xs text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          @input="updateRule(idx, 'value', ($event.target as HTMLInputElement).value)"
        />

        <button
          type="button"
          class="p-1.5 text-muted-foreground hover:text-destructive transition-colors rounded hover:bg-white/5"
          title="Remove rule"
          @click="removeRule(idx)"
        >
          <Trash2 class="w-3.5 h-3.5" />
        </button>
      </div>
    </div>

    <div>
      <button
        type="button"
        class="flex items-center gap-1 text-xs text-primary hover:underline font-medium"
        @click="addRule"
      >
        <Plus class="w-3.5 h-3.5" />
        <span>Add rule</span>
      </button>
    </div>
  </div>
</template>
