import os
import sys
import io
import struct
from PIL import Image, ImageFilter

def build_win32_ico(images, out_path):
    entries = []
    data_blobs = []
    
    for img in images:
        w, h = img.size
        if w >= 256 and h >= 256:
            buf = io.BytesIO()
            img.save(buf, format="PNG")
            data = buf.getvalue()
        else:
            # 32-bit uncompressed DIB/BMP with alpha channel for native Win32 GDI compatibility
            img_flipped = img.transpose(Image.Transpose.FLIP_TOP_BOTTOM).convert("RGBA")
            r, g, b, a = img_flipped.split()
            bgra = Image.merge("RGBA", (b, g, r, a)).tobytes()
            and_row = ((w + 31) // 32) * 4
            and_mask = b"\x00" * (and_row * h)
            header = struct.pack("<IIIHHIIIIII", 40, w, h * 2, 1, 32, 0, len(bgra), 0, 0, 0, 0)
            data = header + bgra + and_mask
        
        bw = 0 if w >= 256 else w
        bh = 0 if h >= 256 else h
        data_blobs.append(data)
        entries.append((bw, bh, len(data)))
    
    offset = 6 + 16 * len(images)
    header = struct.pack("<HHH", 0, 1, len(images))
    dir_entries = []
    for (bw, bh, size), data in zip(entries, data_blobs):
        dir_entries.append(struct.pack("<BBBBHHII", bw, bh, 0, 0, 1, 32, size, offset))
        offset += size
        
    with open(out_path, "wb") as f:
        f.write(header)
        for de in dir_entries:
            f.write(de)
        for data in data_blobs:
            f.write(data)

def generate_icons(master_png_path, dest_dir="src-tauri/icons", public_dir="public", assets_dir="assets"):
    os.makedirs(dest_dir, exist_ok=True)
    os.makedirs(public_dir, exist_ok=True)
    os.makedirs(assets_dir, exist_ok=True)

    master = Image.open(master_png_path).convert("RGBA")
    
    # 1. Master PNG (512x512)
    master_512 = master.resize((512, 512), Image.Resampling.LANCZOS)
    master_512.save(os.path.join(dest_dir, "icon.png"), "PNG")
    master_512.save(os.path.join(public_dir, "icon.png"), "PNG")

    # 2. Key PNG sizes for Tauri & Windows Store / Tiles
    sizes_map = {
        "32x32.png": (32, 32),
        "64x64.png": (64, 64),
        "128x128.png": (128, 128),
        "128x128@2x.png": (256, 256),
        "Square30x30Logo.png": (30, 30),
        "Square44x44Logo.png": (44, 44),
        "Square71x71Logo.png": (71, 71),
        "Square89x89Logo.png": (89, 89),
        "Square107x107Logo.png": (107, 107),
        "Square142x142Logo.png": (142, 142),
        "Square150x150Logo.png": (150, 150),
        "Square284x284Logo.png": (284, 284),
        "Square310x310Logo.png": (310, 310),
        "StoreLogo.png": (50, 50),
    }

    for filename, (w, h) in sizes_map.items():
        resized = master.resize((w, h), Image.Resampling.LANCZOS)
        if w <= 48:
            resized = resized.filter(ImageFilter.UnsharpMask(radius=0.7, percent=140, threshold=2))
        resized.save(os.path.join(dest_dir, filename), "PNG")
        print(f"Generated: {filename} ({w}x{h})")

    # Save additional frontend public assets
    master_512.save(os.path.join(public_dir, "logo.png"), "PNG")
    master_512.save(os.path.join(assets_dir, "eiryn_icon.png"), "PNG")

    # 3. High-DPI Comprehensive Windows .ICO file (100%, 125%, 150%, 200% DPI scales)
    # Includes 16, 20, 24, 30, 32, 40, 48, 64, 96, 128, 256 to ensure ZERO scaling blur on any display
    ico_sizes = [
        (16, 16), (20, 20), (24, 24), (30, 30), (32, 32),
        (40, 40), (48, 48), (64, 64), (96, 96), (128, 128), (256, 256)
    ]
    ico_images = []
    for s in ico_sizes:
        img_s = master.resize(s, Image.Resampling.LANCZOS)
        if s[0] <= 48:
            img_s = img_s.filter(ImageFilter.UnsharpMask(radius=0.7, percent=140, threshold=2))
        ico_images.append(img_s)

    ico_path = os.path.join(dest_dir, "icon.ico")
    build_win32_ico(ico_images, ico_path)
    build_win32_ico(ico_images, os.path.join(assets_dir, "eiryn.ico"))
    print(f"Generated Win32 Multi-Resolution ICO: {ico_path} with sizes {ico_sizes}")

if __name__ == "__main__":
    if len(sys.argv) > 1:
        generate_icons(sys.argv[1])
    else:
        print("Usage: python generate_windows_icons.py <master_image.png>")
