<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { SkipForward } from '@lucide/vue'

const emit = defineEmits<{
  close: [dontShowAgain: boolean]
}>()

const videoEl = ref<HTMLVideoElement | null>(null)
const show = ref(true)
const dontShowAgain = ref(false)

function onEnded() {
  setTimeout(() => emit('close', dontShowAgain.value), 500)
}

function skip() {
  if (videoEl.value) {
    videoEl.value.pause()
    videoEl.value.currentTime = videoEl.value.duration
  }
  emit('close', dontShowAgain.value)
}

onMounted(() => {
  if (videoEl.value) {
    videoEl.value.play().catch(() => {})
  }
})
</script>

<template>
  <Transition name="splash-fade">
    <div v-if="show" class="fixed inset-0 z-[9999] flex flex-col items-center justify-center bg-black select-none overflow-hidden">
      <video
        ref="videoEl"
        src="/eiryn-intro.mp4"
        autoplay
        muted
        playsinline
        class="max-h-full max-w-full object-contain"
        @ended="onEnded"
      />

      <div class="absolute bottom-8 left-0 right-0 flex items-center justify-between px-8">
        <label class="flex items-center gap-2 cursor-pointer text-white/40 hover:text-white/70 transition-colors text-sm">
          <input type="checkbox" v-model="dontShowAgain"
            class="w-4 h-4 rounded border-white/20 bg-white/5 accent-primary" />
          Don't show again
        </label>

        <button @click="skip"
          class="flex items-center gap-1.5 text-white/40 hover:text-white transition-colors text-sm">
          Skip <SkipForward class="w-3.5 h-3.5" />
        </button>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.splash-fade-enter-active,
.splash-fade-leave-active {
  transition: opacity 0.6s ease;
}
.splash-fade-enter-from,
.splash-fade-leave-to {
  opacity: 0;
}
</style>
