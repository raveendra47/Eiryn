<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useAppStore } from '../../stores/app'
import { ImagePlay, MicVocal, Music2, Check, Loader } from '@lucide/vue'

const appStore = useAppStore()

const apiKey = ref('')
const apiSecret = ref('')
const apiKeyTesting = ref(false)
const apiKeyResult = ref('')

onMounted(() => {
  apiKey.value = appStore.lastfmApiKey
  apiSecret.value = appStore.lastfmApiSecret
})

async function handleSaveApiKey() {
  apiKeyResult.value = ''
  await appStore.updateLastfmApiKey(apiKey.value, apiSecret.value)
  handleTestKey()
}

async function handleTestKey() {
  apiKeyTesting.value = true
  apiKeyResult.value = ''
  try {
    const { validateLastfmApiKey } = await import('../../lib/invoke')
    const result = await validateLastfmApiKey(apiKey.value)
    apiKeyResult.value = result.message
  } catch (e: any) {
    apiKeyResult.value = e?.toString() || 'Test failed'
  } finally {
    apiKeyTesting.value = false
  }
}
</script>

<template>
  <div class="space-y-10 animate-in fade-in slide-in-from-bottom-2 duration-500">
    <section>
      <div class="flex items-center gap-2 mb-6 text-foreground opacity-60 select-none">
        <Music2 class="w-4 h-4" />
        <h2 class="text-sm font-bold uppercase tracking-wider">Last.fm</h2>
      </div>
      <div class="bg-card rounded-2xl border border-foreground/[0.06] divide-y divide-foreground/[0.06]">
        <div class="p-5 space-y-4">
          <div>
            <p class="text-sm font-semibold">API Key</p>
            <p class="text-xs text-foreground opacity-60 mt-1">Get yours at <a href="https://www.last.fm/api/account/create" target="_blank" class="text-primary hover:underline">last.fm/api/account/create</a></p>
          </div>
          <div class="flex gap-2">
            <input v-model="apiKey" placeholder="API Key"
              class="flex-1 px-3 py-2 text-sm rounded-lg bg-foreground/[0.07] border border-foreground/20 text-foreground placeholder:text-foreground/40 outline-none focus:border-primary/50 transition-colors font-mono text-xs" />
            <input v-model="apiSecret" placeholder="Shared Secret"
              class="flex-1 px-3 py-2 text-sm rounded-lg bg-foreground/[0.07] border border-foreground/20 text-foreground placeholder:text-foreground/40 outline-none focus:border-primary/50 transition-colors font-mono text-xs" />
          </div>
          <div class="flex items-center gap-2">
            <button @click="handleSaveApiKey" :disabled="apiKeyTesting"
              class="flex items-center gap-1.5 px-4 py-2 text-sm font-medium bg-primary text-primary-foreground rounded-lg hover:opacity-90 transition-opacity disabled:opacity-50">
              <Loader v-if="apiKeyTesting" class="w-4 h-4 animate-spin" />
              <Check v-else class="w-4 h-4" />
              Save & Test
            </button>
            <button @click="handleTestKey" :disabled="apiKeyTesting || !apiKey"
              class="px-3 py-2 text-sm font-medium bg-foreground/[0.07] text-foreground rounded-lg hover:bg-foreground/[0.12] transition-all disabled:opacity-40">
              Test Only
            </button>
            <span v-if="apiKeyResult" class="text-xs font-medium" :class="apiKeyResult.includes('✓') ? 'text-success' : 'text-error'">
              {{ apiKeyResult }}
            </span>
          </div>
        </div>
      </div>
    </section>

    <section>
      <div class="flex items-center gap-2 mb-6 text-foreground opacity-60 select-none">
        <ImagePlay class="w-4 h-4" />
        <h2 class="text-sm font-bold uppercase tracking-wider">Artwork</h2>
      </div>
      <div class="bg-card rounded-2xl border border-foreground/[0.06] p-5 flex items-center justify-between">
        <div>
          <p class="text-sm font-semibold">Online Artist Artwork</p>
          <p class="text-xs text-foreground opacity-60 mt-1">Fetch missing artist images from online sources</p>
        </div>
        <button @click="appStore.updateUseOnlineArtistArtwork(!appStore.useOnlineArtistArtwork)"
          :class="['w-11 h-6 rounded-full transition-colors relative', appStore.useOnlineArtistArtwork ? 'bg-primary' : 'bg-foreground/20']">
          <span :class="['absolute top-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform', appStore.useOnlineArtistArtwork ? 'translate-x-5.5 left-0.5' : 'left-0.5']" />
        </button>
      </div>
    </section>

    <section>
      <div class="flex items-center gap-2 mb-6 text-foreground opacity-60 select-none">
        <MicVocal class="w-4 h-4" />
        <h2 class="text-sm font-bold uppercase tracking-wider">Lyrics</h2>
      </div>
      <div class="bg-card rounded-2xl border border-foreground/[0.06] divide-y divide-foreground/[0.06]">
        <div class="p-5 flex items-center justify-between">
          <div>
            <p class="text-sm font-semibold">LRCLIB.NET</p>
            <p class="text-xs text-foreground opacity-60 mt-1">Fetch lyrics from LRCLIB.NET</p>
          </div>
          <button @click="appStore.updateEnableLrclib(!appStore.enableLrclib)"
            :class="['w-11 h-6 rounded-full transition-colors relative', appStore.enableLrclib ? 'bg-primary' : 'bg-foreground/20']">
            <span :class="['absolute top-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform', appStore.enableLrclib ? 'translate-x-5.5 left-0.5' : 'left-0.5']" />
          </button>
        </div>
        <div class="p-5 flex items-center justify-between">
          <div>
            <p class="text-sm font-semibold">KuGou Lyrics</p>
            <p class="text-xs text-foreground opacity-60 mt-1">Fetch lyrics from KuGou Music</p>
          </div>
          <button @click="appStore.updateEnableKugou(!appStore.enableKugou)"
            :class="['w-11 h-6 rounded-full transition-colors relative', appStore.enableKugou ? 'bg-primary' : 'bg-foreground/20']">
            <span :class="['absolute top-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform', appStore.enableKugou ? 'translate-x-5.5 left-0.5' : 'left-0.5']" />
          </button>
        </div>
        <div class="p-5 flex items-center justify-between">
          <div>
            <p class="text-sm font-semibold">Prefer Local Lyrics</p>
            <p class="text-xs text-foreground opacity-60 mt-1">Use local lyric files when available</p>
          </div>
          <button @click="appStore.updatePreferLocalLyrics(!appStore.preferLocalLyrics)"
            :class="['w-11 h-6 rounded-full transition-colors relative', appStore.preferLocalLyrics ? 'bg-primary' : 'bg-foreground/20']">
            <span :class="['absolute top-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform', appStore.preferLocalLyrics ? 'translate-x-5.5 left-0.5' : 'left-0.5']" />
          </button>
        </div>
      </div>
    </section>
  </div>
</template>
