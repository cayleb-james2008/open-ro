import hashlib
import io
import json
import math
import os
import sys
from collections import deque
from pathlib import Path
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageOps


def fail(msg):
    print(json.dumps({"ok": False, "error": str(msg)}))
    sys.exit(1)


def parse_color(val, default=(0, 0, 0, 255)):
    if val is None:
        return default
    if isinstance(val, (list, tuple)) and len(val) in (3, 4):
        c = [int(max(0, min(255, x))) for x in val]
        return tuple(c) if len(c) == 4 else (c[0], c[1], c[2], 255)
    s = str(val).strip().lstrip("#")
    if len(s) == 6:
        return (int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16), 255)
    if len(s) == 8:
        return (int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16), int(s[6:8], 16))
    return default


def sample_bg_color(img):
    w, h = img.size
    px = img.load()
    pts = [
        (0, 0),
        (w - 1, 0),
        (0, h - 1),
        (w - 1, h - 1),
        (w // 2, 0),
        (w // 2, h - 1),
        (0, h // 2),
        (w - 1, h // 2),
    ]
    samples = [px[x, y] for x, y in pts]
    rs = sorted(c[0] for c in samples)
    gs = sorted(c[1] for c in samples)
    bs = sorted(c[2] for c in samples)
    mid = len(samples) // 2
    return (rs[mid], gs[mid], bs[mid], 255)


def color_dist(a, b):
    return max(abs(a[0] - b[0]), abs(a[1] - b[1]), abs(a[2] - b[2]))


def remove_bg_defringe(img, bg_color=None, method="auto", tol=42, cutout_interior=False, defringe=True):
    img = img.convert("RGBA")
    w, h = img.size
    px = img.load()
    if bg_color is None:
        bg_color = sample_bg_color(img)
    else:
        bg_color = parse_color(bg_color, (0, 255, 0, 255))

    # Keep already-transparent pixels transparent.
    mask = Image.new("L", (w, h), 255)
    mpx = mask.load()

    if method == "chroma":
        for y in range(h):
            for x in range(w):
                c = px[x, y]
                if c[3] <= 12 or color_dist(c, bg_color) <= tol:
                    mpx[x, y] = 0
    else:
        # Outer corner + perimeter flood fill
        visited = bytearray(w * h)
        q = deque()
        border_seeds = []
        for x in range(w):
            border_seeds.append((x, 0))
            border_seeds.append((x, h - 1))
        for y in range(h):
            border_seeds.append((0, y))
            border_seeds.append((w - 1, y))
        for x, y in border_seeds:
            idx = y * w + x
            if not visited[idx]:
                c = px[x, y]
                if c[3] <= 12 or color_dist(c, bg_color) <= tol:
                    visited[idx] = 1
                    q.append((x, y))
        while q:
            x, y = q.popleft()
            mpx[x, y] = 0
            for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
                if 0 <= nx < w and 0 <= ny < h:
                    nidx = ny * w + nx
                    if not visited[nidx]:
                        c = px[nx, ny]
                        if c[3] <= 12 or color_dist(c, bg_color) <= tol:
                            visited[nidx] = 1
                            q.append((nx, ny))
        if cutout_interior:
            for y in range(h):
                for x in range(w):
                    if mpx[x, y] > 0:
                        c = px[x, y]
                        if c[3] <= 12 or color_dist(c, bg_color) <= tol:
                            mpx[x, y] = 0

    out = img.copy()
    orig_a = img.getchannel("A")
    final_a = ImageChops.darker(orig_a, mask)

    if defringe:
        opx = out.load()
        apx = final_a.load()
        bg_r, bg_g, bg_b = bg_color[0], bg_color[1], bg_color[2]
        halo_tol = tol + 48
        # Soft-decontaminate 1px fringe adjacent to transparent pixels
        updates = []
        for y in range(h):
            for x in range(w):
                a = apx[x, y]
                if a == 0:
                    opx[x, y] = (0, 0, 0, 0)
                    continue
                c = opx[x, y]
                near_bg = False
                best_neighbor = None
                best_dist = -1
                for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
                    if 0 <= nx < w and 0 <= ny < h:
                        na = apx[nx, ny]
                        if na == 0:
                            near_bg = True
                        elif na >= 220:
                            nc = opx[nx, ny]
                            d = color_dist(nc, bg_color)
                            if d > best_dist:
                                best_dist = d
                                best_neighbor = nc
                if near_bg and color_dist(c, bg_color) <= halo_tol:
                    if color_dist(c, bg_color) <= tol + 16:
                        updates.append((x, y, (0, 0, 0, 0)))
                    elif best_neighbor is not None:
                        updates.append((x, y, (best_neighbor[0], best_neighbor[1], best_neighbor[2], min(a, 210))))
                else:
                    # Despill strong chroma green or magenta edge tint
                    r, g, b = c[0], c[1], c[2]
                    if bg_g > 200 and bg_r < 60 and bg_b < 60 and g > max(r, b) + 28 and near_bg:
                        g_new = max(r, b)
                        updates.append((x, y, (r, g_new, b, a)))
                    elif bg_r > 200 and bg_b > 200 and bg_g < 60 and min(r, b) > g + 28 and near_bg:
                        avg = (r + b) // 2
                        updates.append((x, y, (min(r, g + 16), g, min(b, g + 16), a)))
        for x, y, col in updates:
            opx[x, y] = col
            apx[x, y] = col[3]
    else:
        out.putalpha(final_a)
    return out


def connected_bbox(img, alpha_thresh=16):
    w, h = img.size
    apx = img.getchannel("A").load()
    visited = bytearray(w * h)
    comps = []
    for sy in range(h):
        for sx in range(w):
            idx = sy * w + sx
            if visited[idx] or apx[sx, sy] <= alpha_thresh:
                continue
            visited[idx] = 1
            q = deque([(sx, sy)])
            area = 0
            x0, y0, x1, y1 = sx, sy, sx, sy
            while q:
                x, y = q.popleft()
                area += 1
                if x < x0:
                    x0 = x
                if y < y0:
                    y0 = y
                if x > x1:
                    x1 = x
                if y > y1:
                    y1 = y
                for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
                    if 0 <= nx < w and 0 <= ny < h:
                        nidx = ny * w + nx
                        if not visited[nidx] and apx[nx, ny] > alpha_thresh:
                            visited[nidx] = 1
                            q.append((nx, ny))
            comps.append((area, x0, y0, x1 + 1, y1 + 1))
    if not comps:
        return None
    max_area = max(c[0] for c in comps)
    min_keep = max(4, int(max_area * 0.005))
    kept = [c for c in comps if c[0] >= min_keep] or comps
    return (
        min(c[1] for c in kept),
        min(c[2] for c in kept),
        max(c[3] for c in kept),
        max(c[4] for c in kept),
    )


def compute_slice_center(img, cutout_interior=False):
    w, h = img.size
    apx = img.getchannel("A").load()
    if cutout_interior:
        mx, my = w // 2, h // 2
        if apx[mx, my] <= 24:
            lx = mx
            while lx > 2 and apx[lx, my] <= 24:
                lx -= 1
            rx = mx
            while rx < w - 3 and apx[rx, my] <= 24:
                rx += 1
            ty = my
            while ty > 2 and apx[mx, ty] <= 24:
                ty -= 1
            by = my
            while by < h - 3 and apx[mx, by] <= 24:
                by += 1
            l = max(8, min(w // 2 - 4, lx + 4))
            r = min(w - 8, max(w // 2 + 4, rx - 4))
            t = max(8, min(h // 2 - 4, ty + 4))
            b = min(h - 8, max(h // 2 + 4, by - 4))
            if 0 < l < r < w and 0 < t < b < h:
                return [int(l), int(t), int(r), int(b)]

    # Scan opaque outer margins + safe bevel inset
    my = h // 2
    mx = w // 2
    left_edge = 0
    while left_edge < w // 2 and apx[left_edge, my] <= 16:
        left_edge += 1
    right_edge = w - 1
    while right_edge > w // 2 and apx[right_edge, my] <= 16:
        right_edge -= 1
    top_edge = 0
    while top_edge < h // 2 and apx[mx, top_edge] <= 16:
        top_edge += 1
    bot_edge = h - 1
    while bot_edge > h // 2 and apx[mx, bot_edge] <= 16:
        bot_edge -= 1

    bw = max(1, right_edge - left_edge + 1)
    bh = max(1, bot_edge - top_edge + 1)
    inset_x = max(12, min(bw // 3, int(round(bw * 0.24))))
    inset_y = max(12, min(bh // 3, int(round(bh * 0.24))))
    l = max(4, min(w // 2 - 2, left_edge + inset_x))
    r = min(w - 4, max(w // 2 + 2, right_edge - inset_x))
    t = max(4, min(h // 2 - 2, top_edge + inset_y))
    b = min(h - 4, max(h // 2 + 2, bot_edge - inset_y))
    return [int(l), int(t), int(r), int(b)]


def save_and_verify(img, out_abs, max_bytes):
    out_p = Path(out_abs)
    out_p.parent.mkdir(parents=True, exist_ok=True)
    tmp = out_p.with_suffix(".png.tmp-openro")
    buf = io.BytesIO()
    img.save(buf, format="PNG", optimize=True)
    data = buf.getvalue()
    if len(data) > max_bytes:
        q = img.quantize(colors=256, method=Image.Quantize.FASTOCTREE)
        buf = io.BytesIO()
        q.save(buf, format="PNG", optimize=True)
        data = buf.getvalue()
    if len(data) > max_bytes:
        fail(f"Output PNG is {len(data)} bytes, exceeding limit {max_bytes} bytes.")
    tmp.write_bytes(data)
    os.replace(tmp, out_p)
    with Image.open(out_p) as check:
        check.load()
        cw, ch = check.size
        rgba = check.convert("RGBA")
        alpha = rgba.getchannel("A")
        hist = alpha.histogram()
        total = max(1, cw * ch)
        opaque = sum(hist[17:])
        has_alpha = any(hist[:255])
        opaque_ratio = round(opaque / total, 4)
    return {
        "width": cw,
        "height": ch,
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
        "has_alpha": has_alpha,
        "opaque_ratio": opaque_ratio,
    }


def fit_to_target(cropped, target_size, pad_px=4):
    if target_size:
        tw, th = int(target_size[0]), int(target_size[1])
    else:
        cw, ch = cropped.size
        tw = max(32, ((cw + pad_px * 2 + 1) // 2) * 2)
        th = max(32, ((ch + pad_px * 2 + 1) // 2) * 2)
    inner_w = max(8, tw - pad_px * 2)
    inner_h = max(8, th - pad_px * 2)
    fitted = ImageOps.contain(cropped, (inner_w, inner_h), Image.Resampling.LANCZOS)
    canvas = Image.new("RGBA", (tw, th), (0, 0, 0, 0))
    ox = (tw - fitted.width) // 2
    oy = (th - fitted.height) // 2
    canvas.paste(fitted, (ox, oy), fitted)
    return canvas


def do_slice_sheet(req):
    src = Image.open(req["input_abs"]).convert("RGBA")
    sw, sh = src.size
    bg_color = req.get("bg_color")
    if bg_color is None:
        bg_color = sample_bg_color(src)
    tol = int(req.get("tolerance", 42))
    method = str(req.get("method") or "auto").lower()
    defringe = bool(req.get("defringe", True))
    max_bytes = int(req.get("max_bytes", 5 * 1024 * 1024))
    out_dir = Path(req["out_dir_abs"])
    out_dir.mkdir(parents=True, exist_ok=True)
    out_dir_rel = req["out_dir_rel"].rstrip("/")

    results = []
    for item in req["items"]:
        name = item["name"]
        if item.get("box"):
            bx = item["box"]
            x0, y0, x1, y1 = int(bx[0]), int(bx[1]), int(bx[2]), int(bx[3])
        else:
            rows = max(1, int(item.get("rows", req.get("rows", 2))))
            cols = max(1, int(item.get("cols", req.get("cols", 4))))
            r = int(item.get("row", 0))
            c = int(item.get("col", 0))
            cw_f = sw / cols
            ch_f = sh / rows
            gutter = int(item.get("gutter", 2))
            x0 = max(0, int(round(c * cw_f)) + gutter)
            y0 = max(0, int(round(r * ch_f)) + gutter)
            x1 = min(sw, int(round((c + 1) * cw_f)) - gutter)
            y1 = min(sh, int(round((r + 1) * ch_f)) - gutter)
        cell = src.crop((x0, y0, max(x0 + 4, x1), max(y0 + 4, y1)))
        cutout_interior = bool(item.get("cutout_interior", False))
        cleaned = remove_bg_defringe(
            cell,
            bg_color=bg_color,
            method=method,
            tol=tol,
            cutout_interior=cutout_interior,
            defringe=defringe,
        )
        bbox = connected_bbox(cleaned)
        if bbox is None:
            fail(f"Sliced cell '{name}' contained no foreground pixels after background removal.")
        cropped = cleaned.crop(bbox)
        target_size = item.get("target_size")
        pad_px = int(item.get("pad_px", 4))
        final_img = fit_to_target(cropped, target_size, pad_px=pad_px)
        is_9slice = bool(item.get("is_9slice", False))
        slice_rect = item.get("slice_override")
        if is_9slice and not slice_rect:
            slice_rect = compute_slice_center(final_img, cutout_interior=cutout_interior)
        out_rel = f"{out_dir_rel}/{name}.png"
        out_abs = out_dir / f"{name}.png"
        info = save_and_verify(final_img, str(out_abs), max_bytes)
        results.append({
            "name": name,
            "path": out_rel,
            "is_9slice": is_9slice,
            "slice": slice_rect,
            **info,
        })
    print(json.dumps({"ok": True, "items": results}))


def lerp_col(c1, c2, t):
    return tuple(int(round(c1[k] + (c2[k] - c1[k]) * t)) for k in range(4))


def draw_bevel_panel(d, box, radius, fill_top, fill_bot, rim, stroke_dark, rim_w=4):
    x0, y0, x1, y1 = box
    d.rounded_rectangle((x0, y0, x1, y1), radius=radius, fill=fill_top, outline=stroke_dark, width=rim_w + 2)
    # Vertical gradient fill inside
    steps = max(8, min(64, (y1 - y0) // 4))
    for i in range(steps):
        t = i / max(1, steps - 1)
        iy0 = y0 + int((y1 - y0) * i / steps)
        iy1 = y0 + int((y1 - y0) * (i + 1) / steps)
        col = lerp_col(fill_top, fill_bot, t)
        pad = rim_w + 2
        if x1 - pad > x0 + pad and iy1 > iy0:
            d.rectangle((x0 + pad, iy0, x1 - pad, iy1), fill=col)
    d.rounded_rectangle((x0 + 3, y0 + 3, x1 - 3, y1 - 3), radius=max(4, radius - 3), outline=rim, width=rim_w)
    # Specular top bevel line
    d.line([(x0 + radius, y0 + rim_w + 6), (x1 - radius, y0 + rim_w + 6)], fill=(255, 255, 255, 90), width=3)


def do_render_ui_sheet(req):
    sheet = req["sheet"]
    pal = req["palette"]
    chroma = parse_color(req.get("chroma", "#00FF00"))
    surface = parse_color(pal["surface"])
    elevated = parse_color(pal["surface_elevated"])
    accent = parse_color(pal["accent"])
    rim = parse_color(pal["rim"])
    danger = parse_color(pal["danger"])
    stroke = parse_color(pal["stroke_dark"])
    crest_fill = parse_color(pal["crest_fill"])
    crest_trim = parse_color(pal["crest_trim"])
    text_col = parse_color(pal["text"])
    max_bytes = int(req.get("max_bytes", 5 * 1024 * 1024))

    ss = 2
    if sheet == "chrome_sheet":
        w, h = 2048, 1024
        cols, rows = 4, 2
        cw, ch = (w * ss) // cols, (h * ss) // rows
        canvas = Image.new("RGBA", (w * ss, h * ss), chroma)
        d = ImageDraw.Draw(canvas)

        # (0,0) modal_panel
        cx, cy = 0, 0
        draw_bevel_panel(d, (cx + 80, cy + 80, cx + cw - 80, cy + ch - 80), 40, elevated, surface, rim, stroke, rim_w=6)

        # (1,0) header_crest — original Skybridge Kite-Wing architectural crest (no stars!)
        cx, cy = cw, 0
        mx, my = cx + cw // 2, cy + ch // 2
        kite = [
            (cx + 90, my + 24),
            (cx + 180, my - 90),
            (mx, my - 130),
            (cx + cw - 180, my - 90),
            (cx + cw - 90, my + 24),
            (cx + cw - 160, my + 100),
            (cx + 160, my + 100),
        ]
        d.polygon(kite, fill=crest_fill, outline=stroke)
        inner_kite = [
            (cx + 125, my + 18),
            (cx + 195, my - 68),
            (mx, my - 102),
            (cx + cw - 195, my - 68),
            (cx + cw - 125, my + 18),
            (cx + cw - 178, my + 78),
            (cx + 178, my + 78),
        ]
        d.polygon(inner_kite, fill=elevated, outline=crest_trim)
        d.line(kite + [kite[0]], fill=crest_trim, width=6)
        gem = [(mx, my - 64), (mx + 44, my), (mx, my + 64), (mx - 44, my)]
        d.polygon(gem, fill=accent, outline=stroke)

        # (2,0) btn_primary
        cx, cy = cw * 2, 0
        draw_bevel_panel(d, (cx + 100, cy + 250, cx + cw - 100, cy + ch - 250), 36, accent, lerp_col(accent, stroke, 0.25), crest_trim, stroke, rim_w=6)

        # (3,0) btn_secondary
        cx, cy = cw * 3, 0
        draw_bevel_panel(d, (cx + 100, cy + 250, cx + cw - 100, cy + ch - 250), 36, elevated, surface, rim, stroke, rim_w=5)

        # (0,1) btn_danger
        cx, cy = 0, ch
        draw_bevel_panel(d, (cx + 100, cy + 250, cx + cw - 100, cy + ch - 250), 36, danger, lerp_col(danger, stroke, 0.28), (252, 165, 165, 255), stroke, rim_w=6)

        # (1,1) close_orb
        cx, cy = cw, ch
        mx, my = cx + cw // 2, cy + ch // 2
        r = 220
        d.ellipse((mx - r, my - r, mx + r, my + r), fill=elevated, outline=stroke, width=10)
        d.ellipse((mx - r + 14, my - r + 14, mx + r - 14, my + r - 14), outline=rim, width=8)
        d.line([(mx - 85, my - 85), (mx + 85, my + 85)], fill=text_col, width=26)
        d.line([(mx + 85, my - 85), (mx - 85, my + 85)], fill=text_col, width=26)

        # (2,1) progress_track
        cx, cy = cw * 2, ch
        draw_bevel_panel(d, (cx + 80, cy + 360, cx + cw - 80, cy + ch - 360), 28, surface, lerp_col(surface, (0, 0, 0, 255), 0.3), rim, stroke, rim_w=4)

        # (3,1) progress_fill
        cx, cy = cw * 3, ch
        draw_bevel_panel(d, (cx + 90, cy + 370, cx + cw - 90, cy + ch - 370), 24, accent, rim, (255, 255, 255, 180), stroke, rim_w=4)

        out_img = canvas.resize((w, h), Image.Resampling.LANCZOS)

    elif sheet == "icons_sheet":
        w, h = 2048, 1536
        cols, rows = 4, 3
        cw, ch = (w * ss) // cols, (h * ss) // rows
        canvas = Image.new("RGBA", (w * ss, h * ss), chroma)
        d = ImageDraw.Draw(canvas)
        sw = 12
        for idx in range(12):
            r, c = divmod(idx, cols)
            cx, cy = c * cw, r * ch
            mx, my = cx + cw // 2, cy + ch // 2
            rad = 220
            # Backplate medallion + distinct icon motif per slot
            if idx == 0:  # summon: winged prism crystal
                pts = [(mx, my - rad), (mx + 120, my), (mx, my + rad), (mx - 120, my)]
                d.polygon(pts, fill=accent, outline=stroke)
                d.line(pts + [pts[0]], fill=stroke, width=sw)
                d.polygon([(mx - 190, my - 40), (mx - 110, my - 10), (mx - 130, my + 70)], fill=crest_trim, outline=stroke)
                d.polygon([(mx + 190, my - 40), (mx + 110, my - 10), (mx + 130, my + 70)], fill=crest_trim, outline=stroke)
            elif idx == 1:  # deck: fanned twin cards
                d.rounded_rectangle((mx - 150, my - 170, mx + 30, my + 140), radius=24, fill=elevated, outline=stroke, width=sw)
                d.rounded_rectangle((mx - 30, my - 140, mx + 150, my + 170), radius=24, fill=accent, outline=stroke, width=sw)
            elif idx == 2:  # battle: crossed blades
                d.line([(mx - 160, my + 160), (mx + 160, my - 160)], fill=crest_trim, width=36)
                d.line([(mx - 160, my - 160), (mx + 160, my + 160)], fill=accent, width=36)
                d.ellipse((mx - 42, my - 42, mx + 42, my + 42), fill=elevated, outline=stroke, width=sw)
            elif idx == 3:  # codex: open tome
                d.rounded_rectangle((mx - 190, my - 140, mx, my + 150), radius=20, fill=elevated, outline=stroke, width=sw)
                d.rounded_rectangle((mx, my - 140, mx + 190, my + 150), radius=20, fill=text_col, outline=stroke, width=sw)
                d.rectangle((mx + 50, my - 140, mx + 96, my + 40), fill=accent, outline=stroke, width=6)
            elif idx == 4:  # quest: wax-sealed scroll
                d.rounded_rectangle((mx - 160, my - 150, mx + 160, my + 150), radius=26, fill=text_col, outline=stroke, width=sw)
                d.ellipse((mx - 54, my + 20, mx + 54, my + 128), fill=danger, outline=stroke, width=sw)
                d.line([(mx - 100, my - 70), (mx + 100, my - 70)], fill=stroke, width=18)
                d.line([(mx - 100, my - 10), (mx + 60, my - 10)], fill=stroke, width=18)
            elif idx == 5:  # shop: pavilion stall crest
                d.polygon([(mx - 180, my - 40), (mx - 130, my - 170), (mx + 130, my - 170), (mx + 180, my - 40)], fill=accent, outline=stroke)
                d.rounded_rectangle((mx - 150, my - 40, mx + 150, my + 160), radius=18, fill=elevated, outline=stroke, width=sw)
            elif idx == 6:  # settings: 6-tooth gear
                for k in range(6):
                    ang = k * math.pi / 3
                    gx, gy = mx + int(math.cos(ang) * 150), my + int(math.sin(ang) * 150)
                    d.ellipse((gx - 48, gy - 48, gx + 48, gy + 48), fill=rim, outline=stroke, width=8)
                d.ellipse((mx - 140, my - 140, mx + 140, my + 140), fill=elevated, outline=stroke, width=sw)
                d.ellipse((mx - 58, my - 58, mx + 58, my + 58), fill=chroma, outline=stroke, width=sw)
            elif idx == 7:  # rewards: ribbon chest
                d.rounded_rectangle((mx - 180, my - 130, mx + 180, my + 160), radius=28, fill=crest_fill, outline=stroke, width=sw)
                d.rectangle((mx - 180, my - 24, mx + 180, my + 24), fill=crest_trim, outline=stroke, width=8)
                d.rectangle((mx - 36, my - 130, mx + 36, my + 160), fill=accent, outline=stroke, width=8)
            elif idx == 8:  # coin: octagonal coin
                oct_pts = [(mx + int(math.cos(k * math.pi / 4) * 185), my + int(math.sin(k * math.pi / 4) * 185)) for k in range(8)]
                d.polygon(oct_pts, fill=crest_trim, outline=stroke)
                d.line(oct_pts + [oct_pts[0]], fill=stroke, width=sw)
                d.ellipse((mx - 110, my - 110, mx + 110, my + 110), outline=elevated, width=10)
            elif idx == 9:  # gem: faceted diamond
                gem_pts = [(mx - 110, my - 140), (mx + 110, my - 140), (mx + 185, my - 20), (mx, my + 185), (mx - 185, my - 20)]
                d.polygon(gem_pts, fill=accent, outline=stroke)
                d.line(gem_pts + [gem_pts[0]], fill=stroke, width=sw)
            elif idx == 10:  # spark: lightning bolt
                bolt = [(mx + 30, my - 190), (mx - 120, my + 20), (mx + 10, my + 20), (mx - 40, my + 190), (mx + 130, my - 20), (mx, my - 20)]
                d.polygon(bolt, fill=crest_trim, outline=stroke)
                d.line(bolt + [bolt[0]], fill=stroke, width=sw)
            else:  # shield: kite shield
                sh_pts = [(mx - 155, my - 165), (mx + 155, my - 165), (mx + 145, my + 40), (mx, my + 190), (mx - 145, my + 40)]
                d.polygon(sh_pts, fill=elevated, outline=stroke)
                d.line(sh_pts + [sh_pts[0]], fill=rim, width=sw)
        out_img = canvas.resize((w, h), Image.Resampling.LANCZOS)

    elif sheet == "rarity_frames_sheet":
        w, h = 2048, 1424
        cols, rows = 4, 2
        cw, ch = (w * ss) // cols, (h * ss) // rows
        canvas = Image.new("RGBA", (w * ss, h * ss), chroma)
        d = ImageDraw.Draw(canvas)
        tiers = [
            ("frame_common", (148, 163, 184, 255), (71, 85, 105, 255)),
            ("frame_uncommon", (52, 211, 153, 255), (6, 95, 70, 255)),
            ("frame_rare", (56, 189, 248, 255), (30, 64, 175, 255)),
            ("frame_epic", (192, 132, 252, 255), (107, 33, 168, 255)),
            ("frame_legendary", (251, 191, 36, 255), (180, 83, 9, 255)),
            ("frame_mythic", (34, 211, 238, 255), (219, 39, 119, 255)),
            ("frame_legacy", (254, 243, 199, 255), (161, 98, 7, 255)),
        ]
        for idx, (_name, hi, lo) in enumerate(tiers):
            r, c = divmod(idx, cols)
            cx, cy = c * cw, r * ch
            fx0, fy0, fx1, fy1 = cx + 80, cy + 70, cx + cw - 80, cy + ch - 70
            d.rounded_rectangle((fx0, fy0, fx1, fy1), radius=48, fill=lo, outline=stroke, width=14)
            d.rounded_rectangle((fx0 + 18, fy0 + 18, fx1 - 18, fy1 - 18), radius=40, outline=hi, width=16)
            # Escalating chamfered corner brackets for higher tiers
            if idx >= 2:
                bw = 64 + idx * 8
                for bx, by in ((fx0, fy0), (fx1 - bw, fy0), (fx0, fy1 - bw), (fx1 - bw, fy1 - bw)):
                    d.rounded_rectangle((bx, by, bx + bw, by + bw), radius=14, fill=hi, outline=stroke, width=8)
            if idx >= 4:
                mx = (fx0 + fx1) // 2
                d.polygon([(mx - 120, fy0 + 24), (mx, fy0 - 18), (mx + 120, fy0 + 24), (mx, fy0 + 58)], fill=hi, outline=stroke)
            # Hollow center cutout filled with exact chroma-key bg color
            pad_in = 84
            d.rounded_rectangle((fx0 + pad_in, fy0 + pad_in, fx1 - pad_in, fy1 - pad_in), radius=26, fill=chroma, outline=hi, width=8)
        out_img = canvas.resize((w, h), Image.Resampling.LANCZOS)
    else:
        fail(f"Unknown UI sheet recipe '{sheet}'.")

    info = save_and_verify(out_img, req["out_abs"], max_bytes)
    print(json.dumps({"ok": True, **info}))


def main():
    req = json.loads(sys.stdin.read() or "{}")
    mode = req.get("mode", "")
    if mode == "slice_sheet":
        do_slice_sheet(req)
    elif mode == "render_ui_sheet":
        do_render_ui_sheet(req)
    else:
        fail(f"Unsupported ui_sheet_driver mode '{mode}'.")


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        fail(e)
