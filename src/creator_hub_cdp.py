#!/usr/bin/env python3
"""OpenRO Creator Hub CDP automation helper (#68).

Connects ONLY to a local loopback Chrome/Chromium DevTools endpoint (CDP), opens
a dedicated Creator Hub tab (never hijacking existing user tabs), never reads
`.ROBLOSECURITY` cookies, never prints raw Open Cloud API keys to stdout/stderr,
and never attempts to bypass login, 2FA, OTP, passkeys, or CAPTCHAs.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
from typing import Any
from urllib.parse import urlparse


CREATIONS_URL = "https://create.roblox.com/dashboard/creations"
CREDENTIALS_URL = "https://create.roblox.com/dashboard/credentials"
EXP_BASE_URL = "https://create.roblox.com/dashboard/creations/experiences"

DEFAULT_SCOPES = [
    "asset:read",
    "asset:write",
    "universe-places:write",
    "game-pass:read",
    "game-pass:write",
    "developer-product:read",
    "developer-product:write",
    "universe:read",
    "universe:write",
]

# Maps OpenRO scope names -> (Creator Hub API System option name, operation keywords)
SCOPE_TO_SYSTEM: dict[str, tuple[str, list[str]]] = {
    "asset:read": ("assets", ["read"]),
    "asset:write": ("assets", ["write"]),
    "universe-places:write": ("universe-places", ["write"]),
    "game-pass:read": ("game-passes", ["read"]),
    "game-pass:write": ("game-passes", ["write"]),
    "developer-product:read": ("developer-products", ["read"]),
    "developer-product:write": ("developer-products", ["write"]),
    "universe:read": ("universe", ["read"]),
    "universe:write": ("universe", ["write"]),
}


def emit(obj: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def validate_loopback_cdp(url: str) -> str:
    parsed = urlparse(url.strip())
    if parsed.scheme != "http":
        raise ValueError("CDP URL must use http:// on loopback.")
    if parsed.username or parsed.password:
        raise ValueError("CDP URL must not contain credentials.")
    host = (parsed.hostname or "").lower()
    if host not in ("127.0.0.1", "localhost", "::1"):
        raise ValueError(f"CDP URL host '{host}' is not loopback.")
    if not parsed.port or not (1 <= parsed.port <= 65535):
        raise ValueError("CDP URL must include an explicit port.")
    return f"http://{host}:{parsed.port}" if host != "::1" else f"http://[::1]:{parsed.port}"


def read_stdin_spec() -> dict[str, Any]:
    if sys.stdin is None or sys.stdin.isatty():
        return {}
    raw = sys.stdin.read().strip()
    if not raw:
        return {}
    data = json.loads(raw)
    return data if isinstance(data, dict) else {}


def detect_blocker(page: Any) -> dict[str, Any] | None:
    """Detect login, 2FA/OTP/passkey, or CAPTCHA walls. Never attempts bypass."""
    url = (page.url or "").lower()
    try:
        info = page.evaluate(
            """() => {
                const bodyText = (document.body && document.body.innerText || '').slice(0, 6000);
                const iframes = Array.from(document.querySelectorAll('iframe')).map(f => (f.src || '') + ' ' + (f.title || ''));
                const hasLoginInput = Boolean(
                    document.querySelector('#login-username, input[name="username"], input[autocomplete="username"], input[type="password"]')
                );
                const hasOtpInput = Boolean(
                    document.querySelector('#two-step-verification-code-input, input[autocomplete="one-time-code"], input[name*="code" i][maxlength="6"]')
                );
                return { bodyText, iframes, hasLoginInput, hasOtpInput };
            }"""
        )
    except Exception:
        info = {"bodyText": "", "iframes": [], "hasLoginInput": False, "hasOtpInput": False}

    text = (info.get("bodyText") or "").lower()
    iframe_blob = " ".join(info.get("iframes") or []).lower()

    captcha_markers = (
        "arkoselabs",
        "funcaptcha",
        "hcaptcha",
        "recaptcha",
        "challenges.cloudflare.com",
        "turnstile",
    )
    captcha_text = (
        "verify you are human",
        "complete the puzzle",
        "solve this puzzle",
        "pick the",
        "verification challenge",
    )
    if any(m in iframe_blob or m in url for m in captcha_markers) or any(
        t in text for t in captcha_text
    ):
        return {
            "ok": False,
            "status": "action_required",
            "blocked_reason": "captcha_required",
            "url": page.url,
            "message": "Roblox showed a CAPTCHA / human verification challenge. Complete it in the open Creator Hub tab, then retry.",
        }

    tfa_url_markers = ("/2sv", "/two-step", "/security-verification", "/challenge")
    tfa_text_markers = (
        "2-step verification",
        "two-step verification",
        "enter the 6-digit code",
        "enter the code sent",
        "authenticator app",
        "security key",
        "use a passkey",
        "verify your identity",
        "one-time code",
        "confirm it's you",
    )
    if (
        info.get("hasOtpInput")
        or any(m in url for m in tfa_url_markers)
        or any(m in text for m in tfa_text_markers)
    ):
        return {
            "ok": False,
            "status": "action_required",
            "blocked_reason": "2fa_required",
            "url": page.url,
            "message": "Roblox requested 2-step verification / re-authentication. Complete it in the open Creator Hub tab, then retry.",
        }

    login_url_markers = ("roblox.com/login", "auth.roblox.com", "/newlogin")
    if (
        any(m in url for m in login_url_markers)
        or info.get("hasLoginInput")
        or ("log in to roblox" in text and "create.roblox.com/dashboard" not in url)
    ):
        return {
            "ok": False,
            "status": "action_required",
            "blocked_reason": "login_required",
            "url": page.url,
            "message": "Sign in to Roblox Creator Hub in the open browser tab, then retry.",
        }

    return None


def fetch_json_in_page(page: Any, url: str) -> dict[str, Any] | None:
    """Invoke a browser-managed credentialed fetch without ever inspecting cookies."""
    try:
        res = page.evaluate(
            """async (targetUrl) => {
                try {
                    const r = await fetch(targetUrl, { credentials: 'include' });
                    if (!r.ok) return { _status: r.status };
                    const j = await r.json();
                    return { _status: r.status, data: j };
                } catch (e) {
                    return { _status: 0, _err: String(e) };
                }
            }""",
            url,
        )
        if isinstance(res, dict) and res.get("_status") == 200 and isinstance(res.get("data"), dict):
            return res["data"]
    except Exception:
        return None
    return None


def fetch_authenticated_user(page: Any) -> dict[str, str] | None:
    data = fetch_json_in_page(page, "https://users.roblox.com/v1/users/authenticated")
    if not data or data.get("id") is None:
        return None
    return {
        "id": str(data["id"]),
        "name": str(data.get("name") or ""),
        "displayName": str(data.get("displayName") or data.get("name") or ""),
    }


def fetch_experiences(page: Any, user_id: str) -> list[dict[str, Any]]:
    by_id: dict[str, dict[str, Any]] = {}

    # 1. Authenticated Creator Hub / Develop APIs (browser session managed internally)
    for api_url in (
        "https://develop.roblox.com/v1/user/universes?isArchived=false&limit=50&sortOrder=Desc",
        f"https://apis.roblox.com/universes/v1/search?CreatorType=User&CreatorTargetId={user_id}&IsArchived=false&PageSize=50&SortParam=LastUpdated&SortOrder=Desc",
    ):
        payload = fetch_json_in_page(page, api_url)
        items = payload.get("data") if isinstance(payload, dict) else None
        if isinstance(items, list):
            for item in items:
                if not isinstance(item, dict):
                    continue
                uid = item.get("id") or item.get("universeId")
                if uid is None:
                    continue
                uid_s = str(uid)
                rp = item.get("rootPlaceId")
                rp_s = str(rp) if rp is not None else None
                name = str(item.get("name") or "").strip()
                cur = by_id.get(uid_s)
                if cur is None:
                    by_id[uid_s] = {"universeId": uid_s, "rootPlaceId": rp_s, "name": name}
                else:
                    if not cur.get("rootPlaceId") and rp_s:
                        cur["rootPlaceId"] = rp_s
                    if not cur.get("name") and name:
                        cur["name"] = name

    # 2. Fallback / supplement from Creator Hub /dashboard/creations DOM links
    try:
        dom_items = page.evaluate(
            """() => {
                const out = [];
                for (const a of document.querySelectorAll('a[href*="/dashboard/creations/experiences/"]')) {
                    const href = a.getAttribute('href') || '';
                    const m = href.match(/\\/experiences\\/(\\d+)/);
                    if (!m) continue;
                    const text = (a.innerText || a.getAttribute('aria-label') || '').trim().split('\\n')[0].trim();
                    out.push({ universeId: m[1], name: text });
                }
                return out;
            }"""
        )
        if isinstance(dom_items, list):
            for item in dom_items:
                uid_s = str(item.get("universeId") or "").strip()
                if not uid_s.isdigit():
                    continue
                name = str(item.get("name") or "").strip()
                if uid_s not in by_id:
                    by_id[uid_s] = {"universeId": uid_s, "rootPlaceId": None, "name": name}
                elif not by_id[uid_s].get("name") and name:
                    by_id[uid_s]["name"] = name
    except Exception:
        pass

    return list(by_id.values())


def resolve_root_place_id(page: Any, universe_id: str) -> str | None:
    payload = fetch_json_in_page(
        page, f"https://develop.roblox.com/v1/universes/{universe_id}/places?limit=10"
    )
    items = payload.get("data") if isinstance(payload, dict) else None
    if isinstance(items, list):
        for item in items:
            if isinstance(item, dict) and item.get("id") is not None:
                return str(item["id"])

    # Fallback: visit the Creator Hub Places page and inspect /places/<placeId>/configure links
    try:
        page.goto(
            f"{EXP_BASE_URL}/{universe_id}/places",
            wait_until="domcontentloaded",
            timeout=25000,
        )
        page.wait_for_timeout(1500)
        pid = page.evaluate(
            """() => {
                for (const a of document.querySelectorAll('a[href*="/places/"]')) {
                    const m = (a.getAttribute('href') || '').match(/\\/places\\/(\\d+)/);
                    if (m) return m[1];
                }
                return null;
            }"""
        )
        if pid and str(pid).isdigit():
            return str(pid)
    except Exception:
        pass
    return None


def pick_experience(
    experiences: list[dict[str, Any]],
    reuse_universe_id: str | None,
    existing_universe_id: str | None,
    experience_name: str | None,
) -> dict[str, Any] | None:
    for want_id in (reuse_universe_id, existing_universe_id):
        if want_id:
            for exp in experiences:
                if str(exp.get("universeId")) == str(want_id):
                    return exp
            return {"universeId": str(want_id), "rootPlaceId": None, "name": experience_name or ""}

    if experience_name:
        target_low = experience_name.strip().lower()
        for exp in experiences:
            if str(exp.get("name") or "").strip().lower() == target_low:
                return exp

    for exp in experiences:
        if "untitled experience" in str(exp.get("name") or "").strip().lower():
            return exp

    return experiences[0] if experiences else None


def group_scopes_by_system(scopes: list[str]) -> list[tuple[str, list[str]]]:
    order: list[str] = []
    grouped: dict[str, list[str]] = {}
    for raw in scopes or DEFAULT_SCOPES:
        sc = str(raw).strip().lower()
        if sc in SCOPE_TO_SYSTEM:
            sys_name, ops = SCOPE_TO_SYSTEM[sc]
        elif ":" in sc:
            sys_name, op = sc.split(":", 1)
            ops = [op]
        else:
            continue
        if sys_name not in grouped:
            order.append(sys_name)
            grouped[sys_name] = []
        for op in ops:
            if op not in grouped[sys_name]:
                grouped[sys_name].append(op)
    return [(s, grouped[s]) for s in order]


def select_autocomplete_option(page: Any, input_loc: Any, query: str) -> bool:
    """Click an MUI/Creator Hub combobox input, type query, and pick the matching option."""
    try:
        input_loc.scroll_into_view_if_needed(timeout=4000)
        input_loc.click(timeout=4000)
        input_loc.fill("")
        input_loc.type(query, delay=20)
        page.wait_for_timeout(450)
        options = page.locator('[role="option"], [role="listbox"] li')
        count = options.count()
        q_low = query.lower()
        best_idx = -1
        for i in range(count):
            txt = (options.nth(i).inner_text() or "").strip().lower()
            if txt == q_low or txt.startswith(q_low + " ") or txt.startswith(q_low + ":"):
                best_idx = i
                break
            if best_idx < 0 and q_low in txt:
                best_idx = i
        if best_idx >= 0:
            options.nth(best_idx).click(timeout=4000)
            page.wait_for_timeout(250)
            return True
    except Exception:
        return False
    return False


def pick_listbox_operation(page: Any, sys_name: str, op: str) -> bool:
    """Click the matching [role=option] inside the open operations listbox, honoring
    grouped subheaders (e.g. `universe` group vs `universe.following` / `universe.place`)."""
    singular = sys_name[:-2] if sys_name.endswith("es") and sys_name != "universe-places" else (
        sys_name[:-1] if sys_name.endswith("s") else sys_name
    )
    wanted_groups = {sys_name.lower(), singular.lower()}
    try:
        idx = page.evaluate(
            """([wantedGroups, op]) => {
                const lb = document.querySelector('[role="listbox"]');
                if (!lb) return -1;
                const opts = Array.from(lb.querySelectorAll('[role="option"]'));
                const matchesOp = (txt) => {
                    const t = (txt || '').trim().toLowerCase();
                    return t === op || t.endsWith(':' + op) || t.endsWith('-' + op);
                };
                // First pass: check grouped subheader text if present (e.g. MUI Autocomplete group)
                let fallbackIdx = -1;
                for (let i = 0; i < opts.length; i++) {
                    const el = opts[i];
                    if (!matchesOp(el.innerText)) continue;
                    if (fallbackIdx < 0) fallbackIdx = i;
                    let groupLabel = '';
                    const groupLi = el.closest('li')?.parentElement?.closest('li');
                    if (groupLi) {
                        const hdr = groupLi.querySelector('[class*="groupLabel"], [role="presentation"], div:first-child');
                        if (hdr && hdr !== el) groupLabel = (hdr.innerText || '').trim().toLowerCase().split('\\n')[0].trim();
                    }
                    if (!groupLabel) {
                        let prev = el.previousElementSibling;
                        while (prev) {
                            if (prev.getAttribute('role') !== 'option') {
                                groupLabel = (prev.innerText || '').trim().toLowerCase().split('\\n')[0].trim();
                                break;
                            }
                            prev = prev.previousElementSibling;
                        }
                    }
                    if (groupLabel && wantedGroups.includes(groupLabel)) {
                        return i;
                    }
                }
                return fallbackIdx;
            }""",
            [list(wanted_groups), op.lower()],
        )
        if isinstance(idx, int) and idx >= 0:
            page.locator('[role="listbox"] [role="option"]').nth(idx).click(timeout=4000)
            page.wait_for_timeout(250)
            return True
    except Exception:
        return False
    return False


def add_operations_for_input(page: Any, op_input_id: str, sys_name: str, ops: list[str]) -> None:
    """Select each required operation on the exact newly-added system's Operations input."""
    op_input = page.locator(f"#{op_input_id}") if op_input_id else page.locator(
        'input[placeholder="Select Operations to Add"]'
    ).first
    if op_input.count() == 0:
        return

    for op in ops:
        try:
            op_input.scroll_into_view_if_needed(timeout=4000)
            op_input.click(timeout=4000)
            page.wait_for_timeout(300)
            if not pick_listbox_operation(page, sys_name, op):
                select_autocomplete_option(page, op_input, op)
            page.keyboard.press("Escape")
            page.wait_for_timeout(150)
        except Exception:
            continue


def extract_generated_api_key(page: Any) -> str | None:
    """Read generated Open Cloud secret directly from Creator Hub (dialog or Copy Key section)."""
    try:
        candidates = page.evaluate(
            """() => {
                const vals = [];
                const scopes = Array.from(document.querySelectorAll('[role="dialog"], .MuiDialog-root, main, body'));
                for (const root of scopes) {
                    for (const el of root.querySelectorAll('input, textarea, code, pre')) {
                        const v = (el.value !== undefined ? el.value : el.innerText) || '';
                        if (v && v.trim().length >= 32) vals.push(v.trim());
                    }
                    // Creator Hub displays the generated key inside a leaf span/p right next to Copy Key To Clipboard
                    for (const el of root.querySelectorAll('span, p, div')) {
                        if (el.children.length === 0) {
                            const t = (el.innerText || '').trim();
                            if (/^[A-Za-z0-9+/_=-]{64,}$/.test(t) && !t.startsWith('http')) {
                                vals.push(t);
                            }
                        }
                    }
                }
                return vals;
            }"""
        )
        if isinstance(candidates, list):
            for c in candidates:
                tok = str(c).strip()
                # Roblox Open Cloud API keys are long opaque non-whitespace tokens (>= 32 chars)
                if len(tok) >= 32 and not any(ch.isspace() for ch in tok) and not tok.startswith("http"):
                    return tok
    except Exception:
        return None
    return None


def confirm_unrestricted_access_dialog_if_present(page: Any) -> bool:
    """Acknowledge Creator Hub's Unrestricted Experience Access warning dialog if open."""
    try:
        dlg = page.locator('[role="dialog"], .MuiDialog-root').filter(
            has_text=re.compile(r"Unrestricted Experience Access|I understand the security risks", re.I)
        ).first
        if dlg.count() == 0 or not dlg.is_visible():
            return False
        cb = dlg.locator('input[type="checkbox"]').first
        if cb.count() > 0:
            if not cb.is_checked():
                cb.check(force=True)
                page.wait_for_timeout(250)
        else:
            lbl = dlg.get_by_text(re.compile(r"I understand the security risks", re.I)).first
            if lbl.count() > 0:
                lbl.click(timeout=3000)
                page.wait_for_timeout(250)
        confirm_btn = dlg.get_by_role("button", name=re.compile(r"Save\s*&\s*Generate Key", re.I)).first
        if confirm_btn.count() > 0 and confirm_btn.is_visible() and confirm_btn.is_enabled():
            confirm_btn.click(timeout=5000)
            page.wait_for_timeout(600)
            return True
    except Exception:
        return False
    return False


def write_secret_file(path: str, secret: str) -> None:
    """Write secret into the pre-created 0600 file provided by Rust without printing it."""
    fd = os.open(path, os.O_WRONLY | os.O_TRUNC, 0o600)
    try:
        os.write(fd, secret.encode("utf-8"))
        os.fsync(fd)
    finally:
        os.close(fd)


def provision_api_key(
    page: Any,
    key_out: str,
    key_name: str,
    description: str,
    cidr: str,
    scopes: list[str],
    exp_name: str | None,
) -> dict[str, Any]:
    page.goto(CREDENTIALS_URL, wait_until="domcontentloaded", timeout=25000)
    page.wait_for_timeout(1500)
    blocker = detect_blocker(page)
    if blocker:
        return blocker

    create_btn = page.get_by_role("button", name=re.compile(r"Create API Key", re.I)).first
    create_btn.wait_for(state="visible", timeout=15000)
    create_btn.click(timeout=5000)

    name_input = page.locator("#api-key-name").first
    name_input.wait_for(state="visible", timeout=15000)
    blocker = detect_blocker(page)
    if blocker:
        return blocker

    name_input.fill(key_name)
    desc_input = page.locator("#api-key-description").first
    if desc_input.count() > 0 and desc_input.is_visible():
        desc_input.fill(description)

    sys_input = page.locator('input[placeholder="Select API System"]').first
    for sys_name, ops in group_scopes_by_system(scopes):
        if sys_input.count() > 0 and sys_input.is_visible():
            before_ids = set(
                page.evaluate(
                    """() => Array.from(document.querySelectorAll('input[placeholder="Select Operations to Add"]')).map(e => e.id).filter(Boolean)"""
                )
                or []
            )
            if select_autocomplete_option(page, sys_input, sys_name):
                add_sys_btn = page.get_by_role("button", name=re.compile(r"Add API System", re.I)).first
                if add_sys_btn.count() > 0 and add_sys_btn.is_visible() and add_sys_btn.is_enabled():
                    add_sys_btn.click(timeout=4000)
                page.wait_for_timeout(450)
                after_ids = (
                    page.evaluate(
                        """() => Array.from(document.querySelectorAll('input[placeholder="Select Operations to Add"]')).map(e => e.id).filter(Boolean)"""
                    )
                    or []
                )
                new_ids = [x for x in after_ids if x not in before_ids]
                add_operations_for_input(page, new_ids[0] if new_ids else "", sys_name, ops)

    # IP restriction CIDR (default 0.0.0.0/0): #api-key-restrict-ips -> #ip-address-input -> Add IP Address
    try:
        ip_toggle = page.locator("input#api-key-restrict-ips").first
        if ip_toggle.count() > 0 and ip_toggle.is_visible() and not ip_toggle.is_checked():
            ip_toggle.check(force=True)
            page.wait_for_timeout(300)
        cidr_input = page.locator(
            '#ip-address-input, input[placeholder*="CIDR" i], input[placeholder*="0.0.0.0" i], input[name*="cidr" i]'
        ).first
        if cidr_input.count() > 0 and cidr_input.is_visible():
            cidr_input.fill(cidr)
            page.wait_for_timeout(200)
            add_ip_btn = page.get_by_role("button", name=re.compile(r"Add IP", re.I)).first
            if add_ip_btn.count() > 0 and add_ip_btn.is_visible() and add_ip_btn.is_enabled():
                add_ip_btn.click(timeout=3000)
                page.wait_for_timeout(250)
    except Exception:
        pass

    save_btn = page.get_by_role("button", name=re.compile(r"Save\s*&\s*Generate Key", re.I)).first
    save_btn.scroll_into_view_if_needed(timeout=5000)
    save_btn.click(timeout=5000)

    # Poll for Unrestricted Access confirmation dialog, generated key, OR 2FA/captcha prompt
    deadline = time.monotonic() + 25.0
    confirmed_unrestricted = False
    while time.monotonic() < deadline:
        blocker = detect_blocker(page)
        if blocker:
            return blocker
        if not confirmed_unrestricted and confirm_unrestricted_access_dialog_if_present(page):
            confirmed_unrestricted = True
            deadline = max(deadline, time.monotonic() + 15.0)
            continue
        secret = extract_generated_api_key(page)
        if secret:
            write_secret_file(key_out, secret)
            return {"ok": True, "status": "ready", "key_written": True}
        page.wait_for_timeout(500)

    # Surface any visible validation message without hiding the tab
    err_msg = ""
    try:
        err_msg = page.evaluate(
            """() => {
                const errs = Array.from(document.querySelectorAll('[role="alert"], .Mui-error, .error-message'))
                    .map(e => (e.innerText || '').trim()).filter(Boolean);
                return errs.slice(0, 3).join('; ');
            }"""
        )
    except Exception:
        pass
    return {
        "ok": False,
        "status": "action_required",
        "blocked_reason": "key_generation_incomplete",
        "url": page.url,
        "message": (
            f"Creator Hub did not finish generating the API key ({err_msg}). Review the open tab and retry."
            if err_msg
            else "Creator Hub did not display a generated API key after Save & Generate Key. Check the open tab for a prompt or validation message, then retry."
        ),
    }


def click_save_changes(page: Any) -> bool:
    try:
        btn = page.get_by_role("button", name=re.compile(r"Save Changes|Save$", re.I)).first
        if btn.count() > 0 and btn.is_visible() and btn.is_enabled():
            btn.scroll_into_view_if_needed(timeout=3000)
            btn.click(timeout=5000)
            page.wait_for_timeout(1500)
            return True
    except Exception:
        return False
    return False


def run_status(cdp_url: str, check_login: bool) -> dict[str, Any]:
    from playwright.sync_api import sync_playwright

    with sync_playwright() as p:
        browser = p.chromium.connect_over_cdp(cdp_url, timeout=10000)
        version = getattr(browser, "version", "") or ""
        if not check_login:
            return {
                "ok": True,
                "cdp_url": cdp_url,
                "cdp_reachable": True,
                "browser_version": version,
            }
        context = browser.contexts[0] if browser.contexts else browser.new_context()
        page = context.new_page()
        keep_open = False
        try:
            page.goto(CREATIONS_URL, wait_until="domcontentloaded", timeout=25000)
            page.wait_for_timeout(1500)
            blocker = detect_blocker(page)
            if blocker:
                keep_open = True
                return {
                    "ok": True,
                    "cdp_url": cdp_url,
                    "cdp_reachable": True,
                    "browser_version": version,
                    "logged_in": False,
                    "blocked_reason": blocker["blocked_reason"],
                    "url": blocker["url"],
                    "message": blocker["message"],
                }
            user = fetch_authenticated_user(page)
            if not user:
                keep_open = True
                return {
                    "ok": True,
                    "cdp_url": cdp_url,
                    "cdp_reachable": True,
                    "browser_version": version,
                    "logged_in": False,
                    "blocked_reason": "login_required",
                    "url": page.url,
                    "message": "Not signed in to Roblox Creator Hub. Sign in via the open tab, then retry.",
                }
            experiences = fetch_experiences(page, user["id"])
            return {
                "ok": True,
                "cdp_url": cdp_url,
                "cdp_reachable": True,
                "browser_version": version,
                "logged_in": True,
                "user": user,
                "experiences": experiences,
            }
        finally:
            if not keep_open:
                try:
                    page.close()
                except Exception:
                    pass


def run_setup(cdp_url: str, key_out: str | None, spec: dict[str, Any]) -> dict[str, Any]:
    from playwright.sync_api import sync_playwright

    need_key = bool(spec.get("need_key", True))
    ensure_exp = bool(spec.get("ensure_experience", True))
    reuse_uid = spec.get("reuse_universe_id")
    existing_uid = spec.get("existing_universe_id")
    exp_name = spec.get("experience_name")
    key_name = str(spec.get("key_name") or "OpenRO Auto Key")
    description = str(spec.get("description") or "Provisioned by OpenRO for local project publishing")
    cidr = str(spec.get("cidr") or "0.0.0.0/0")
    scopes = spec.get("scopes") if isinstance(spec.get("scopes"), list) else DEFAULT_SCOPES

    with sync_playwright() as p:
        browser = p.chromium.connect_over_cdp(cdp_url, timeout=10000)
        context = browser.contexts[0] if browser.contexts else browser.new_context()
        page = context.new_page()
        keep_open = False
        try:
            page.goto(CREATIONS_URL, wait_until="domcontentloaded", timeout=25000)
            page.wait_for_timeout(1500)
            blocker = detect_blocker(page)
            if blocker:
                keep_open = True
                return blocker

            user = fetch_authenticated_user(page)
            if not user:
                keep_open = True
                return {
                    "ok": False,
                    "status": "action_required",
                    "blocked_reason": "login_required",
                    "url": page.url,
                    "message": "Sign in to Roblox Creator Hub in the open browser tab, then retry setup.",
                }

            experiences = fetch_experiences(page, user["id"])
            chosen = (
                pick_experience(
                    experiences,
                    str(reuse_uid) if reuse_uid else None,
                    str(existing_uid) if existing_uid else None,
                    str(exp_name) if exp_name else None,
                )
                if ensure_exp
                else None
            )
            universe_id = str(chosen["universeId"]) if chosen and chosen.get("universeId") else None
            place_id = str(chosen["rootPlaceId"]) if chosen and chosen.get("rootPlaceId") else None
            chosen_name = str(chosen.get("name") or "") if chosen else ""
            if universe_id and not place_id:
                place_id = resolve_root_place_id(page, universe_id)

            key_written = False
            if need_key:
                if not key_out:
                    return {
                        "ok": False,
                        "status": "error",
                        "error": "missing --key-out path for API key handoff",
                    }
                key_res = provision_api_key(
                    page,
                    key_out=key_out,
                    key_name=key_name,
                    description=description,
                    cidr=cidr,
                    scopes=scopes,
                    exp_name=chosen_name or (str(exp_name) if exp_name else None),
                )
                if not key_res.get("ok"):
                    keep_open = True
                    key_res.setdefault("user", user)
                    if universe_id:
                        key_res["universeId"] = universe_id
                    if place_id:
                        key_res["placeId"] = place_id
                    if chosen_name:
                        key_res["experience_name"] = chosen_name
                    return key_res
                key_written = bool(key_res.get("key_written"))

            return {
                "ok": True,
                "status": "ready",
                "user": user,
                "universeId": universe_id,
                "placeId": place_id,
                "experience_name": chosen_name or None,
                "experiences": experiences,
                "key_written": key_written,
            }
        finally:
            if not keep_open:
                try:
                    page.close()
                except Exception:
                    pass


def run_configure(cdp_url: str, spec: dict[str, Any]) -> dict[str, Any]:
    from playwright.sync_api import sync_playwright

    universe_id = str(spec.get("universeId") or "").strip()
    place_id = str(spec.get("placeId") or "").strip()
    if not universe_id.isdigit():
        return {"ok": False, "error": "configure requires a numeric universeId"}

    configured: list[str] = []
    manual_steps: list[dict[str, str]] = []

    with sync_playwright() as p:
        browser = p.chromium.connect_over_cdp(cdp_url, timeout=10000)
        context = browser.contexts[0] if browser.contexts else browser.new_context()
        page = context.new_page()
        keep_open = False
        try:
            # 1. Private servers on /advanced-settings
            ps = spec.get("private_servers")
            if isinstance(ps, dict):
                page.goto(
                    f"{EXP_BASE_URL}/{universe_id}/advanced-settings",
                    wait_until="domcontentloaded",
                    timeout=25000,
                )
                page.wait_for_timeout(1500)
                blocker = detect_blocker(page)
                if blocker:
                    keep_open = True
                    blocker["configured"] = configured
                    blocker["manual_steps_remaining"] = manual_steps
                    return blocker

                want_enabled = bool(ps.get("enabled", True))
                toggle = page.get_by_label(re.compile(r"Allow private servers", re.I)).first
                if toggle.count() == 0:
                    toggle = page.locator('input[type="checkbox"]').first
                if toggle.count() > 0:
                    if toggle.is_checked() != want_enabled:
                        toggle.click(force=True)
                        page.wait_for_timeout(300)
                if want_enabled and ps.get("price_robux") is not None:
                    price = int(ps["price_robux"])
                    if price <= 0:
                        free_opt = page.get_by_label(re.compile(r"^Free$", re.I)).first
                        if free_opt.count() > 0:
                            free_opt.click(force=True)
                    else:
                        paid_opt = page.get_by_label(re.compile(r"^Paid$", re.I)).first
                        if paid_opt.count() > 0:
                            paid_opt.click(force=True)
                        price_input = page.locator('input[type="number"], input[name*="price" i]').first
                        if price_input.count() > 0 and price_input.is_visible():
                            price_input.fill(str(price))
                click_save_changes(page)
                configured.append("private_servers")

            # 2. Game Icon on /places/<placeId>/icon
            icon_abs = spec.get("icon_abs_path")
            if isinstance(icon_abs, str) and icon_abs:
                if not place_id.isdigit():
                    return {"ok": False, "error": "icon upload requires placeId"}
                page.goto(
                    f"{EXP_BASE_URL}/{universe_id}/places/{place_id}/icon",
                    wait_until="domcontentloaded",
                    timeout=25000,
                )
                page.wait_for_timeout(1500)
                blocker = detect_blocker(page)
                if blocker:
                    keep_open = True
                    blocker["configured"] = configured
                    blocker["manual_steps_remaining"] = manual_steps
                    return blocker

                file_in = page.locator('input[type="file"]').first
                file_in.wait_for(state="attached", timeout=15000)
                file_in.set_input_files(icon_abs)
                page.wait_for_timeout(1500)
                click_save_changes(page)
                configured.append("icon")

            # 3. Store Thumbnails on /places/<placeId>/thumbnails
            thumb_paths = spec.get("thumbnail_abs_paths")
            if isinstance(thumb_paths, list) and thumb_paths:
                if not place_id.isdigit():
                    return {"ok": False, "error": "thumbnail upload requires placeId"}
                page.goto(
                    f"{EXP_BASE_URL}/{universe_id}/places/{place_id}/thumbnails",
                    wait_until="domcontentloaded",
                    timeout=25000,
                )
                page.wait_for_timeout(1500)
                blocker = detect_blocker(page)
                if blocker:
                    keep_open = True
                    blocker["configured"] = configured
                    blocker["manual_steps_remaining"] = manual_steps
                    return blocker

                for tp in thumb_paths:
                    file_in = page.locator('input[type="file"]').first
                    file_in.wait_for(state="attached", timeout=15000)
                    file_in.set_input_files(str(tp))
                    page.wait_for_timeout(1200)
                click_save_changes(page)
                configured.append("thumbnails")

            # 4. Optional basic fields (name / description / visibility)
            if any(spec.get(k) for k in ("name", "description", "visibility")):
                page.goto(
                    f"{EXP_BASE_URL}/{universe_id}/configure",
                    wait_until="domcontentloaded",
                    timeout=25000,
                )
                page.wait_for_timeout(1500)
                blocker = detect_blocker(page)
                if blocker:
                    keep_open = True
                    blocker["configured"] = configured
                    blocker["manual_steps_remaining"] = manual_steps
                    return blocker
                if spec.get("name"):
                    name_in = page.get_by_label(re.compile(r"^Name$", re.I)).first
                    if name_in.count() > 0 and name_in.is_visible():
                        name_in.fill(str(spec["name"]))
                        configured.append("name")
                if spec.get("description"):
                    desc_in = page.get_by_label(re.compile(r"^Description$", re.I)).first
                    if desc_in.count() > 0 and desc_in.is_visible():
                        desc_in.fill(str(spec["description"]))
                        configured.append("description")
                if spec.get("visibility"):
                    vis = str(spec["visibility"]).strip().lower()
                    vis_opt = page.get_by_label(re.compile(rf"^{re.escape(vis)}$", re.I)).first
                    if vis_opt.count() > 0:
                        vis_opt.click(force=True)
                        configured.append("visibility")
                click_save_changes(page)

            # 5. Content Rating Questionnaire status check (never auto-answers legal questionnaires)
            if spec.get("check_questionnaire", True):
                q_url = f"{EXP_BASE_URL}/{universe_id}/content-rating"
                page.goto(q_url, wait_until="domcontentloaded", timeout=25000)
                page.wait_for_timeout(1500)
                blocker = detect_blocker(page)
                if blocker:
                    keep_open = True
                    blocker["configured"] = configured
                    blocker["manual_steps_remaining"] = manual_steps
                    return blocker
                body_text = ""
                try:
                    body_text = (page.evaluate("() => document.body ? document.body.innerText : ''") or "").lower()
                except Exception:
                    pass
                needs_q = any(
                    marker in body_text
                    for marker in ("start questionnaire", "take questionnaire", "unrated", "not yet rated", "complete questionnaire")
                ) or not any(
                    done in body_text
                    for done in ("retake questionnaire", "edit questionnaire", "minimal", "mild", "moderate", "restricted")
                )
                if needs_q:
                    manual_steps.append(
                        {
                            "step": "Complete Age & Experience Guidelines questionnaire",
                            "url": q_url,
                        }
                    )

            return {
                "ok": True,
                "status": "ready",
                "universeId": universe_id,
                "placeId": place_id or None,
                "configured": configured,
                "manual_steps_remaining": manual_steps,
            }
        finally:
            if not keep_open:
                try:
                    page.close()
                except Exception:
                    pass


def main() -> int:
    parser = argparse.ArgumentParser(description="OpenRO Creator Hub CDP helper")
    parser.add_argument("--action", required=True, choices=["status", "setup", "configure"])
    parser.add_argument("--cdp-url", required=True)
    parser.add_argument("--key-out", default=None)
    parser.add_argument("--check-login", action="store_true")
    args = parser.parse_args()

    try:
        cdp_url = validate_loopback_cdp(args.cdp_url)
    except ValueError as e:
        emit({"ok": False, "error": str(e)})
        return 2

    try:
        spec = read_stdin_spec()
        if args.action == "status":
            res = run_status(cdp_url, check_login=args.check_login)
        elif args.action == "setup":
            res = run_setup(cdp_url, key_out=args.key_out, spec=spec)
        else:
            res = run_configure(cdp_url, spec=spec)
        emit(res)
        return 0
    except ModuleNotFoundError as e:
        emit(
            {
                "ok": False,
                "error": f"Python Playwright is not installed ({e}). Install with: pip install playwright",
            }
        )
        return 1
    except Exception as e:
        emit({"ok": False, "error": f"Creator Hub browser automation failed: {e}"})
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
