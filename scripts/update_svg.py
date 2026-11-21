import base64
import shutil

shutil.copy2(r"src-tauri\icons\icon.png", r"public\logo.png")
print("Updated public/logo.png")

with open(r"src-tauri\icons\icon.png", "rb") as f:
    b64 = base64.b64encode(f.read()).decode("utf-8")

svg_content = f'''<svg width="512" height="512" viewBox="0 0 512 512" fill="none" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
  <image href="data:image/png;base64,{b64}" width="512" height="512" />
</svg>'''

with open(r"public\icon.svg", "w", encoding="utf-8") as f:
    f.write(svg_content)
print("Updated public/icon.svg")
