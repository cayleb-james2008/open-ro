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


def _fg_mask_bytes(src, bg_color, tol):
    sw, sh = src.size
    rgba = src.convert("RGBA")
    alpha = rgba.getchannel("A")
    if bg_color[3] <= 12:
        fg = alpha.point(lambda a: 255 if a > 16 else 0)
    else:
        diff_rgb = ImageChops.difference(rgba.convert("RGB"), Image.new("RGB", (sw, sh), bg_color[:3]))
        dr, dg, db = diff_rgb.split()
        max_diff = ImageChops.lighter(dr, ImageChops.lighter(dg, db))
        non_bg = max_diff.point(lambda d: 255 if d > tol else 0)
        non_tr = alpha.point(lambda a: 255 if a > 16 else 0)
        fg = ImageChops.darker(non_bg, non_tr)
    return fg.tobytes(), sw, sh


def _grid_seam_clashes(fg_bytes, sw, sh, rows, cols):
    for r in range(1, rows):
        yc = int(round(r * sh / rows))
        hits = 0
        samples = 0
        for y in range(max(0, yc - 1), min(sh, yc + 2)):
            row = fg_bytes[y * sw : (y + 1) * sw]
            hits += sum(1 for v in row if v)
            samples += sw
        if samples > 0 and (hits / samples) > 0.015:
            return True
    for c in range(1, cols):
        xc = int(round(c * sw / cols))
        hits = 0
        samples = 0
        for x in range(max(0, xc - 1), min(sw, xc + 2)):
            hits += sum(1 for y in range(sh) if fg_bytes[y * sw + x])
            samples += sh
        if samples > 0 and (hits / samples) > 0.015:
            return True
    return False


def _merge_runs(active_flags, max_gap, min_span):
    runs = []
    start = None
    gap = 0
    for i, on in enumerate(active_flags):
        if on:
            if start is None:
                start = i
            gap = 0
        elif start is not None:
            gap += 1
            if gap > max_gap:
                end = i - gap
                if (end - start + 1) >= min_span:
                    runs.append((start, end))
                start = None
                gap = 0
    if start is not None:
        end = len(active_flags) - 1 - gap
        if (end - start + 1) >= min_span:
            runs.append((start, end))
    return runs


def detect_sheet_boxes(src, bg_color, tol, expected_count, default_rows, default_cols):
    fg_bytes, sw, sh = _fg_mask_bytes(src, bg_color, tol)
    if not _grid_seam_clashes(fg_bytes, sw, sh, default_rows, default_cols):
        return None
    row_has = [any(fg_bytes[y * sw : (y + 1) * sw]) for y in range(sh)]
    ybands = _merge_runs(row_has, max_gap=max(6, sh // 120), min_span=16)
    min_area = max(256, int(sw * sh * 0.0015))
    boxes = []
    for by0, by1 in ybands:
        col_has = [any(fg_bytes[y * sw + x] for y in range(by0, by1 + 1)) for x in range(sw)]
        xbands = _merge_runs(col_has, max_gap=max(6, sw // 120), min_span=16)
        for bx0, bx1 in xbands:
            ys = [y for y in range(by0, by1 + 1) if any(fg_bytes[y * sw + bx0 : y * sw + bx1 + 1])]
            if not ys:
                continue
            sy0, sy1 = ys[0], ys[-1]
            if (bx1 - bx0 + 1) * (sy1 - sy0 + 1) < min_area:
                continue
            boxes.append([max(0, bx0 - 4), max(0, sy0 - 4), min(sw, bx1 + 5), min(sh, sy1 + 5)])
    if len(boxes) == expected_count:
        return boxes
    return None


def fit_to_target(cropped, target_size, pad_px=4, tight=False):
    cw, ch = cropped.size
    if target_size:
        max_tw, max_th = int(target_size[0]), int(target_size[1])
        if tight:
            inner_max_w = max(8, max_tw - pad_px * 2)
            inner_max_h = max(8, max_th - pad_px * 2)
            if cw > inner_max_w or ch > inner_max_h:
                fitted = ImageOps.contain(cropped, (inner_max_w, inner_max_h), Image.Resampling.LANCZOS)
            else:
                fitted = cropped
            tw = max(32, ((fitted.width + pad_px * 2 + 1) // 2) * 2)
            th = max(28, ((fitted.height + pad_px * 2 + 1) // 2) * 2)
        else:
            tw, th = max_tw, max_th
            inner_w = max(8, tw - pad_px * 2)
            inner_h = max(8, th - pad_px * 2)
            fitted = ImageOps.contain(cropped, (inner_w, inner_h), Image.Resampling.LANCZOS)
    else:
        tw = max(32, ((cw + pad_px * 2 + 1) // 2) * 2)
        th = max(28 if tight else 32, ((ch + pad_px * 2 + 1) // 2) * 2)
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
    else:
        bg_color = parse_color(bg_color, (0, 255, 0, 255))
    tol = int(req.get("tolerance", 42))
    method = str(req.get("method") or "auto").lower()
    defringe = bool(req.get("defringe", True))
    max_bytes = int(req.get("max_bytes", 5 * 1024 * 1024))
    out_dir = Path(req["out_dir_abs"])
    out_dir.mkdir(parents=True, exist_ok=True)
    out_dir_rel = req["out_dir_rel"].rstrip("/")

    items = req["items"]
    auto_boxes = None
    if items and not any(it.get("box") for it in items):
        default_rows = max(1, int(items[0].get("rows", req.get("rows", 2))))
        default_cols = max(1, int(items[0].get("cols", req.get("cols", 4))))
        auto_boxes = detect_sheet_boxes(src, bg_color, tol, len(items), default_rows, default_cols)

    results = []
    for idx, item in enumerate(items):
        name = item["name"]
        if item.get("box"):
            bx = item["box"]
            x0, y0, x1, y1 = int(bx[0]), int(bx[1]), int(bx[2]), int(bx[3])
        elif auto_boxes is not None:
            x0, y0, x1, y1 = auto_boxes[idx]
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
        is_9slice = bool(item.get("is_9slice", False))
        tight_fit = bool(item.get("tight_fit", is_9slice and not cutout_interior))
        target_size = item.get("target_size")
        default_pad = 2 if tight_fit else 4
        pad_px = int(item.get("pad_px", default_pad))
        final_img = fit_to_target(cropped, target_size, pad_px=pad_px, tight=tight_fit)
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
    elif sheet == "panels_sheet":
        # Gap #91: Image-Gen-First panels sheet fallback (4x2, 8 crisp 9-slice plates).
        w, h = 2048, 1024
        cols, rows = 4, 2
        cw, ch = (w * ss) // cols, (h * ss) // rows
        canvas = Image.new("RGBA", (w * ss, h * ss), chroma)
        d = ImageDraw.Draw(canvas)
        white = (255, 255, 255, 255)
        obsidian = (10, 13, 20, 255)
        obsidian_hi = (28, 35, 52, 255)
        emerald_hi, emerald_lo = (52, 211, 153, 255), (4, 120, 87, 255)
        gold_hi, gold_lo = (252, 211, 77, 255), (161, 98, 7, 255)
        titan_hi, titan_lo = (100, 116, 139, 255), (30, 41, 59, 255)

        def cell_box(col, row, bw, bh):
            mx, my = col * cw + cw // 2, row * ch + ch // 2
            return (mx - bw // 2, my - bh // 2, mx + bw // 2, my + bh // 2)

        def glossy_plate(box, radius, top, bot, rim_col, rim_w, sheen=True):
            # Rounded-mask vertical gradient so no square fill corners leak past the bevel.
            x0, y0, x1, y1 = box
            bw, bh = x1 - x0, y1 - y0
            grad = Image.new("RGBA", (bw, bh))
            gd = ImageDraw.Draw(grad)
            for yy in range(bh):
                gd.line([(0, yy), (bw, yy)], fill=lerp_col(top, bot, yy / max(1, bh - 1)))
            mask = Image.new("L", (bw, bh), 0)
            ImageDraw.Draw(mask).rounded_rectangle((0, 0, bw - 1, bh - 1), radius=radius, fill=255)
            canvas.paste(grad, (x0, y0), mask)
            d.rounded_rectangle(box, radius=radius, outline=stroke, width=rim_w + 2)
            ins = rim_w + 2
            d.rounded_rectangle((x0 + ins, y0 + ins, x1 - ins, y1 - ins), radius=max(4, radius - ins), outline=rim_col, width=rim_w)
            if sheen:
                d.rounded_rectangle(
                    (x0 + radius // 2 + 16, y0 + ins + rim_w + 10, x1 - radius // 2 - 16, y0 + ins + rim_w + 10 + max(10, bh // 4)),
                    radius=max(6, bh // 8),
                    fill=lerp_col(top, white, 0.38),
                )

        def brushed_lines(box, top, bot):
            x0, y0, x1, y1 = box
            for k in range(1, 7):
                yy = y0 + (y1 - y0) * (k + 2) // 10
                d.line([(x0 + 48, yy), (x1 - 48, yy)], fill=lerp_col(top, bot, 0.18 + 0.08 * (k % 2)), width=2)

        # (0,0) modal_glass: jet-black obsidian glass + thin neon-gradient outer rim glow
        box = cell_box(0, 0, 880, 550)
        glossy_plate(box, 44, obsidian_hi, obsidian, accent, 5, sheen=False)
        x0, y0, x1, y1 = box
        d.rounded_rectangle((x0 + 4, y0 + 4, x1 - 4, y1 - 4), radius=40, outline=lerp_col(accent, crest_trim, 0.5), width=3)
        d.rounded_rectangle((x0 + 18, y0 + 18, x1 - 18, y1 - 18), radius=30, outline=lerp_col(crest_trim, obsidian, 0.55), width=2)
        d.line([(x0 + 80, y0 + 38), (x1 - 80, y0 + 38)], fill=lerp_col(obsidian_hi, white, 0.35), width=3)

        # (1,0) title_ribbon: breaking-out diagonal slash-crest banner + radiant geometric flare (original)
        cx = cw
        mx, my = cx + cw // 2, ch // 2
        bx0, bx1 = cx + 60, cx + cw - 60
        ribbon = [(bx0 + 80, my - 115), (bx1, my - 115), (bx1 - 80, my + 115), (bx0, my + 115)]
        d.polygon(ribbon, fill=crest_fill, outline=stroke)
        inner = [(bx0 + 108, my - 90), (bx1 - 30, my - 90), (bx1 - 102, my + 90), (bx0 + 36, my + 90)]
        d.polygon(inner, fill=lerp_col(crest_fill, accent, 0.4), outline=crest_trim)
        d.line(ribbon + [ribbon[0]], fill=crest_trim, width=8)
        d.polygon([(bx0 + 150, my - 78), (bx1 - 40, my - 78), (bx1 - 52, my - 50), (bx0 + 138, my - 50)], fill=lerp_col(crest_fill, white, 0.3))
        fx, fy = bx0 + 128, my
        for k in range(3):
            off = 26 + k * 34
            d.polygon([(fx - off - 22, fy - 86 + k * 8), (fx - off + 18, fy), (fx - off - 22, fy + 86 - k * 8), (fx - off - 4, fy)], fill=lerp_col(crest_trim, accent, k / 2.0), outline=stroke)
        diamond = [(fx, fy - 70), (fx + 46, fy), (fx, fy + 70), (fx - 46, fy)]
        d.polygon(diamond, fill=accent, outline=stroke)
        d.line(diamond + [diamond[0]], fill=crest_trim, width=5)

        # (2,0) tab_active: brushed metallic gold segmented filter tab plate
        box = cell_box(2, 0, 840, 315)
        glossy_plate(box, 30, gold_hi, gold_lo, (254, 243, 199, 255), 6)
        brushed_lines(box, gold_hi, gold_lo)

        # (3,0) tab_inactive: brushed dark titanium/steel tab plate
        box = cell_box(3, 0, 840, 315)
        glossy_plate(box, 30, titan_hi, titan_lo, (148, 163, 184, 255), 5, sheen=False)
        brushed_lines(box, titan_hi, titan_lo)

        # (0,1) btn_emerald / (1,1) btn_gold: glossy CTA buttons with top specular sheen
        glossy_plate(cell_box(0, 1, 860, 300), 44, emerald_hi, emerald_lo, (167, 243, 208, 255), 6)
        glossy_plate(cell_box(1, 1, 860, 300), 44, gold_hi, gold_lo, (254, 243, 199, 255), 6)

        # (2,1) dock_tile: glossy obsidian square hotkey tile + diagonal glass reflection
        box = cell_box(2, 1, 700, 700)
        glossy_plate(box, 64, obsidian_hi, obsidian, rim, 7, sheen=False)
        x0, y0, x1, y1 = box
        d.polygon([(x0 + 40, y0 + 40), (x0 + 360, y0 + 40), (x0 + 40, y0 + 360)], fill=lerp_col(obsidian_hi, white, 0.16))
        d.rounded_rectangle((x0 + 22, y0 + 22, x1 - 22, y1 - 22), radius=48, outline=lerp_col(crest_trim, obsidian, 0.3), width=3)

        # (3,1) inspector_card: dark translucent detail sub-panel with glowing accent edge
        box = cell_box(3, 1, 840, 610)
        glossy_plate(box, 32, elevated, surface, accent, 5, sheen=False)
        x0, y0, x1, y1 = box
        d.line([(x0 + 28, y0 + 64), (x0 + 28, y1 - 64)], fill=crest_trim, width=10)
        d.rounded_rectangle((x0 + 18, y0 + 18, x1 - 18, y1 - 18), radius=20, outline=lerp_col(accent, surface, 0.5), width=2)

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
