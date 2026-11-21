import os
import json
import re
from pathlib import Path

print("=" * 60)
print("1. NPM DEPENDENCIES AUDIT")
print("=" * 60)

with open("package.json", "r", encoding="utf-8") as f:
    pkg = json.load(f)

deps = pkg.get("dependencies", {})
dev_deps = pkg.get("devDependencies", {})

src_files = []
for root, _, files in os.walk("src"):
    for file in files:
        if file.endswith((".ts", ".vue", ".js", ".html", ".css")):
            src_files.append(os.path.join(root, file))

if os.path.exists("index.html"):
    src_files.append("index.html")

dep_usage = {d: [] for d in deps}

for p in src_files:
    try:
        with open(p, "r", encoding="utf-8", errors="ignore") as f:
            content = f.read()
            for d in deps:
                pattern = r"['\"]" + re.escape(d) + r"(\/|['\"])"
                if re.search(pattern, content):
                    dep_usage[d].append(p)
    except Exception as e:
        pass

for d, files in dep_usage.items():
    status = f"USED in {len(files)} files" if files else "--> [POTENTIALLY UNUSED]"
    print(f"{d:35s} : {status}")

print("\n" + "=" * 60)
print("2. NPM DEPENDENCY DISK SIZES (node_modules)")
print("=" * 60)

def get_dir_size(path):
    total = 0
    if not os.path.exists(path):
        return 0
    for root, _, files in os.walk(path):
        for f in files:
            fp = os.path.join(root, f)
            try:
                total += os.path.getsize(fp)
            except:
                pass
    return total

all_deps = list(deps.keys()) + list(dev_deps.keys())
pkg_sizes = []
for d in all_deps:
    pkg_dir = os.path.join("node_modules", d)
    size = get_dir_size(pkg_dir)
    pkg_sizes.append((d, size))

pkg_sizes.sort(key=lambda x: x[1], reverse=True)
for d, size in pkg_sizes:
    mb = size / (1024 * 1024)
    print(f"{d:35s} : {mb:6.2f} MB ({size:,} bytes)")

total_node_modules = get_dir_size("node_modules") / (1024 * 1024)
print(f"\nTotal node_modules size: {total_node_modules:.2f} MB")

print("\n" + "=" * 60)
print("3. RUST CARGO DEPENDENCIES AUDIT")
print("=" * 60)

cargo_toml_path = os.path.join("src-tauri", "Cargo.toml")
with open(cargo_toml_path, "r", encoding="utf-8") as f:
    cargo_content = f.read()

# Extract crate names from [dependencies] and [target.'...'.dependencies]
cargo_deps = []
in_deps = False
for line in cargo_content.splitlines():
    line = line.strip()
    if line.startswith("[") and ("dependencies" in line):
        in_deps = True
        continue
    elif line.startswith("["):
        in_deps = False
        continue
    if in_deps and line and not line.startswith("#") and "=" in line:
        crate_name = line.split("=")[0].strip()
        if crate_name not in cargo_deps:
            cargo_deps.append(crate_name)

rust_files = []
for root, _, files in os.walk(os.path.join("src-tauri", "src")):
    for file in files:
        if file.endswith(".rs"):
            rust_files.append(os.path.join(root, file))

# Normalize crate names (hyphen to underscore)
crate_usage = {}
for c in cargo_deps:
    ident = c.replace("-", "_")
    crate_usage[c] = []
    # search patterns in rust code:
    # use ident::, ident::, extern crate ident, ident =
    pat = re.compile(r'\b' + re.escape(ident) + r'::')
    for rf in rust_files:
        try:
            with open(rf, "r", encoding="utf-8", errors="ignore") as f:
                content = f.read()
                if pat.search(content) or f'"{c}"' in content or f'"{ident}"' in content:
                    crate_usage[c].append(rf)
        except:
            pass

for c, files in crate_usage.items():
    status = f"USED in {len(files)} files" if files else "--> [POTENTIALLY UNUSED]"
    print(f"{c:35s} : {status}")

print("\n" + "=" * 60)
print("4. ORPHAN / UNUSED VUE COMPONENTS & COMPOSABLES")
print("=" * 60)

vue_files = [f for f in src_files if f.endswith((".vue", ".ts"))]
# Check for each file if its basename (without extension) is referenced anywhere else
all_content = {}
for f in vue_files:
    try:
        with open(f, "r", encoding="utf-8", errors="ignore") as fp:
            all_content[f] = fp.read()
    except:
        pass

for f in sorted(vue_files):
    name = Path(f).stem
    # Skip main entry points
    if name in ["main", "App", "index", "router", "vite-env.d"]:
        continue
    # Count references across other files
    refs = []
    for other_f, content in all_content.items():
        if other_f == f:
            continue
        if name in content:
            refs.append(other_f)
    if not refs:
        print(f"--> [ORPHAN FILE - 0 references]: {f}")

print("\n" + "=" * 60)
print("5. REPO DISK FOOTPRINT BY DIRECTORY")
print("=" * 60)

for entry in os.listdir("."):
    if os.path.isdir(entry):
        sz = get_dir_size(entry) / (1024 * 1024)
        print(f"{entry:35s} : {sz:8.2f} MB")
