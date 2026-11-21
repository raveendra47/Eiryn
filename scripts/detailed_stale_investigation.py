import os
import json
import re
from pathlib import Path

print("--- A. DEEP DIVE: NPM PACKAGES ---")
suspect_npm = [
    "@material-symbols-svg/vue",
    "@radix-icons/vue",
    "ogl",
    "@tauri-apps/plugin-window",
    "@tauri-apps/plugin-fs",
    "@tauri-apps/plugin-shell"
]

for pkg in suspect_npm:
    matches = []
    for root, _, files in os.walk("."):
        if any(skip in root for skip in ["node_modules", ".git", "target", "dist"]):
            continue
        for f in files:
            if f.endswith((".ts", ".vue", ".js", ".json", ".html")):
                fp = os.path.join(root, f)
                try:
                    with open(fp, "r", encoding="utf-8", errors="ignore") as file_obj:
                        txt = file_obj.read()
                        if pkg in txt:
                            matches.append(fp)
                except:
                    pass
    print(f"Package: {pkg}")
    print(f"  Referenced in: {matches}")

print("\n--- B. DEEP DIVE: RUST CARGO CRATES ---")
suspect_cargo = ["md-5", "md5", "dashmap", "moka", "pollster", "tantivy"]
rust_files = []
for root, _, files in os.walk("src-tauri/src"):
    for f in files:
        if f.endswith(".rs"):
            rust_files.append(os.path.join(root, f))

for crate in suspect_cargo:
    matches = []
    ident = crate.replace("-", "_")
    for rf in rust_files:
        try:
            with open(rf, "r", encoding="utf-8", errors="ignore") as file_obj:
                txt = file_obj.read()
                if ident in txt:
                    # find matching lines
                    for idx, line in enumerate(txt.splitlines(), 1):
                        if ident in line:
                            matches.append(f"{rf}:{idx} -> {line.strip()}")
        except:
            pass
    print(f"Crate: {crate} (ident: {ident})")
    if matches:
        for m in matches[:5]:
            print(f"  {m}")
    else:
        print("  NO USAGE FOUND in src-tauri/src/*.rs!")

print("\n--- C. DEEP DIVE: ORPHAN VUE / TS FILES ---")
suspect_files = [
    "src/components/EQProfileDialog.vue",
    "src/components/PlaylistArtwork.vue",
    "src/components/PlaylistCard.vue",
    "src/components/PullToRefresh.vue",
    "src/components/SignalPathModal.vue",
    "src/components/TrackTableFilter.vue",
    "src/components/TrackTableSection.vue",
    "src/components/UpdateDialog.vue",
    "src/components/player/FloatingPlayerContent.vue",
    "src/components/player/GlassBlurBackground.vue",
    "src/components/player/PlayerTrackInfo.vue",
    "src/components/settings/PlaybackSettings.vue",
    "src/components/settings/SyncProgressDialog.vue",
    "src/composables/useFindLyricsDialog.ts",
    "src/composables/useGlassBlur.ts",
    "src/composables/useKeyboardShortcut.ts",
    "src/composables/useLibraryUpdates.ts",
    "src/composables/usePersistentSettings.ts",
    "src/composables/useRestoreScroll.ts",
    "src/composables/useRowBackground.ts",
]

for sf in suspect_files:
    stem = Path(sf).stem
    matches = []
    for root, _, files in os.walk("src"):
        for f in files:
            fp = os.path.join(root, f)
            if os.path.abspath(fp) == os.path.abspath(sf):
                continue
            try:
                with open(fp, "r", encoding="utf-8", errors="ignore") as file_obj:
                    txt = file_obj.read()
                    if stem in txt:
                        matches.append(fp)
            except:
                pass
    if matches:
        print(f"File {sf} is REFERENCED in {len(matches)} files: {matches}")
    else:
        size = os.path.getsize(sf) if os.path.exists(sf) else 0
        print(f"--> [CONFIRMED DEAD FILE] {sf} ({size:,} bytes)")
