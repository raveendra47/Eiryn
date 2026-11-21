<script setup lang="ts">
import { ref, onErrorCaptured } from 'vue'

const props = withDefaults(defineProps<{
  fallback?: string
}>(), {
  fallback: 'Something went wrong displaying this section.',
})

const hasError = ref(false)
const error = ref<Error | null>(null)
const errorInfo = ref<string | null>(null)

onErrorCaptured((err, _instance, info) => {
  error.value = err as Error
  errorInfo.value = info
  hasError.value = true
  console.error('[ErrorBoundary]', err, info)
  return false
})

function retry() {
  hasError.value = false
  error.value = null
  errorInfo.value = null
}
</script>

<template>
  <div v-if="hasError" class="error-boundary">
    <div class="error-boundary-content">
      <svg class="error-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="12" cy="12" r="10" />
        <line x1="12" y1="8" x2="12" y2="12" />
        <line x1="12" y1="16" x2="12.01" y2="16" />
      </svg>
      <p class="error-message">{{ props.fallback }}</p>
      <details v-if="error" class="error-details">
        <summary>Technical details</summary>
        <pre>{{ error.message }}{{ errorInfo ? '\nPhase: ' + errorInfo : '' }}</pre>
      </details>
      <button class="retry-btn" @click="retry">Try Again</button>
    </div>
  </div>
  <slot v-else />
</template>

<style scoped>
.error-boundary {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 120px;
  padding: 24px;
}
.error-boundary-content {
  text-align: center;
  max-width: 400px;
}
.error-icon {
  width: 32px;
  height: 32px;
  margin: 0 auto 12px;
  opacity: 0.5;
}
.error-message {
  color: var(--text-secondary, #999);
  font-size: 14px;
  margin: 0 0 12px;
}
.error-details {
  margin-bottom: 12px;
  text-align: left;
}
.error-details summary {
  cursor: pointer;
  color: var(--text-tertiary, #777);
  font-size: 12px;
}
.error-details pre {
  margin: 8px 0 0;
  padding: 8px;
  background: var(--surface-elevated, #1a1a2e);
  border-radius: 6px;
  font-size: 11px;
  color: var(--text-secondary, #999);
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 200px;
  overflow: auto;
}
.retry-btn {
  padding: 6px 16px;
  border: 1px solid var(--border, #333);
  border-radius: 6px;
  background: transparent;
  color: var(--text-primary, #fff);
  cursor: pointer;
  font-size: 13px;
}
.retry-btn:hover {
  background: var(--surface-hover, #2a2a3e);
}
</style>
