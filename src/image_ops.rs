//! Image post-processing, alpha/background removal, deterministic raster drawing,
//! 9-slice metadata, card packing (`art/pack-card`), and sandboxed Pillow scripts.
//!
//! Uses Python 3 + Pillow (`python3` by default, override via `OPENRO_PYTHON_BIN`).
//! All input/output paths are jailed to the project folder via `opencloud::safe_rel`.
//! Every output PNG is decoded, verified `<= max_bytes` (default 5 MB), hashed (sha256),
//! and logged to `<project>/art/ART-LOG.jsonl`.

use crate::assets::{append_art_log, sanitize_art_slug};
use crate::opencloud::{now_iso, safe_rel, Oc};
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const DEFAULT_MAX_PNG_BYTES: u64 = 5 * 1024 * 1024;

type R<T> = Result<T, Oc>;
fn bad<T>(msg: impl Into<String>) -> R<T> {
    Err(Oc { status: 400, msg: msg.into() })
}
fn err<T>(status: u16, msg: impl Into<String>) -> R<T> {
    Err(Oc { status, msg: msg.into() })
}

const SCRIPT_DENYLIST: &[&str] = &[
    "os.system",
    "os.popen",
    "subprocess",
    "socket",
    "urllib",
    "http.client",
    "requests",
    "shutil.rmtree",
    "__import__",
    "importlib",
    "ctypes",
    "eval(",
    "exec(",
];

fn python_bin() -> String {
    std::env::var("OPENRO_PYTHON_BIN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "python3".to_string())
}

fn run_python_json(script: &str, payload: &Value, cwd: &Path, timeout_s: u64) -> R<Value> {
    let payload_bytes =
        serde_json::to_vec(payload).map_err(|e| Oc { status: 500, msg: format!("encode payload: {e}") })?;
    let mut cmd = Command::new(python_bin());
    cmd.arg("-c")
        .arg(script)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| Oc { status: 503, msg: format!("could not run python3 (Pillow): {e}") })?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(&payload_bytes);
    }
    let started = Instant::now();
    let cap = Duration::from_secs(timeout_s.clamp(5, 120));
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let out = child
                    .wait_with_output()
                    .map_err(|e| Oc { status: 500, msg: format!("read python output: {e}") })?;
                if let Ok(v) = serde_json::from_slice::<Value>(&out.stdout) {
                    if v.get("ok").and_then(Value::as_bool) == Some(true) {
                        return Ok(v);
                    }
                    let msg = v
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("image processing failed");
                    return err(400, msg.to_string());
                }
                let stderr = String::from_utf8_lossy(&out.stderr);
                return err(
                    500,
                    format!(
                        "image processor exited {}: {}",
                        out.status.code().unwrap_or(-1),
                        stderr.chars().take(400).collect::<String>()
                    ),
                );
            }
            Ok(None) => {
                if started.elapsed() >= cap {
                    let _ = child.kill();
                    let _ = child.wait();
                    return err(504, format!("image processing timed out after {}s.", cap.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                return err(500, format!("wait python: {e}"));
            }
        }
    }
}

const PY_OPS_DRIVER: &str = r##"
import hashlib, io, json, math, os, sys
from collections import deque
from pathlib import Path
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageOps

def fail(msg):
    print(json.dumps({"ok": False, "error": str(msg)}))
    sys.exit(1)

def parse_color(val, default=(0, 0, 0, 0)):
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

def cover_resize(img, w, h, anchor="center"):
    sw, sh = img.size
    scale = max(w / sw, h / sh)
    nw, nh = max(1, int(round(sw * scale))), max(1, int(round(sh * scale)))
    resized = img.resize((nw, nh), Image.Resampling.LANCZOS)
    x0 = max(0, (nw - w) // 2)
    if anchor == "top":
        y0 = 0
    elif anchor == "top_third":
        y0 = max(0, min(nh - h, int((nh - h) * 0.22)))
    elif anchor == "bottom":
        y0 = max(0, nh - h)
    else:
        y0 = max(0, (nh - h) // 2)
    return resized.crop((x0, y0, x0 + w, y0 + h))

def remove_bg(img, op):
    img = img.convert("RGBA")
    w, h = img.size
    px = img.load()
    method = str(op.get("method") or "corner_flood").lower()
    tol = int(op.get("tolerance", 32))
    feather = int(op.get("feather", 1))
    def dist(a, b):
        return max(abs(a[0] - b[0]), abs(a[1] - b[1]), abs(a[2] - b[2]))
    mask = Image.new("L", (w, h), 255)
    mpx = mask.load()
    if method == "chroma":
        target = parse_color(op.get("color"), (255, 255, 255, 255))
        for y in range(h):
            for x in range(w):
                if dist(px[x, y], target) <= tol:
                    mpx[x, y] = 0
    else:
        seeds = [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)]
        bg_samples = [px[x, y] for x, y in seeds]
        visited = bytearray(w * h)
        q = deque()
        for x, y in seeds:
            idx = y * w + x
            if not visited[idx]:
                visited[idx] = 1
                q.append((x, y, px[x, y]))
        while q:
            x, y, ref = q.popleft()
            mpx[x, y] = 0
            for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
                if 0 <= nx < w and 0 <= ny < h:
                    nidx = ny * w + nx
                    if not visited[nidx]:
                        c = px[nx, ny]
                        if dist(c, ref) <= tol or any(dist(c, s) <= tol for s in bg_samples):
                            visited[nidx] = 1
                            q.append((nx, ny, ref))
    if feather > 0:
        mask = mask.filter(ImageFilter.GaussianBlur(radius=min(feather, 4)))
    out = img.copy()
    orig_a = img.getchannel("A")
    new_a = ImageChops.darker(orig_a, mask)
    out.putalpha(new_a)
    return out

def rounded_mask(img, radius):
    img = img.convert("RGBA")
    w, h = img.size
    ss = 4
    m = Image.new("L", (w * ss, h * ss), 0)
    d = ImageDraw.Draw(m)
    r = max(0, int(radius) * ss)
    d.rounded_rectangle((0, 0, w * ss - 1, h * ss - 1), radius=r, fill=255)
    m = m.resize((w, h), Image.Resampling.LANCZOS)
    out = img.copy()
    out.putalpha(ImageChops.darker(img.getchannel("A"), m))
    return out

def parse_wh(op, op_name):
    if "size" in op and op["size"] is not None:
        sz = op["size"]
        if not (isinstance(sz, (list, tuple)) and len(sz) == 2):
            fail(f"op '{op_name}': 'size' must be [width, height].")
        w, h = int(sz[0]), int(sz[1])
    elif "width" in op and "height" in op:
        w, h = int(op["width"]), int(op["height"])
    else:
        fail(f"op '{op_name}' requires 'width' and 'height' (or 'size': [width, height]).")
    if w <= 0 or h <= 0 or w > 8192 or h > 8192:
        fail(f"op '{op_name}': dimensions {w}x{h} out of range (1..8192).")
    return w, h

VALID_SHAPES = ("rect", "rounded_rect", "ellipse", "ring", "polygon", "star", "gradient")

def draw_shapes(img, op):
    shapes = op.get("shapes")
    if shapes is None:
        if op.get("kind") is not None or op.get("shape") is not None:
            shapes = [op]
        else:
            fail("op 'draw' requires 'shapes': [{kind|shape, ...}] or a flat {shape|kind, box|points|cx, fill|stroke, ...} object.")
    if not isinstance(shapes, list) or len(shapes) == 0:
        fail("op 'draw' drew 0 shapes; provide at least one shape in 'shapes' (or flat 'shape'/'kind').")
    img = img.convert("RGBA")
    w, h = img.size
    ss = 2
    layer = Image.new("RGBA", (w * ss, h * ss), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    drawn = 0
    for idx, s in enumerate(shapes):
        if not isinstance(s, dict):
            fail(f"op 'draw' shape[{idx}] must be an object.")
        raw_kind = s.get("kind") if s.get("kind") is not None else s.get("shape")
        if not raw_kind:
            fail(f"op 'draw' shape[{idx}] missing 'kind' (or 'shape'); expected one of: {', '.join(VALID_SHAPES)}.")
        kind = str(raw_kind).strip().lower()
        if kind not in VALID_SHAPES:
            fail(f"op 'draw' shape[{idx}] unknown kind '{kind}'; expected one of: {', '.join(VALID_SHAPES)}.")
        fill_raw = s.get("fill") if s.get("fill") is not None else (s.get("color") if kind != "ring" else None)
        stroke_raw = s.get("stroke") if s.get("stroke") is not None else (s.get("color") if kind == "ring" else None)
        fill = parse_color(fill_raw) if fill_raw is not None else None
        stroke = parse_color(stroke_raw) if stroke_raw is not None else None
        sw = int(s.get("stroke_width", 0)) * ss
        if kind == "gradient":
            c1 = parse_color(s.get("from"), (30, 34, 52, 255))
            c2 = parse_color(s.get("to"), (15, 18, 30, 255))
            horiz = str(s.get("direction") or "vertical").lower() == "horizontal"
            gw, gh = w * ss, h * ss
            steps = gw if horiz else gh
            for i in range(steps):
                t = i / max(1, steps - 1)
                col = tuple(int(c1[k] + (c2[k] - c1[k]) * t) for k in range(4))
                if horiz:
                    d.line([(i, 0), (i, gh)], fill=col)
                else:
                    d.line([(0, i), (gw, i)], fill=col)
            drawn += 1
        elif kind == "rect":
            if fill is None and stroke is None:
                fail(f"op 'draw' shape[{idx}] ('rect') needs 'fill' or 'stroke' (or 'color').")
            box = [int(v * ss) for v in s.get("box", [0, 0, w, h])]
            d.rectangle(box, fill=fill, outline=stroke, width=sw)
            drawn += 1
        elif kind == "rounded_rect":
            if fill is None and stroke is None:
                fail(f"op 'draw' shape[{idx}] ('rounded_rect') needs 'fill' or 'stroke' (or 'color').")
            box = [int(v * ss) for v in s.get("box", [0, 0, w, h])]
            rad = int(s.get("radius", 12)) * ss
            d.rounded_rectangle(box, radius=rad, fill=fill, outline=stroke, width=sw)
            drawn += 1
        elif kind == "ellipse":
            if fill is None and stroke is None:
                fail(f"op 'draw' shape[{idx}] ('ellipse') needs 'fill' or 'stroke' (or 'color').")
            box = [int(v * ss) for v in s.get("box", [0, 0, w, h])]
            d.ellipse(box, fill=fill, outline=stroke, width=sw)
            drawn += 1
        elif kind == "ring":
            box = [int(v * ss) for v in s.get("box", [0, 0, w, h])]
            thickness = max(2, int(s.get("thickness", 12))) * ss
            rad = int(s.get("radius", 0)) * ss
            col = stroke or fill or (255, 215, 0, 255)
            if rad > 0:
                d.rounded_rectangle(box, radius=rad, outline=col, width=thickness)
            else:
                d.rectangle(box, outline=col, width=thickness)
            drawn += 1
        elif kind == "polygon":
            pts = [(int(p[0] * ss), int(p[1] * ss)) for p in s.get("points", [])]
            if len(pts) < 3:
                fail(f"op 'draw' shape[{idx}] ('polygon') requires at least 3 [x,y] points.")
            d.polygon(pts, fill=fill, outline=stroke)
            drawn += 1
        elif kind == "star":
            cx, cy = float(s.get("cx", w / 2)) * ss, float(s.get("cy", h / 2)) * ss
            r_out = float(s.get("outer_r", min(w, h) * 0.42)) * ss
            r_in = float(s.get("inner_r", r_out * 0.44)) * ss
            n = max(3, int(s.get("points", 5)))
            pts = []
            for i in range(n * 2):
                ang = -math.pi / 2 + i * math.pi / n
                r = r_out if i % 2 == 0 else r_in
                pts.append((cx + math.cos(ang) * r, cy + math.sin(ang) * r))
            d.polygon(pts, fill=fill, outline=stroke)
            drawn += 1
    if drawn == 0:
        fail("op 'draw' drew 0 shapes; expected shapes: [{kind|shape, ...}].")
    layer = layer.resize((w, h), Image.Resampling.LANCZOS)
    return Image.alpha_composite(img, layer)

def save_and_verify(img, out_abs, max_bytes):
    out_p = Path(out_abs)
    out_p.parent.mkdir(parents=True, exist_ok=True)
    tmp = out_p.with_suffix(".png.tmp-openro")
    buf = io.BytesIO()
    img.save(buf, format="PNG", optimize=True)
    data = buf.getvalue()
    if len(data) > max_bytes:
        # Adaptive palette quantize while preserving alpha to fit max_bytes
        q = img.quantize(colors=256, method=Image.Quantize.FASTOCTREE)
        buf = io.BytesIO()
        q.save(buf, format="PNG", optimize=True)
        data = buf.getvalue()
    if len(data) > max_bytes:
        fail(f"Output PNG is {len(data)} bytes, exceeding limit {max_bytes} bytes.")
    tmp.write_bytes(data)
    os.replace(tmp, out_p)
    # Full decode check
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

def main():
    req = json.loads(sys.stdin.read() or "{}")
    mode = req.get("mode", "ops")
    max_bytes = int(req.get("max_bytes", 5 * 1024 * 1024))
    if mode == "script":
        script_src = req["script"]
        ns = {
            "Image": Image, "ImageDraw": ImageDraw, "ImageFilter": ImageFilter,
            "ImageChops": ImageChops, "ImageOps": ImageOps,
            "math": math, "json": json, "Path": Path,
            "PROJECT_ROOT": Path(req["project_root"]),
            "ART_ROOT": Path(req["art_root"]),
            "OUT_PATH": Path(req["out_abs"]) if req.get("out_abs") else None,
        }
        exec(compile(script_src, "<openro-image-script>", "exec"), ns, ns)
        out_abs = req.get("out_abs")
        if out_abs and Path(out_abs).is_file():
            with Image.open(out_abs) as im:
                im.load()
                info = save_and_verify(im.convert("RGBA"), out_abs, max_bytes)
            print(json.dumps({"ok": True, **info}))
        else:
            print(json.dumps({"ok": True, "script_ran": True}))
        return

    if mode == "pack_card":
        src = Image.open(req["source_abs"]).convert("RGBA")
        art_w, art_h = req["art_size"]
        ico_w, ico_h = req["icon_size"]
        art_img = cover_resize(src, art_w, art_h, "top_third")
        icon_img = cover_resize(src, ico_w, ico_h, "top_third")
        full_img = art_img.copy()
        if req.get("frame_abs") and Path(req["frame_abs"]).is_file():
            fr = Image.open(req["frame_abs"]).convert("RGBA").resize((art_w, art_h), Image.Resampling.LANCZOS)
            full_img = Image.alpha_composite(full_img, fr)
        info_art = save_and_verify(art_img, req["art_abs"], max_bytes)
        info_icon = save_and_verify(icon_img, req["icon_abs"], max_bytes)
        info_full = save_and_verify(full_img, req["full_abs"], max_bytes)

        # Build 4-panel contact sheet: REF | ART | ICON | FULL
        cw, ch, pad, lbl = 240, 320, 14, 28
        panels = []
        if req.get("ref_abs") and Path(req["ref_abs"]).is_file():
            panels.append(("REF", Image.open(req["ref_abs"]).convert("RGBA")))
        else:
            panels.append(("SRC", src))
        panels.extend([("ART", art_img), ("ICON", icon_img), ("FULL", full_img)])
        sheet = Image.new("RGBA", (pad + len(panels) * (cw + pad), pad * 2 + ch + lbl), (18, 22, 34, 255))
        draw = ImageDraw.Draw(sheet)
        for i, (label, pim) in enumerate(panels):
            x = pad + i * (cw + pad)
            y = pad + lbl
            fitted = ImageOps.contain(pim, (cw, ch), Image.Resampling.LANCZOS)
            ox = x + (cw - fitted.width) // 2
            oy = y + (ch - fitted.height) // 2
            draw.rectangle((x, y, x + cw, y + ch), fill=(28, 34, 50, 255), outline=(72, 86, 118, 255))
            sheet.paste(fitted, (ox, oy), fitted)
            draw.text((x + 6, pad + 6), label, fill=(235, 240, 255, 255))
        info_contact = save_and_verify(sheet, req["contact_abs"], max_bytes)
        print(json.dumps({
            "ok": True,
            "art": info_art,
            "icon": info_icon,
            "full": info_full,
            "contact": info_contact,
        }))
        return

    img = None
    if req.get("input_abs"):
        img = Image.open(req["input_abs"]).convert("RGBA")

    for op in req.get("ops", []):
        name = str(op.get("op") or "").strip().lower()
        if name == "canvas":
            w, h = parse_wh(op, "canvas")
            img = Image.new("RGBA", (w, h), parse_color(op.get("color"), (0, 0, 0, 0)))
        elif name == "contact_sheet":
            inputs = op.get("resolved_inputs") or []
            if not inputs:
                fail("op 'contact_sheet' requires a non-empty 'inputs' list.")
            cols = max(1, int(op.get("columns", 4)))
            cw, ch = [int(x) for x in op.get("cell_size", [256, 256])]
            bg = parse_color(op.get("bg"), (18, 22, 34, 255))
            rows = max(1, (len(inputs) + cols - 1) // cols)
            pad, lbl = 12, 24
            img = Image.new("RGBA", (pad + cols * (cw + pad), pad + rows * (ch + lbl + pad)), bg)
            d = ImageDraw.Draw(img)
            for idx, item in enumerate(inputs):
                r, c = divmod(idx, cols)
                x = pad + c * (cw + pad)
                y = pad + r * (ch + lbl + pad)
                im = Image.open(item["abs"]).convert("RGBA")
                fitted = ImageOps.contain(im, (cw, ch), Image.Resampling.LANCZOS)
                img.paste(fitted, (x + (cw - fitted.width) // 2, y + lbl + (ch - fitted.height) // 2), fitted)
                if item.get("label"):
                    d.text((x + 4, y + 4), str(item["label"]), fill=(235, 240, 255, 255))
        elif name not in ("cover_resize", "resize", "crop", "remove_bg", "rounded_mask", "draw", "composite", "quantize", "compress"):
            fail(f"unknown image op '{name}'; expected one of: canvas, draw, cover_resize, resize, crop, remove_bg, rounded_mask, composite, quantize, compress, contact_sheet.")
        elif img is None:
            fail(f"op '{name}' requires an 'input' image or an earlier 'canvas' op.")
        elif name == "cover_resize":
            w, h = parse_wh(op, "cover_resize")
            img = cover_resize(img, w, h, str(op.get("anchor") or "center"))
        elif name == "resize":
            w, h = parse_wh(op, "resize")
            if str(op.get("fit") or "stretch") == "contain":
                fitted = ImageOps.contain(img, (w, h), Image.Resampling.LANCZOS)
                canvas = Image.new("RGBA", (w, h), (0, 0, 0, 0))
                canvas.paste(fitted, ((w - fitted.width) // 2, (h - fitted.height) // 2), fitted)
                img = canvas
            else:
                img = img.resize((w, h), Image.Resampling.LANCZOS)
        elif name == "crop":
            w, h = parse_wh(op, "crop")
            x, y = int(op.get("x", 0)), int(op.get("y", 0))
            img = img.crop((x, y, x + w, y + h))
        elif name == "remove_bg":
            img = remove_bg(img, op)
        elif name == "rounded_mask":
            img = rounded_mask(img, int(op.get("radius", 16)))
        elif name == "draw":
            img = draw_shapes(img, op)
        elif name == "composite":
            for layer in op.get("resolved_layers") or []:
                ov = Image.open(layer["abs"]).convert("RGBA")
                if layer.get("width") and layer.get("height"):
                    ov = ov.resize((int(layer["width"]), int(layer["height"])), Image.Resampling.LANCZOS)
                opacity = float(layer.get("opacity", 1.0))
                if opacity < 1.0:
                    a = ov.getchannel("A").point(lambda p: int(p * max(0.0, min(1.0, opacity))))
                    ov.putalpha(a)
                canvas = Image.new("RGBA", img.size, (0, 0, 0, 0))
                canvas.paste(ov, (int(layer.get("x", 0)), int(layer.get("y", 0))), ov)
                img = Image.alpha_composite(img, canvas)
        elif name == "quantize":
            colors = max(2, min(256, int(op.get("colors", 256))))
            img = img.quantize(colors=colors, method=Image.Quantize.FASTOCTREE).convert("RGBA")
        elif name == "compress":
            max_bytes = int(op.get("max_bytes", max_bytes))

    if img is None:
        fail("No input image or canvas op provided.")
    info = save_and_verify(img, req["out_abs"], max_bytes)
    print(json.dumps({"ok": True, **info}))

if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        fail(e)
"##;

const PY_UI_SHEET_DRIVER: &str = include_str!("ui_sheet_driver.py");

fn art_url(project: &str, rel: &str) -> String {
    if project.is_empty() {
        format!("/api/art/image?path={rel}")
    } else {
        format!("/api/art/image?project={project}&path={rel}")
    }
}

fn slice_key_for_path(target_rel: &str) -> String {
    let norm = target_rel.trim().replace('\\', "/");
    if let Some(sub) = norm.strip_prefix("art/ui/") {
        if let Some((stem, _)) = sub.rsplit_once('.') {
            return stem.to_string();
        }
        return sub.to_string();
    }
    norm.rsplit('/')
        .next()
        .unwrap_or(&norm)
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(&norm)
        .to_string()
}

pub(crate) fn update_slices_json(root: &Path, slices_rel: &str, target_rel: &str, slice: &[i64; 4], w: u64, h: u64) -> R<String> {
    let abs = safe_rel(root, slices_rel)?;
    if let Some(p) = abs.parent() {
        std::fs::create_dir_all(p).or_else(|e| err(500, format!("create slices dir: {e}")))?;
    }
    let mut doc: Value = std::fs::read_to_string(&abs)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let key = slice_key_for_path(target_rel);
    doc[key] = json!({
        "slice": slice,
        "size": [w, h],
        "path": target_rel,
    });
    let text = serde_json::to_string_pretty(&doc).map_err(|e| Oc { status: 500, msg: format!("encode slices: {e}") })?;
    let tmp = abs.with_extension("json.tmp-openro");
    std::fs::write(&tmp, format!("{text}\n"))
        .and_then(|_| std::fs::rename(&tmp, &abs))
        .or_else(|e| err(500, format!("write {slices_rel}: {e}")))?;
    Ok(slices_rel.to_string())
}

/// `POST /api/art/process` (and `/api/tools/image/process`)
pub fn process(root: &Path, body: &Value) -> R<Value> {
    let project = body.get("project").and_then(Value::as_str).unwrap_or("");
    let max_bytes = body
        .get("max_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MAX_PNG_BYTES)
        .clamp(1024, 20 * 1024 * 1024);
    let art_dir = root.join("art");
    std::fs::create_dir_all(&art_dir).or_else(|e| err(500, format!("create art dir: {e}")))?;

    // Mode B: sandboxed Pillow script
    if let Some(script) = body.get("script").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        let lower = script.to_ascii_lowercase();
        for banned in SCRIPT_DENYLIST {
            if lower.contains(banned) {
                return bad(format!(
                    "Script rejected by safety check: '{banned}' is not allowed in image scripts."
                ));
            }
        }
        let out_rel = body.get("output").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
        let out_abs = match out_rel {
            Some(r) => Some(safe_rel(root, r)?),
            None => None,
        };
        let timeout_s = body.get("timeout_s").and_then(Value::as_u64).unwrap_or(30);
        let payload = json!({
            "mode": "script",
            "script": script,
            "project_root": root.to_string_lossy(),
            "art_root": art_dir.to_string_lossy(),
            "out_abs": out_abs.as_ref().map(|p| p.to_string_lossy().to_string()),
            "max_bytes": max_bytes,
        });
        let res = run_python_json(PY_OPS_DRIVER, &payload, &art_dir, timeout_s)?;
        if let Some(rel) = out_rel {
            let _ = append_art_log(
                root,
                &json!({
                    "at": now_iso(),
                    "action": "image_script",
                    "path": rel,
                    "width": res.get("width"),
                    "height": res.get("height"),
                    "bytes": res.get("bytes"),
                    "sha256": res.get("sha256"),
                    "opaque_ratio": res.get("opaque_ratio"),
                }),
            );
            return Ok(json!({
                "ok": true,
                "path": rel,
                "url": art_url(project, rel),
                "width": res.get("width"),
                "height": res.get("height"),
                "bytes": res.get("bytes"),
                "sha256": res.get("sha256"),
                "has_alpha": res.get("has_alpha"),
                "opaque_ratio": res.get("opaque_ratio"),
            }));
        }
        return Ok(res);
    }

    // Mode A: structured ops list
    let out_rel = body
        .get("output")
        .or_else(|| body.get("out_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Oc { status: 400, msg: "Send 'output' (project-relative PNG path) and 'ops' (or 'script').".into() })?;
    let out_abs = safe_rel(root, out_rel)?;
    let input_abs = match body.get("input").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        Some(inp) => {
            let abs = safe_rel(root, inp)?;
            if !abs.is_file() {
                return err(404, format!("Input image '{inp}' not found."));
            }
            Some(abs)
        }
        None => None,
    };

    // Resolve any nested file paths inside composite / contact_sheet ops safely via safe_rel.
    let raw_ops = body.get("ops").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut resolved_ops = Vec::with_capacity(raw_ops.len());
    for mut op in raw_ops {
        let name = op.get("op").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
        if name == "composite" {
            let mut layers = Vec::new();
            for l in op.get("layers").and_then(Value::as_array).into_iter().flatten() {
                let p = l.get("path").and_then(Value::as_str).unwrap_or("");
                let abs = safe_rel(root, p)?;
                if !abs.is_file() {
                    return err(404, format!("Composite layer '{p}' not found."));
                }
                let mut item = l.clone();
                item["abs"] = json!(abs.to_string_lossy());
                layers.push(item);
            }
            op["resolved_layers"] = json!(layers);
        } else if name == "contact_sheet" {
            let mut inputs = Vec::new();
            for inp in op.get("inputs").and_then(Value::as_array).into_iter().flatten() {
                let (p, label) = if let Some(s) = inp.as_str() {
                    (s, s.rsplit('/').next().unwrap_or(s))
                } else {
                    (
                        inp.get("path").and_then(Value::as_str).unwrap_or(""),
                        inp.get("label").and_then(Value::as_str).unwrap_or(""),
                    )
                };
                let abs = safe_rel(root, p)?;
                if !abs.is_file() {
                    return err(404, format!("Contact sheet input '{p}' not found."));
                }
                inputs.push(json!({ "abs": abs.to_string_lossy(), "label": label }));
            }
            op["resolved_inputs"] = json!(inputs);
        }
        resolved_ops.push(op);
    }

    let payload = json!({
        "mode": "ops",
        "input_abs": input_abs.as_ref().map(|p| p.to_string_lossy().to_string()),
        "out_abs": out_abs.to_string_lossy(),
        "ops": resolved_ops,
        "max_bytes": max_bytes,
    });
    let res = run_python_json(PY_OPS_DRIVER, &payload, &art_dir, 45)?;

    let mut slices_updated = Value::Null;
    if let Some(arr) = body.get("slice").and_then(Value::as_array) {
        if arr.len() == 4 {
            let sl = [
                arr[0].as_i64().unwrap_or(0),
                arr[1].as_i64().unwrap_or(0),
                arr[2].as_i64().unwrap_or(0),
                arr[3].as_i64().unwrap_or(0),
            ];
            let slices_rel = body
                .get("slices_json")
                .and_then(Value::as_str)
                .unwrap_or("art/ui/slices.json");
            let updated = update_slices_json(
                root,
                slices_rel,
                out_rel,
                &sl,
                res.get("width").and_then(Value::as_u64).unwrap_or(0),
                res.get("height").and_then(Value::as_u64).unwrap_or(0),
            )?;
            slices_updated = json!(updated);
        }
    }

    let _ = append_art_log(
        root,
        &json!({
            "at": now_iso(),
            "action": "image_process",
            "path": out_rel,
            "width": res.get("width"),
            "height": res.get("height"),
            "bytes": res.get("bytes"),
            "sha256": res.get("sha256"),
            "has_alpha": res.get("has_alpha"),
            "opaque_ratio": res.get("opaque_ratio"),
        }),
    );

    Ok(json!({
        "ok": true,
        "path": out_rel,
        "url": art_url(project, out_rel),
        "width": res.get("width"),
        "height": res.get("height"),
        "bytes": res.get("bytes"),
        "sha256": res.get("sha256"),
        "has_alpha": res.get("has_alpha"),
        "opaque_ratio": res.get("opaque_ratio"),
        "slices_updated": slices_updated,
    }))
}

/// `POST /api/art/pack-card` — one-shot card art packing:
/// produces `art.png` (512x712), `icon.png` (256x256 upper-centre), `full.png` (frame composite),
/// plus an uncommitted `art/previews/<card_id>_contact.png` (`REF | ART | ICON | FULL`).
pub fn pack_card(root: &Path, body: &Value) -> R<Value> {
    let project = body.get("project").and_then(Value::as_str).unwrap_or("");
    let raw_id = body.get("card_id").and_then(Value::as_str).unwrap_or("").trim();
    if raw_id.is_empty() {
        return bad("Send 'card_id' and 'source_image' (project-relative path).");
    }
    let card_id = sanitize_art_slug(raw_id);
    let src_rel = body
        .get("source_image")
        .or_else(|| body.get("input"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Oc { status: 400, msg: "Send 'source_image' (project-relative path).".into() })?;
    let src_abs = safe_rel(root, src_rel)?;
    if !src_abs.is_file() {
        return err(404, format!("source_image '{src_rel}' not found."));
    }

    let ref_abs = match body.get("reference_image").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        Some(r) => Some(safe_rel(root, r)?),
        None => None,
    };
    let frame_abs = match body.get("frame_image").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        Some(f) => {
            let abs = safe_rel(root, f)?;
            if !abs.is_file() {
                return err(404, format!("frame_image '{f}' not found."));
            }
            Some(abs)
        }
        None => None,
    };

    let base_dir = body
        .get("out_dir")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|| format!("art/cards/{card_id}"));
    let art_rel = format!("{base_dir}/art.png");
    let icon_rel = format!("{base_dir}/icon.png");
    let full_rel = format!("{base_dir}/full.png");
    let contact_rel = format!("art/previews/{card_id}_contact.png");

    let art_abs = safe_rel(root, &art_rel)?;
    let icon_abs = safe_rel(root, &icon_rel)?;
    let full_abs = safe_rel(root, &full_rel)?;
    let contact_abs = safe_rel(root, &contact_rel)?;

    let pair = |key: &str, def: [u64; 2]| -> [u64; 2] {
        body.get(key)
            .and_then(Value::as_array)
            .filter(|a| a.len() == 2)
            .map(|a| [a[0].as_u64().unwrap_or(def[0]), a[1].as_u64().unwrap_or(def[1])])
            .unwrap_or(def)
    };
    let art_size = pair("art_size", [512, 712]);
    let icon_size = pair("icon_size", [256, 256]);
    let max_bytes = body
        .get("max_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MAX_PNG_BYTES);

    let art_dir = root.join("art");
    std::fs::create_dir_all(&art_dir).or_else(|e| err(500, format!("create art dir: {e}")))?;
    let payload = json!({
        "mode": "pack_card",
        "source_abs": src_abs.to_string_lossy(),
        "ref_abs": ref_abs.as_ref().map(|p| p.to_string_lossy().to_string()),
        "frame_abs": frame_abs.as_ref().map(|p| p.to_string_lossy().to_string()),
        "art_abs": art_abs.to_string_lossy(),
        "icon_abs": icon_abs.to_string_lossy(),
        "full_abs": full_abs.to_string_lossy(),
        "contact_abs": contact_abs.to_string_lossy(),
        "art_size": art_size,
        "icon_size": icon_size,
        "max_bytes": max_bytes,
    });
    let res = run_python_json(PY_OPS_DRIVER, &payload, &art_dir, 45)?;

    let _ = append_art_log(
        root,
        &json!({
            "at": now_iso(),
            "action": "pack_card",
            "card_id": card_id,
            "art": art_rel,
            "icon": icon_rel,
            "full": full_rel,
            "contact": contact_rel,
            "art_sha256": res.pointer("/art/sha256"),
            "icon_sha256": res.pointer("/icon/sha256"),
            "full_sha256": res.pointer("/full/sha256"),
        }),
    );

    Ok(json!({
        "ok": true,
        "card_id": card_id,
        "art": { "path": art_rel, "url": art_url(project, &art_rel), "info": res.get("art") },
        "icon": { "path": icon_rel, "url": art_url(project, &icon_rel), "info": res.get("icon") },
        "full": { "path": full_rel, "url": art_url(project, &full_rel), "info": res.get("full") },
        "contact_sheet": { "path": contact_rel, "url": art_url(project, &contact_rel), "info": res.get("contact") },
    }))
}

/// Deterministic 2x-supersampled Pillow UI sheet renderer (used for offline/CI `procedural_fallback`).
pub fn render_procedural_ui_sheet(
    root: &Path,
    sheet: &str,
    out_rel: &str,
    palette: &Value,
    chroma: &str,
) -> R<Value> {
    let out_abs = safe_rel(root, out_rel)?;
    let art_dir = root.join("art");
    std::fs::create_dir_all(&art_dir).or_else(|e| err(500, format!("create art dir: {e}")))?;
    let payload = json!({
        "mode": "render_ui_sheet",
        "sheet": sheet,
        "out_abs": out_abs.to_string_lossy(),
        "palette": palette,
        "chroma": chroma,
        "max_bytes": DEFAULT_MAX_PNG_BYTES,
    });
    let res = run_python_json(PY_UI_SHEET_DRIVER, &payload, &art_dir, 60)?;
    let _ = append_art_log(
        root,
        &json!({
            "at": now_iso(),
            "action": "render_ui_sheet",
            "sheet": sheet,
            "path": out_rel,
            "width": res.get("width"),
            "height": res.get("height"),
            "bytes": res.get("bytes"),
            "sha256": res.get("sha256"),
        }),
    );
    Ok(json!({
        "ok": true,
        "path": out_rel,
        "width": res.get("width"),
        "height": res.get("height"),
        "bytes": res.get("bytes"),
        "sha256": res.get("sha256"),
    }))
}

/// Auto-slice a UI asset sheet PNG into individual de-fringed transparent PNGs under `art/ui/<skin>/`,
/// auto-compute 9-slice `SliceCenter` rects for panels/crests/buttons/bars/frames, update
/// `art/ui/slices.json`, verify `<= 5 MB` + decode + sha256, and log to `art/ART-LOG.jsonl`.
pub fn slice_ui_sheet(root: &Path, body: &Value) -> R<Value> {
    let project = body.get("project").and_then(Value::as_str).unwrap_or("");
    let src_rel = body
        .get("input")
        .or_else(|| body.get("sheet_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Oc {
            status: 400,
            msg: "Send 'input' (or 'sheet_path') pointing to a project-relative PNG sheet.".into(),
        })?;
    let src_abs = safe_rel(root, src_rel)?;
    if !src_abs.is_file() {
        return err(404, format!("Input UI sheet '{src_rel}' not found."));
    }

    let raw_skin = body.get("skin").and_then(Value::as_str).unwrap_or("obsidian-prism");
    let skin = sanitize_art_slug(raw_skin).replace('_', "-");
    let out_dir_rel = body
        .get("out_dir")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|| format!("art/ui/{skin}"));
    let out_dir_abs = safe_rel(root, &out_dir_rel)?;
    std::fs::create_dir_all(&out_dir_abs).or_else(|e| err(500, format!("create ui out dir: {e}")))?;

    let slices_rel = body
        .get("slices_json")
        .and_then(Value::as_str)
        .unwrap_or("art/ui/slices.json");

    let items: Vec<Value> = if let Some(arr) = body.get("items").and_then(Value::as_array) {
        arr.clone()
    } else {
        let sheet_id = body.get("sheet").and_then(Value::as_str).unwrap_or("chrome_sheet");
        crate::ui_kit::slice_items_for_recipe(sheet_id)
            .map_err(|msg| Oc { status: 400, msg })?
    };
    if items.is_empty() {
        return bad("No slice items provided or resolved for UI sheet.");
    }

    let max_bytes = body
        .get("max_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MAX_PNG_BYTES)
        .clamp(1024, 20 * 1024 * 1024);

    let art_dir = root.join("art");
    std::fs::create_dir_all(&art_dir).or_else(|e| err(500, format!("create art dir: {e}")))?;

    let payload = json!({
        "mode": "slice_sheet",
        "input_abs": src_abs.to_string_lossy(),
        "out_dir_abs": out_dir_abs.to_string_lossy(),
        "out_dir_rel": out_dir_rel,
        "items": items,
        "bg_color": body.get("bg_color").cloned().unwrap_or(Value::Null),
        "method": body.get("method").and_then(Value::as_str).unwrap_or("auto"),
        "tolerance": body.get("tolerance").and_then(Value::as_i64).unwrap_or(42),
        "defringe": body.get("defringe").and_then(Value::as_bool).unwrap_or(true),
        "max_bytes": max_bytes,
    });
    let res = run_python_json(PY_UI_SHEET_DRIVER, &payload, &art_dir, 60)?;

    let sliced_items = res
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out_items = Vec::with_capacity(sliced_items.len());
    let mut slice_count = 0usize;

    for mut item in sliced_items {
        let item_path = item.get("path").and_then(Value::as_str).unwrap_or("").to_string();
        let w = item.get("width").and_then(Value::as_u64).unwrap_or(0);
        let h = item.get("height").and_then(Value::as_u64).unwrap_or(0);
        if item.get("is_9slice").and_then(Value::as_bool) == Some(true) {
            if let Some(arr) = item.get("slice").and_then(Value::as_array) {
                if arr.len() == 4 {
                    let sl = [
                        arr[0].as_i64().unwrap_or(16),
                        arr[1].as_i64().unwrap_or(16),
                        arr[2].as_i64().unwrap_or((w as i64).saturating_sub(16)),
                        arr[3].as_i64().unwrap_or((h as i64).saturating_sub(16)),
                    ];
                    update_slices_json(root, slices_rel, &item_path, &sl, w, h)?;
                    slice_count += 1;
                }
            }
        }
        item["url"] = json!(art_url(project, &item_path));
        let _ = append_art_log(
            root,
            &json!({
                "at": now_iso(),
                "action": "slice_ui_sheet",
                "skin": skin,
                "path": item_path,
                "width": w,
                "height": h,
                "bytes": item.get("bytes"),
                "sha256": item.get("sha256"),
                "has_alpha": item.get("has_alpha"),
                "opaque_ratio": item.get("opaque_ratio"),
                "slice": item.get("slice"),
            }),
        );
        out_items.push(item);
    }

    Ok(json!({
        "ok": true,
        "skin": skin,
        "input": src_rel,
        "out_dir": out_dir_rel,
        "slices_json": if slice_count > 0 { json!(slices_rel) } else { Value::Null },
        "sliced_count": out_items.len(),
        "nine_slice_count": slice_count,
        "sliced": out_items,
    }))
}
