#!/usr/bin/env python3
"""OMNI-LOOP v4.0 APEX deterministic controller.

Stdlib-only command line tool that drives the deterministic parts of the
omni-loop operating procedure: state initialization, contract validation,
cryptographic hashing, UEVAM scoring, the anti-tamper artifact audit,
memory compaction, and project smoke checks.

Usage:
  python omni_loop_controller.py <command> [args]

Run from the project root (default) or pass --root <path>.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

SCHEMA_VERSION = "4.0.0"

RECORD_FILES = [
    ".omni-loop/state.json",
    ".omni-loop/genesis-dag.json",
    ".omni-loop/current-contract.json",
    ".omni-loop/last-verdict.json",
    ".omni-loop/baseline.md",
    ".omni-loop/product-map.md",
    ".omni-loop/feature-inventory.md",
    ".omni-loop/opportunity-backlog.md",
    ".omni-loop/quality-scorecard.md",
    ".omni-loop/iteration-log.md",
    ".omni-loop/decisions.md",
    ".omni-loop/rejected-candidates.md",
    ".omni-loop/process-benchmarks.md",
]

# Skill internals and vendored reference material are not project deliverables.
AUDIT_EXEMPT_PREFIXES = (".opencode/",)

DEFAULT_SMOKE = ["npm run typecheck", "npm run lint", "npm test"]

DIMENSIONS = [
    "U01", "U02", "U03", "U04", "U05", "U06", "U07", "U08",
    "U09", "U10", "U11", "U12", "U13", "U14", "U15", "U16",
]


def utc_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def resolve_git() -> str:
    """Return a git executable name/path that works on Windows and POSIX."""
    candidate = "git"
    try:
        subprocess.run([candidate, "--version"], capture_output=True, check=True)
        return candidate
    except (OSError, subprocess.CalledProcessError):
        if os.name == "nt":
            for fallback in (
                r"C:\Program Files\Git\cmd\git.exe",
                r"C:\Program Files\Git\bin\git.exe",
            ):
                if Path(fallback).is_file():
                    return fallback
    return candidate


def git_lines(root: Path, *args: str) -> list[str]:
    out = subprocess.run(
        [resolve_git(), *args], cwd=str(root), capture_output=True
    )
    if out.returncode != 0:
        return []
    return out.stdout.decode("utf-8", "replace").splitlines()


# --------------------------------------------------------------------------
# init / status
# --------------------------------------------------------------------------

def default_state() -> dict:
    return {
        "schema_version": SCHEMA_VERSION,
        "updated_at": utc_now(),
        "project_root": None,
        "entry_mode": None,
        "active_harnesses": [],
        "incumbent_commit": None,
        "current_iteration": 0,
        "current_wave": 0,
        "universal_dimensions": {d: None for d in DIMENSIONS},
        "dimension_stagnation_streaks": {d: 0 for d in DIMENSIONS},
        "map_elites_archive": [],
        "next_recommended_action": None,
        "verification": {"smoke": list(DEFAULT_SMOKE)},
        "converged": False,
    }


HEADER_FOR = {
    ".omni-loop/baseline.md": "# Baseline intake (mode 0B)\n\nEnvironment, pre-existing failures, and baseline evidence.\n",
    ".omni-loop/product-map.md": "# Product map\n\nEntry points, routes, data stores, shared UI, pipeline, trust boundaries.\n",
    ".omni-loop/feature-inventory.md": "# Feature inventory\n\n| ID | Feature | Verified status | Evidence | Files | Protecting tests |\n|---|---|---|---|---|---|\n",
    ".omni-loop/opportunity-backlog.md": "# Opportunity backlog (UEVAM ranked)\n",
    ".omni-loop/quality-scorecard.md": "# Quality scorecard (16D universal, 0.0-5.0)\n",
    ".omni-loop/iteration-log.md": "# Iteration log (append-only, L1 active context)\n",
    ".omni-loop/decisions.md": "# Decisions (L2 episodic memory)\n\nArchitectural choices with competing-approach rationale, plus compacted epochs.\n",
    ".omni-loop/rejected-candidates.md": "# Rejected candidates (negative-result archive)\n",
    ".omni-loop/process-benchmarks.md": "# Process benchmarks (70% tunable dev set / 30% sealed holdout)\n",
    ".omni-loop/genesis-dag.json": "",
    ".omni-loop/current-contract.json": "",
    ".omni-loop/last-verdict.json": "",
}


def cmd_init(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    created, present = [], []
    for rel in RECORD_FILES:
        if rel.endswith("state.json"):
            continue  # handled at the end with full defaults
        path = root / rel
        if path.exists():
            present.append(rel)
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        if rel.endswith(".json"):
            skeleton: dict = {"schema_version": SCHEMA_VERSION}
            if rel.endswith("genesis-dag.json"):
                skeleton = {"schema_version": SCHEMA_VERSION, "status": "not_compiled"}
            if rel.endswith("current-contract.json"):
                skeleton = {"schema_version": SCHEMA_VERSION, "status": "not_locked"}
            if rel.endswith("last-verdict.json"):
                skeleton = {"schema_version": SCHEMA_VERSION, "status": "not_written"}
            path.write_text(
                json.dumps(skeleton, indent=2) + "\n", encoding="utf-8"
            )
        else:
            path.write_text(HEADER_FOR.get(rel, ""), encoding="utf-8")
        created.append(rel)
    for rel in (".omni-loop/locks", ".omni-loop/worktrees", ".omni-loop/screenshots"):
        target = root / rel
        if not target.exists():
            target.mkdir(parents=True, exist_ok=True)
            created.append(rel)
    state_path = root / ".omni-loop" / "state.json"
    data = default_state()
    if state_path.exists():
        try:
            loaded_raw = json.loads(state_path.read_text(encoding="utf-8"))
            loaded = {k: v for k, v in loaded_raw.items() if v is not None} if isinstance(loaded_raw, dict) else {}
            data = {**default_state(), **loaded}
        except (OSError, json.JSONDecodeError):
            data = default_state()
        data.setdefault("verification", {"smoke": list(DEFAULT_SMOKE)})
        data["verification"].setdefault("smoke", list(DEFAULT_SMOKE))
    if not data.get("project_root"):
        branch_first = (git_lines(root, "rev-parse", "--abbrev-ref", "HEAD") + [None])[0]
        data["project_root"] = str(root)
        data["branch"] = branch_first
        data["incumbent_commit"] = (
            git_lines(root, "rev-parse", "HEAD") + [data.get("incumbent_commit")]
        )[0]
    dirty = git_lines(root, "status", "--porcelain")
    if dirty:
        data["uncommitted_changes"] = len(dirty)
    data["updated_at"] = utc_now()
    state_path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")

    print(f"root: {root}")
    if created:
        print("created: " + ", ".join(created))
    print(f"already present: {len(present)} record file(s)")
    print("entry_mode: " + str(data.get("entry_mode") or "(unassigned)"))
    if dirty:
        print(f"note: {len(dirty)} uncommitted change line(s) in the working tree")
    return 0


def cmd_status(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    state_path = root / ".omni-loop" / "state.json"
    if not state_path.exists():
        print("no state: run init first")
        return 1
    try:
        data = json.loads(state_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        print(f"state unreadable: {exc}")
        return 1
    scored = {k: v for k, v in (data.get("universal_dimensions") or {}).items() if v is not None}
    entries = git_header_active(root)
    print(f"entry_mode: {data.get('entry_mode') or '(unassigned)'}")
    print(f"incumbent_commit: {data.get('incumbent_commit') or '(unset)'}")
    print(f"iteration: {data.get('current_iteration')} (wave {data.get('current_wave')})")
    print(f"active harnesses: {', '.join(data.get('active_harnesses') or []) or '(none)'}")
    if scored:
        values = list(scored.values())
        composite = round(0.75 * (sum(values) / len(values)) + 0.25 * min(values), 3)
        print("scored dimensions: " + ", ".join(f"{k}={v}" for k, v in sorted(scored.items())))
        print(f"composite: {composite}")
    else:
        print("scored dimensions: none yet")
    print(f"next_recommended_action: {data.get('next_recommended_action') or '(none)'}")
    print(f"stagnation streaks >= 3: " + ", ".join(
        k for k, v in (data.get("dimension_stagnation_streaks") or {}).items() if v >= 3
    ) or "none")
    print(f"converged: {bool(data.get('converged'))}")
    print("iteration log entries (L1): " + str(entries))
    return 0


def git_header_active(root: Path) -> int:
    path = root / ".omni-loop" / "iteration-log.md"
    if not path.exists():
        return 0
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return 0
    return len(re.findall(r"^##\s+Iteration\b", text, flags=re.MULTILINE))


# --------------------------------------------------------------------------
# hash
# --------------------------------------------------------------------------

def cmd_hash(args: argparse.Namespace) -> int:
    exit_code = 0
    for raw in args.files:
        path = Path(args.root).resolve() / raw
        if not path.is_file():
            print(f"{raw}: MISSING")
            exit_code = 1
            continue
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        print(f"{digest}  {raw}")
    return exit_code


# --------------------------------------------------------------------------
# verify-contract
# --------------------------------------------------------------------------

STRICT_FAILS: list[str] = []


def _fail(msg: str) -> None:
    STRICT_FAILS.append(msg)


def _require(cond: bool, msg: str) -> None:
    if not cond:
        _fail(msg)


def cmd_verify_contract(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    path = Path(args.file) if args.file else root / ".omni-loop" / "current-contract.json"
    if not path.is_file():
        print(f"contract missing: {path}")
        return 1
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        print(f"contract unreadable JSON: {exc}")
        return 1
    _require(data.get("schema_version") == SCHEMA_VERSION, "schema_version must be 4.0.0")
    for key in (
        "iteration_id", "wave_id", "active_harness", "target_dimension",
        "ponytail_rung_selected", "problem_or_genesis_goal", "reproduction_evidence",
        "user_impact", "atomic_scope_files", "non_goals", "acceptance_oracles",
        "metamorphic_invariants", "regression_commands", "rollback_tag",
        "pre_edit_hashes", "uevam_inputs",
    ):
        _require(key in data, f"missing required field: {key}")
    if "iteration_id" in data:
        _require(isinstance(data["iteration_id"], int) and data["iteration_id"] >= 1,
                 "iteration_id must be an integer >= 1")
        _require(isinstance(data.get("wave_id"), int) and data["wave_id"] >= 0,
                 "wave_id must be an integer >= 0")
    scope = data.get("atomic_scope_files")
    if isinstance(scope, list):
        _require(1 <= len(scope) <= 5, f"atomic_scope_files must contain 1..5 files (got {len(scope)})")
        _require(all(isinstance(s, str) and s for s in scope), "atomic_scope_files entries must be non-empty strings")
        if args.enforce_disjoint:
            base = Path(root)
            for other in (root / ".omni-loop" / "locks").glob("*.lock"):
                try:
                    locked = set(json.loads(other.read_text(encoding="utf-8")))
                except (OSError, json.JSONDecodeError):
                    continue
                overlap = sorted(set(scope) & locked)
                _require(not overlap, f"lease {other.name} overlaps contracted files: {overlap}")
    oracles = data.get("acceptance_oracles")
    if isinstance(oracles, list):
        _require(len(oracles) >= 1, "acceptance_oracles needs at least one oracle")
        for i, o in enumerate(oracles):
            if isinstance(o, dict):
                _require(bool(o.get("id")), f"oracle[{i}] missing id")
                _require(bool(o.get("description")), f"oracle[{i}] missing description")
                _require(bool(o.get("command")), f"oracle[{i}] missing command")
    _require(isinstance(data.get("non_goals"), list) and data.get("non_goals"),
             "non_goals must be a non-empty list")
    _require(bool(data.get("problem_or_genesis_goal")), "problem_or_genesis_goal required")
    _require(bool(data.get("reproduction_evidence")), "reproduction_evidence required")
    tag = data.get("rollback_tag") or ""
    if tag:
        _require(bool(re.fullmatch(r"omni-loop/iter-\d+-pre", tag)),
                 f"rollback_tag must match omni-loop/iter-<n>-pre (got {tag!r})")
    hashes = data.get("pre_edit_hashes")
    if isinstance(hashes, dict) and hashes:
        for file_rel, digest in hashes.items():
            _require(isinstance(digest, str) and re.fullmatch(r"[0-9a-f]{64}", digest),
                     f"pre_edit_hashes[{file_rel}] must be a sha256 hex digest")
    u = data.get("uevam_inputs")
    if isinstance(u, dict):
        _require(isinstance(u.get("impact"), (int, float)) and 1.0 <= u["impact"] <= 5.0, "uevam impact in [1,5]")
        _require(isinstance(u.get("severity"), (int, float)) and 1.0 <= u["severity"] <= 5.0, "uevam severity in [1,5]")
        _require(isinstance(u.get("confidence"), (int, float)) and 0.1 <= u["confidence"] <= 1.0, "uevam confidence in [0.1,1]")
        _require(isinstance(u.get("unlocks"), int) and u["unlocks"] >= 0, "uevam unlocks >= 0")
        _require(isinstance(u.get("effort"), (int, float)) and 1.0 <= u["effort"] <= 5.0, "uevam effort in [1,5]")
        _require(isinstance(u.get("risk"), (int, float)) and 0.0 <= u["risk"] <= 5.0, "uevam risk in [0,5]")
        _require(isinstance(u.get("substructure_reward"), (int, float)) and 0.5 <= u.get("substructure_reward", 1.0) <= 2.0,
                 "uevam substructure_reward in [0.5,2]")
        _require(isinstance(u.get("info_gain"), (int, float)) and 0.0 <= u.get("info_gain", 0.0) <= 10.0,
                 "uevam info_gain in [0,10]")
    if STRICT_FAILS:
        print("contract INVALID:")
        for m in STRICT_FAILS:
            print(f"  - {m}")
        return 1
    print("contract VALID")
    return 0


# --------------------------------------------------------------------------
# uevam
# --------------------------------------------------------------------------

def cmd_uevam(args: argparse.Namespace) -> int:
    impact, severity, confidence = args.impact, args.severity, args.confidence
    unlocks, effort, risk = args.unlocks, args.effort, args.risk
    info_gain = args.info_gain
    rsub = args.substructure_reward
    numerator = (impact * severity * confidence * (1 + 0.25 * unlocks) + 0.35 * info_gain) * rsub
    denominator = (effort ** 0.8) * (1 + 0.5 * risk)
    score = numerator / denominator if denominator else 0.0
    if args.is_p0_blocker:
        score += 1000.0
    score = round(score, 3)
    print(json.dumps({
        "uevam": score,
        "is_p0_blocker": args.is_p0_blocker,
        "inputs": {
            "impact": impact, "severity": severity, "confidence": confidence,
            "unlocks": unlocks, "info_gain": info_gain,
            "substructure_reward": rsub, "effort": effort, "risk": risk,
        },
    }))
    return 0


# --------------------------------------------------------------------------
# audit-artifact
# --------------------------------------------------------------------------

class Rule:
    def __init__(self, name: str, pattern: str, severity: str):
        self.name = name
        self.re = re.compile(pattern, re.IGNORECASE if severity != "literal-intent" else 0)
        self.severity = severity


RULES = [
    Rule("type-suppression", r"@ts-ignore\b|@ts-expect-error\b|@ts-nocheck\b", "fail"),
    Rule("lint-suppression", r"eslint-disable|#?\s?(?:pylint|flake8|ruff):\s*noqa|# noqa\b|(?<![\w-])# type:\s*ignore", "fail"),
    Rule("skipped-test", r"\b(?:it|test|describe)\.(?:only|skip)\b|\bxit\b|\bxdescribe\b|@pytest\.mark\.skip|@unittest\.skip", "fail"),
    Rule("placeholder-text", r"\b(?:TODO|TBD|FIXME)\b|Lorem ipsum", "warn-in-docs"),
    Rule("insert-placeholder", r"\[insert [^\]]{0,80}\]", "warn-in-docs"),
    Rule("ellipsis-dump", r"\.\.\.(?:\s*existing code|\s*rest of (?:file|the))", "warn-in-docs"),
    Rule("bare-github-url", r"https://github\.com/(?:[\"'\s\\)\]>]|$)", "warn-in-docs"),
    Rule("bare-arxiv-url", r"https://arxiv\.org/(?:[\"'\s\\)\]>]|$)", "warn-in-docs"),
    Rule("aws-key", r"\bAKIA[0-9A-Z]{16}\b", "fail"),
    Rule("github-token", r"\bgh[pousrcr]_[A-Za-z0-9]{30,80}\b|\bgithub_pat_[A-Za-z0-9_]{20,}\b", "fail"),
    Rule("anthropic-key", r"\bsk-ant-[A-Za-z0-9_\-]{20,}\b", "fail"),
    Rule("openai-key", r"\bsk-proj-[A-Za-z0-9_\-]{20,}\b|\bsk-[A-Za-z0-9]{40,}\b", "fail"),
    Rule("slack-token", r"\bxox[baprs]-[A-Za-z0-9\-]{10,}\b", "fail"),
    Rule("google-key", r"\bAIza[0-9A-Za-z_\-]{35}\b", "fail"),
    Rule("private-key-block", r"-----BEGIN [A-Z ]*PRIVATE KEY-----", "fail"),
    Rule("gitlab-token", r"\bglpat-[A-Za-z0-9_\-]{20,}\b", "fail"),
]

DOC_SUFFIXES = {".md", ".mdx", ".txt", ".rst", ".adoc", ".json", ".yml", ".yaml"}


def is_doc(rel: str) -> bool:
    suffix = Path(rel).suffix.lower()
    return rel.startswith(".omni-loop") or suffix in DOC_SUFFIXES


def cmd_audit_artifact(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    if args.changed_only:
        paths = set(git_lines(root, "diff", "--name-only", args.changed_only + "..HEAD"))
        paths |= set(git_lines(root, "diff", "--name-only", args.changed_only))  # uncommitted
        paths |= set(git_lines(root, "ls-files", "--others", "--exclude-standard"))
    else:
        paths = set(git_lines(root, "ls-files"))
        paths |= set(git_lines(root, "ls-files", "--others", "--exclude-standard"))
    auditable = [
        p for p in paths
        if p and not p.startswith(AUDIT_EXEMPT_PREFIXES)
        and not any(p.startswith(w + "/") or "/.omni-loop/worktrees/" in "/" + p for w in ("node_modules",))
        and ".omni-loop/worktrees/" not in ("/" + p)
    ]
    findings: list[str] = []
    counts = {"fail": 0, "warn": 0, "info": 0}
    for rel in sorted(auditable):
        path = root / rel
        try:
            blob = path.read_bytes()
        except OSError:
            findings.append(f"INFO  {rel}: unreadable (skipped)")
            counts["info"] += 1
            continue
        if blob[:8192].find(b"\x00") != -1:
            continue
        text = blob.decode("utf-8", "replace")
        doc = is_doc(rel)
        for lineno, line in enumerate(text.splitlines(), start=1):
            for rule in RULES:
                if not rule.re.search(line):
                    continue
                sev = rule.severity
                if sev == "warn-in-docs":
                    if rel.startswith(".omni-loop"):
                        sev_key, label = "info", "INFO"
                    elif doc:
                        sev_key, label = "warn", "WARN"
                    else:
                        sev_key, label = "fail", "FAIL"
                else:
                    sev_key, label = "fail", "FAIL"
                counts[sev_key] += 1
                findings.append(f"{label}  {rel}:{lineno} [{rule.name}] {line.strip()[:160]}")
    # contract hash integrity
    contract_path = root / ".omni-loop" / "current-contract.json"
    if contract_path.is_file():
        try:
            contract = json.loads(contract_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            contract = None
        for file_rel, digest in ((contract or {}).get("pre_edit_hashes") or {}).items():
            target = root / file_rel
            if not target.is_file():
                findings.append(f"FAIL  {file_rel}: hashed at contract time, file now MISSING")
                counts["fail"] += 1
                continue
            current = hashlib.sha256(target.read_bytes()).hexdigest()
            if current != digest:
                findings.append(
                    f"WARN  {file_rel}: changed since contract lock (recheck diff: only test strengthening allowed)"
                )
                counts["warn"] += 1
    for line in findings:
        print(line)
    print(
        f"audit: {len(auditable)} file(s) scanned, "
        f"{counts['fail']} fail, {counts['warn']} warn, {counts['info']} info"
    )
    return 1 if counts["fail"] else 0


# --------------------------------------------------------------------------
# compact-memory
# --------------------------------------------------------------------------

HEADING_RE = re.compile(r"^##\s+", re.MULTILINE)


def split_sections(text: str) -> list[str]:
    matches = list(HEADING_RE.finditer(text))
    if not matches:
        return []
    sections = []
    for i, m in enumerate(matches):
        end = matches[i + 1].start() if i + 1 < len(matches) else len(text)
        sections.append(text[m.start():end])
    return sections


def cmd_compact_memory(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    log_path = root / ".omni-loop" / "iteration-log.md"
    decisions_path = root / ".omni-loop" / "decisions.md"
    if not log_path.is_file():
        print("iteration-log.md missing")
        return 1
    text = log_path.read_text(encoding="utf-8", errors="replace")
    header, _, _ = text.partition("\n## ")
    body = text[len(header):] if header else text
    sections = split_sections(body)
    keep_n = max(1, args.window_size)
    keep, move = sections[-keep_n:], sections[:-keep_n]
    if not move:
        print(f"nothing to compact ({len(sections)} record(s), window {keep_n})")
        return 0
    stamp = utc_now()
    moved = len(move)
    epoch = {
        "schema_version": SCHEMA_VERSION,
        "type": "epoch_compaction",
        "moved_records": moved,
        "compacted_at": stamp,
        "source_window_size": keep_n,
    }
    log_header = header if header else "# Iteration log (append-only, L1 active context)\n"
    log_path.write_text(log_header + "".join(keep), encoding="utf-8")
    with decisions_path.open("a", encoding="utf-8") as handle:
        if decisions_path.exists() and decisions_path.stat().st_size > 0:
            handle.write("\n\n---\n\n")
        handle.write(f"## Epoch compaction {stamp}\n\n")
        handle.write("```json\n" + json.dumps(epoch, indent=2) + "\n```\n\n")
        for section in move:
            handle.write(section.rstrip() + "\n")
    remaining = git_header_active(root)
    print(f"compacted {moved} iteration record(s) into decisions.md; {remaining} remain in L1")
    return 0


# --------------------------------------------------------------------------
# smoke
# --------------------------------------------------------------------------

def cmd_smoke(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    state_path = root / ".omni-loop" / "state.json"
    commands = list(DEFAULT_SMOKE)
    if state_path.is_file():
        try:
            data = json.loads(state_path.read_text(encoding="utf-8"))
            commands = (data.get("verification") or {}).get("smoke") or commands
        except (OSError, json.JSONDecodeError):
            pass
    overall = 0
    for command in commands:
        runner = (
            ["cmd.exe", "/d", "/c", command]
            if os.name == "nt"
            else ["/bin/sh", "-c", command]
        )
        proc = subprocess.run(runner, cwd=str(root), capture_output=True, text=True)
        code = proc.returncode
        overall = code if code != 0 else overall
        tail = (proc.stdout or "").strip().splitlines()[-6:]
        errtail = (proc.stderr or "").strip().splitlines()[-6:]
        print(f"[exit {code}] {command}")
        for line in tail:
            print(f"  out| {line[:200]}")
        for line in errtail:
            print(f"  err| {line[:200]}")
    print("smoke: " + ("FAILED" if overall else "PASS"))
    return 1 if overall else 0


# --------------------------------------------------------------------------
# argument parsing
# --------------------------------------------------------------------------

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="OMNI-LOOP v4.0 APEX controller")
    parser.add_argument("--root", default=os.getcwd(), help="project root (default: cwd)")
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("init", help="create or repair .omni-loop state records")
    sub.add_parser("status", help="brief resume summary")

    p_hash = sub.add_parser("hash", help="SHA-256 of files for contract locking")
    p_hash.add_argument("files", nargs="+", metavar="FILE")

    p_vc = sub.add_parser("verify-contract", help="validate the current improvement contract")
    p_vc.add_argument("--file", default=None, help="contract path (default .omni-loop/current-contract.json)")
    p_vc.add_argument("--enforce-disjoint", action="store_true", help="reject overlap with active lease locks")

    p_ue = sub.add_parser("uevam", help="compute the UEVAM priority score")
    p_ue.add_argument("--impact", type=float, required=True)
    p_ue.add_argument("--severity", type=float, required=True)
    p_ue.add_argument("--confidence", type=float, required=True)
    p_ue.add_argument("--unlocks", type=int, required=True)
    p_ue.add_argument("--info-gain", dest="info_gain", type=float, default=0.0)
    p_ue.add_argument("--substructure-reward", dest="substructure_reward", type=float, default=1.0)
    p_ue.add_argument("--effort", type=float, required=True)
    p_ue.add_argument("--risk", type=float, required=True)
    p_ue.add_argument("--is-p0-blocker", dest="is_p0_blocker", action="store_true")

    p_au = sub.add_parser("audit-artifact", help="anti-tamper / placeholder / secret scan")
    p_au.add_argument("--changed-only", dest="changed_only", default=None, metavar="REV",
                      help="scan only changes vs REV plus untracked files")

    p_cm = sub.add_parser("compact-memory", help="archive older iteration records to decisions.md")
    p_cm.add_argument("--window-size", dest="window_size", type=int, default=5)

    sub.add_parser("smoke", help="run state.json verification.smoke commands")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    handler = {
        "init": cmd_init,
        "status": cmd_status,
        "hash": cmd_hash,
        "verify-contract": cmd_verify_contract,
        "uevam": cmd_uevam,
        "audit-artifact": cmd_audit_artifact,
        "compact-memory": cmd_compact_memory,
        "smoke": cmd_smoke,
    }[args.command]
    try:
        return handler(args)
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())
