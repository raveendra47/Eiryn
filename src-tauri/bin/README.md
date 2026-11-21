# FFmpeg sidecar (loudness analysis)

`audio/lufs.rs:find_ffmpeg()` resolves the binary in this order:

1. `EIRYN_FFMPEG` env var (dev override)
2. Bundled sidecar next to the app executable:
   `ffmpeg-x86_64-pc-windows-msvc.exe` (or plain `ffmpeg.exe`)
3. `ffmpeg` on `PATH`

## Vendoring for release

1. Download a **LGPL** Windows x86_64 build (keeps Eiryn's MIT licensing
   intact — avoid GPL full builds):
   `https://github.com/BtbN/FFmpeg-Builds/releases` →
   `ffmpeg-n7.1-latest-win64-lgpl-7.1.zip` → extract `bin/ffmpeg.exe`.
2. Place it here as `bin/ffmpeg-x86_64-pc-windows-msvc.exe`.
3. Re-add to `tauri.conf.json` under `bundle`:
   `"externalBin": ["bin/ffmpeg-x86_64-pc-windows-msvc"]`
   (Tauri appends the target triple at build time, so the file MUST exist
   or the build script fails.)

Required filters (present in both BtbN and Gyan builds):
`ebur128`, `aresample`, `aformat`, `anull`.

Without a binary, loudness analysis reports `ffmpeg_available: false`
(`get_loudness_progress`) and playback falls back to ReplayGain tags.
