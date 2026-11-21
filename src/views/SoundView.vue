<script setup lang="ts">
import SignalFlow from '../components/SignalFlow.vue'
import EQPanel from '../components/EQPanel.vue'
import { usePlayerStore } from '../stores/player'
import { useAppStore } from '../stores/app'

const player = usePlayerStore()
const app = useAppStore()

function scrollToSection(id: string) {
  const el = document.getElementById(`sound-${id}`)
  if (el) {
    el.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }
}

// Sound Check options
const replaygainModes = [
  { value: 'album', label: 'By album' },
  { value: 'track', label: 'By song' },
]

function toggleSoundCheck(checked: boolean) {
  if (checked) {
    app.updateReplaygainMode('album')
  } else {
    app.updateReplaygainMode('off')
  }
}

// Crossfeed levels
const crossfeedLevels = [
  { value: 'default', label: 'Subtle' },
  { value: 'cmoy', label: 'Medium' },
  { value: 'meier', label: 'Strong' },
]
</script>

<template>
  <div class="sound-page h-full overflow-y-auto px-8 py-8 select-none">
    <div class="max-w-4xl mx-auto space-y-8">
      <!-- Title & Lead -->
      <div>
        <h1 class="text-3xl font-bold tracking-tight text-foreground">Sound</h1>
        <p class="text-sm text-foreground/60 mt-2 max-w-2xl leading-relaxed">
          How your music is shaped on its way to the speakers. Everything here is optional; with it all off, Eiryn plays your files exactly as they are.
        </p>
      </div>

      <!-- Top Interactive Signal Path Flow -->
      <SignalFlow :onstage="scrollToSection" />

      <!-- Direct Output (WASAPI Exclusive) -->
      <section id="sound-output" class="sound-section">
        <div class="title-row">
          <div class="section-meta">
            <h2 class="section-title">Direct output</h2>
            <p class="section-desc">
              Send music straight to your device at each song’s own sample rate, skipping the Windows mixer (WASAPI exclusive mode).
              With Sound Check, the equalizer and crossfeed off and volume at 100%, every sample arrives exactly as it is in the file.
            </p>
            <p class="section-hint">
              While Eiryn plays in exclusive mode, other apps can’t make sound on this device.
            </p>
          </div>
          <button
            class="toggle-switch"
            :class="{ active: player.bpRequested }"
            role="switch"
            :aria-checked="player.bpRequested"
            @click="player.setBitPerfect(!player.bpRequested)"
          >
            <span class="toggle-thumb" />
          </button>
        </div>
      </section>

      <!-- Sound Check (Leveling / ReplayGain) -->
      <section id="sound-leveling" class="sound-section">
        <div class="title-row">
          <div class="section-meta">
            <h2 class="section-title">Sound Check</h2>
            <p class="section-desc">
              Plays every song at the same loudness using ReplayGain / LUFS normalization, and gives loud masters the headroom they need to stay clean.
            </p>
          </div>
          <button
            class="toggle-switch"
            :class="{ active: app.replaygainMode !== 'off' }"
            role="switch"
            :aria-checked="app.replaygainMode !== 'off'"
            @click="toggleSoundCheck(app.replaygainMode === 'off')"
          >
            <span class="toggle-thumb" />
          </button>
        </div>

        <div v-if="app.replaygainMode !== 'off'" class="mt-4 pl-4 border-l-2 border-foreground/10 space-y-4">
          <div class="flex items-center gap-4">
            <span class="text-xs text-foreground/60 w-16">Match</span>
            <div class="flex rounded-xl bg-foreground/[0.05] p-1 border border-foreground/[0.08]">
              <button
                v-for="m in replaygainModes"
                :key="m.value"
                @click="app.updateReplaygainMode(m.value as any)"
                class="px-3.5 py-1 text-xs rounded-lg font-medium transition-all"
                :class="app.replaygainMode === m.value ? 'bg-primary text-primary-foreground shadow-sm' : 'text-foreground/70 hover:text-foreground'"
              >
                {{ m.label }}
              </button>
            </div>
          </div>
          <p class="section-hint">
            {{
              app.replaygainMode === 'album'
                ? 'Every album plays at the same loudness, keeping the differences between its songs.'
                : 'Every song plays at the same loudness.'
            }}
          </p>
        </div>
      </section>

      <!-- Equalizer -->
      <section id="sound-eq" class="sound-section">
        <div class="title-row mb-6">
          <div class="section-meta">
            <h2 class="section-title">Equalizer</h2>
            <p class="section-desc">
              Drag the points to shape the sound. 10-band graphic equalizer with preset curves, plus parametric headphone correction profiles.
            </p>
          </div>
        </div>

        <!-- Embedded EQ Panel with AutoEq Importer -->
        <div class="bg-foreground/[0.02] border border-foreground/[0.08] rounded-2xl p-6">
          <EQPanel />
        </div>
      </section>

      <!-- Crossfade -->
      <section id="sound-crossfade" class="sound-section">
        <div class="title-row">
          <div class="section-meta">
            <h2 class="section-title">Crossfade</h2>
            <p class="section-desc">
              Blend the end of one song into the start of the next. Songs from the same album always play back to back without a gap.
            </p>
          </div>
          <button
            class="toggle-switch"
            :class="{ active: player.crossfade > 0 }"
            role="switch"
            :aria-checked="player.crossfade > 0"
            @click="player.setCrossfade(player.crossfade > 0 ? 0 : 3)"
          >
            <span class="toggle-thumb" />
          </button>
        </div>

        <div v-if="player.crossfade > 0" class="mt-4 pl-4 border-l-2 border-foreground/10 space-y-2">
          <div class="flex items-center gap-4">
            <span class="text-xs text-foreground/60 w-16">Length</span>
            <input
              type="range"
              min="1"
              max="12"
              step="1"
              :value="player.crossfade"
              @input="player.setCrossfade(Number(($event.target as HTMLInputElement).value))"
              class="w-56 accent-primary"
            />
            <span class="text-xs font-mono tabular-nums text-foreground/80">{{ player.crossfade }} s</span>
          </div>
        </div>
      </section>

      <!-- Headphone Crossfeed (bs2b) -->
      <section id="sound-crossfeed" class="sound-section">
        <div class="title-row">
          <div class="section-meta">
            <h2 class="section-title">Headphone crossfeed</h2>
            <p class="section-desc">
              Lets each ear hear a little of the other channel, like speakers in a room. Makes hard-panned recordings natural and fatigue-free on headphones.
            </p>
          </div>
          <button
            class="toggle-switch"
            :class="{ active: player.crossfeedEnabled }"
            role="switch"
            :aria-checked="player.crossfeedEnabled"
            @click="player.setCrossfeedSettings(!player.crossfeedEnabled, player.crossfeedLevel || 'default')"
          >
            <span class="toggle-thumb" />
          </button>
        </div>

        <div v-if="player.crossfeedEnabled" class="mt-4 pl-4 border-l-2 border-foreground/10 space-y-4">
          <div class="flex items-center gap-4">
            <span class="text-xs text-foreground/60 w-16">Strength</span>
            <div class="flex rounded-xl bg-foreground/[0.05] p-1 border border-foreground/[0.08]">
              <button
                v-for="lvl in crossfeedLevels"
                :key="lvl.value"
                @click="player.setCrossfeedSettings(true, lvl.value)"
                class="px-3.5 py-1 text-xs rounded-lg font-medium transition-all"
                :class="player.crossfeedLevel === lvl.value ? 'bg-primary text-primary-foreground shadow-sm' : 'text-foreground/70 hover:text-foreground'"
              >
                {{ lvl.label }}
              </button>
            </div>
          </div>
          <p class="section-hint">
            {{
              player.crossfeedLevel === 'cmoy'
                ? 'Medium (Chu Moy preset: 700 Hz, 6.0 dB crossfeed).'
                : player.crossfeedLevel === 'meier'
                  ? 'Strong (Jan Meier preset: 650 Hz, 9.5 dB crossfeed).'
                  : 'Subtle (Default bs2b preset: 700 Hz, 4.5 dB crossfeed).'
            }}
          </p>
        </div>
      </section>

      <!-- Karaoke Center-Vocal Remover -->
      <section id="sound-karaoke" class="sound-section">
        <div class="title-row">
          <div class="section-meta">
            <h2 class="section-title">Karaoke Vocal Remover</h2>
            <p class="section-desc">
              Cancels center-panned vocals using differential phase filtering while preserving sub-bass and punch below 150 Hz so you can sing along to any song.
            </p>
          </div>
          <button
            class="toggle-switch"
            :class="{ active: player.karaokeEnabled }"
            role="switch"
            :aria-checked="player.karaokeEnabled"
            @click="player.setKaraoke(!player.karaokeEnabled)"
          >
            <span class="toggle-thumb" />
          </button>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.sound-section {
  padding-top: 28px;
  border-top: 1px solid rgba(255, 255, 255, 0.08);
  scroll-margin-top: 40px;
}

.title-row {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 28px;
}

.section-meta {
  flex: 1;
}

.section-title {
  font-size: 18px;
  font-weight: 600;
  color: #ffffff;
}

.section-desc {
  margin-top: 6px;
  font-size: 13px;
  color: rgba(255, 255, 255, 0.65);
  line-height: 1.5;
  max-width: 66ch;
}

.section-hint {
  margin-top: 8px;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.45);
  line-height: 1.5;
}

.toggle-switch {
  width: 44px;
  height: 24px;
  border-radius: 9999px;
  background: rgba(255, 255, 255, 0.12);
  position: relative;
  transition: background 0.2s ease;
  flex-shrink: 0;
  cursor: pointer;
}

.toggle-switch.active {
  background: var(--primary, #e11d48);
}

.toggle-thumb {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 20px;
  height: 20px;
  border-radius: 9999px;
  background: #ffffff;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.3);
  transition: transform 0.2s ease;
}

.toggle-switch.active .toggle-thumb {
  transform: translateX(20px);
}
</style>
