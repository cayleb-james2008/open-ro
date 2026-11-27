// Static Luau Analyzer, Syntax Highlighter Tokenizer, and Place Build Diff engine.
// Mirrors src/luau_check.rs checks for instant client-side diagnostics + standalone web-engine parity.
import type { Place, PlaceDiff, PlaceDiffItem, PlaceInstance } from "./data";
import type {
  LuauContractMismatch,
  LuauSymbol,
  WorkspaceSymbolIndex,
  LuauDefinitionResult,
} from "./ship/api";

export type LuauSeverity = "error" | "warning";

export type LuauDiagnostic = {
  path: string;
  line: number;
  severity: LuauSeverity;
  code: string;
  message: string;
};

const VALID_SERVICES = new Set([
  "Workspace",
  "ServerScriptService",
  "ServerStorage",
  "StarterGui",
  "StarterPlayer",
  "ReplicatedStorage",
  "Lighting",
  "SoundService",
]);

const WORKSPACE_BUILTINS = new Set([
  "CurrentCamera",
  "Gravity",
  "Terrain",
  "FindFirstChild",
  "WaitForChild",
  "GetChildren",
  "GetDescendants",
  "ChildAdded",
  "DescendantAdded",
  "Raycast",
  "Name",
  "Parent",
]);

const IDENT_START_RE = /[\p{L}_]/u;
const IDENT_CHAR_RE = /[\p{L}\p{N}_]/u;
const IDENT_PAT = "[\\p{L}_][\\p{L}\\p{N}_]*";

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function matchLongBracketOpen(src: string, idx: number): number | null {
  if (src[idx] !== "[") return null;
  let j = idx + 1;
  while (j < src.length && src[j] === "=") j++;
  if (src[j] === "[") return j - idx - 1;
  return null;
}

function findLongBracketClose(src: string, startAfterOpen: number, eqCount: number): { end: number; newlines: number } | null {
  let i = startAfterOpen;
  let newlines = 0;
  while (i < src.length) {
    if (src[i] === "\n") newlines++;
    if (src[i] === "]") {
      let ok = true;
      for (let k = 1; k <= eqCount; k++) {
        if (src[i + k] !== "=") {
          ok = false;
          break;
        }
      }
      if (ok && src[i + eqCount + 1] === "]") {
        return { end: i + eqCount + 2, newlines };
      }
    }
    i++;
  }
  return null;
}

type ScannedLuau = {
  codeOnlySrc: string;
  codeNoCommentsSrc: string;
  codeOnlyLines: string[];
  codeNoCommentsLines: string[];
  isCommentOffset: Uint8Array;
  isStringOffset: Uint8Array;
};

/** Scan Luau source into 1:1 line-aligned views masking comments and string interiors. */
function scanLuauCode(src: string): ScannedLuau {
  const n = src.length;
  const codeOnlyArr = new Array<string>(n);
  const noCommentArr = new Array<string>(n);
  const isCommentOffset = new Uint8Array(n);
  const isStringOffset = new Uint8Array(n);

  const maskRange = (from: number, to: number, kind: "comment" | "string-inner" | "quote") => {
    for (let k = from; k < to; k++) {
      const ch = src[k];
      if (kind === "comment") {
        isCommentOffset[k] = 1;
        codeOnlyArr[k] = ch === "\n" ? "\n" : " ";
        noCommentArr[k] = ch === "\n" ? "\n" : " ";
      } else if (kind === "string-inner") {
        isStringOffset[k] = 1;
        codeOnlyArr[k] = ch === "\n" ? "\n" : " ";
        noCommentArr[k] = ch;
      } else {
        isStringOffset[k] = 1;
        codeOnlyArr[k] = ch;
        noCommentArr[k] = ch;
      }
    }
  };

  let i = 0;
  while (i < n) {
    const c = src[i];
    if (c === "-" && src[i + 1] === "-") {
      const eq = matchLongBracketOpen(src, i + 2);
      if (eq !== null) {
        const close = findLongBracketClose(src, i + 4 + eq, eq);
        const end = close ? close.end : n;
        maskRange(i, end, "comment");
        i = end;
        continue;
      }
      let end = i + 2;
      while (end < n && src[end] !== "\n") end++;
      maskRange(i, end, "comment");
      i = end;
      continue;
    }

    if (c === '"' || c === "'" || c === "`") {
      const q = c;
      maskRange(i, i + 1, "quote");
      let j = i + 1;
      let closed = false;
      while (j < n) {
        if (src[j] === "\\" && j + 1 < n) {
          j += 2;
          continue;
        }
        if (src[j] === "\n" && q !== "`") {
          break;
        }
        if (src[j] === q) {
          maskRange(i + 1, j, "string-inner");
          maskRange(j, j + 1, "quote");
          j++;
          closed = true;
          break;
        }
        j++;
      }
      if (!closed) {
        maskRange(i + 1, j, "string-inner");
      }
      i = j;
      continue;
    }

    const eq = matchLongBracketOpen(src, i);
    if (eq !== null) {
      const openLen = 2 + eq;
      const close = findLongBracketClose(src, i + openLen, eq);
      const closeStart = close ? close.end - openLen : n;
      const end = close ? close.end : n;
      maskRange(i, Math.min(i + openLen, n), "quote");
      if (closeStart > i + openLen) {
        maskRange(i + openLen, closeStart, "string-inner");
      }
      if (close) {
        maskRange(closeStart, end, "quote");
      }
      i = end;
      continue;
    }

    codeOnlyArr[i] = c;
    noCommentArr[i] = c;
    i++;
  }

  const codeOnlySrc = codeOnlyArr.join("");
  const codeNoCommentsSrc = noCommentArr.join("");
  return {
    codeOnlySrc,
    codeNoCommentsSrc,
    codeOnlyLines: codeOnlySrc.split("\n"),
    codeNoCommentsLines: codeNoCommentsSrc.split("\n"),
    isCommentOffset,
    isStringOffset,
  };
}

export function checkLuauSyntaxDetailed(source: string): { line: number; code: string; message: string } | null {
  if (!source.trim()) return { line: 1, code: "LUAU_EMPTY", message: "Empty Luau script" };
  let line = 1;
  let i = 0;
  const delims: Array<[string, number]> = [];
  const blocks: Array<[string, number]> = [];
  type LexTok = { kind: "ident" | "punct" | "str"; value: string; line: number; isMember: boolean };
  const tokens: LexTok[] = [];

  while (i < source.length) {
    const c = source[i];
    if (c === "\n") {
      line++;
      i++;
      continue;
    }
    if (/\s/.test(c)) {
      i++;
      continue;
    }
    if (c === "-" && source[i + 1] === "-") {
      const eq = matchLongBracketOpen(source, i + 2);
      if (eq !== null) {
        const startLine = line;
        const close = findLongBracketClose(source, i + 4 + eq, eq);
        if (!close) return { line: startLine, code: "LUAU_COMMENT", message: "Unclosed block comment --[[" };
        line += close.newlines;
        i = close.end;
      } else {
        while (i < source.length && source[i] !== "\n") i++;
      }
      continue;
    }
    if (c === '"' || c === "'" || c === "`") {
      const startLine = line;
      const q = c;
      i++;
      let closed = false;
      while (i < source.length) {
        if (source[i] === "\\" && i + 1 < source.length) {
          if (source[i + 1] === "\n") line++;
          i += 2;
          continue;
        }
        if (source[i] === "\n" && q !== "`") return { line: startLine, code: "LUAU_STRING", message: "Unclosed string literal" };
        if (source[i] === "\n") line++;
        if (source[i] === q) {
          i++;
          closed = true;
          break;
        }
        i++;
      }
      if (!closed) return { line: startLine, code: "LUAU_STRING", message: "Unclosed string literal" };
      tokens.push({ kind: "str", value: "", line: startLine, isMember: false });
      continue;
    }
    const longStrEq = matchLongBracketOpen(source, i);
    if (longStrEq !== null) {
      const startLine = line;
      const close = findLongBracketClose(source, i + 2 + longStrEq, longStrEq);
      if (!close) return { line: startLine, code: "LUAU_LONG_STR", message: "Unclosed long string [[" };
      line += close.newlines;
      i = close.end;
      tokens.push({ kind: "str", value: "", line: startLine, isMember: false });
      continue;
    }
    if (c === "(" || c === "[" || c === "{") {
      delims.push([c, line]);
      tokens.push({ kind: "punct", value: c, line, isMember: false });
      i++;
      continue;
    }
    if (c === ")" || c === "]" || c === "}") {
      const want = c === ")" ? "(" : c === "]" ? "[" : "{";
      const top = delims.pop();
      if (!top) return { line, code: "LUAU_DELIM", message: `Unexpected closing delimiter '${c}'` };
      if (top[0] !== want) {
        return {
          line,
          code: "LUAU_DELIM",
          message: `Mismatched delimiter '${c}' (opened '${top[0]}' on line ${top[1]})`,
        };
      }
      tokens.push({ kind: "punct", value: c, line, isMember: false });
      i++;
      continue;
    }
    if (IDENT_START_RE.test(c)) {
      const start = i;
      const prevMember = start > 0 && (source[start - 1] === "." || source[start - 1] === ":");
      while (i < source.length && IDENT_CHAR_RE.test(source[i])) i++;
      tokens.push({ kind: "ident", value: source.slice(start, i), line, isMember: prevMember });
      continue;
    }
    const two = source.slice(i, i + 2);
    if (["..", "==", "~=", "<=", ">=", "+=", "-=", "*=", "/=", "%=", "^="].includes(two)) {
      tokens.push({ kind: "punct", value: two, line, isMember: false });
      i += 2;
      continue;
    }
    tokens.push({ kind: "punct", value: c, line, isMember: false });
    i++;
  }

  if (delims.length) {
    const [ch, l] = delims[delims.length - 1];
    return { line: l, code: "LUAU_DELIM", message: `Unclosed delimiter '${ch}'` };
  }

  const IF_EXPR_PREV_PUNCT = new Set([
    "=", "(", ",", "{", "+", "-", "*", "/", "%", "^", "..",
    "==", "~=", "<", "<=", ">", ">=", "+=", "-=", "*=", "/=",
  ]);
  const IF_EXPR_PREV_IDENT = new Set(["return", "and", "or", "not"]);
  const isIfExpressionContext = (prev: LexTok | undefined): boolean => {
    if (!prev) return false;
    if (prev.kind === "punct") return IF_EXPR_PREV_PUNCT.has(prev.value);
    if (prev.kind === "ident" && !prev.isMember) return IF_EXPR_PREV_IDENT.has(prev.value);
    return false;
  };

  let expectLoopDo = false;
  for (let idx = 0; idx < tokens.length; idx++) {
    const tok = tokens[idx];
    if (tok.kind !== "ident" || tok.isMember) continue;
    const word = tok.value;
    const tokLine = tok.line;
    if (word === "function") blocks.push(["function", tokLine]);
    else if (word === "if") {
      if (!isIfExpressionContext(tokens[idx - 1])) {
        blocks.push(["if", tokLine]);
      }
    } else if (word === "for" || word === "while") {
      blocks.push([word, tokLine]);
      expectLoopDo = true;
    } else if (word === "do") {
      if (expectLoopDo) expectLoopDo = false;
      else blocks.push(["do", tokLine]);
    } else if (word === "repeat") blocks.push(["repeat", tokLine]);
    else if (word === "until") {
      const top = blocks.pop();
      if (!top || top[0] !== "repeat") return { line: tokLine, code: "LUAU_BLOCK", message: "Unexpected 'until'" };
    } else if (word === "end") {
      const top = blocks.pop();
      if (!top) return { line: tokLine, code: "LUAU_BLOCK", message: "Unexpected 'end' with no matching block" };
      if (top[0] === "repeat") {
        return { line: tokLine, code: "LUAU_BLOCK", message: `Expected 'until' for 'repeat' from line ${top[1]}` };
      }
    }
  }
  if (blocks.length) {
    const [kind, l] = blocks[blocks.length - 1];
    return { line: l, code: "LUAU_BLOCK", message: `Unclosed '${kind}' block (missing 'end')` };
  }
  return null;
}

function findLineOf(source: string, needle: RegExp): number {
  const lines = source.split("\n");
  for (let i = 0; i < lines.length; i++) {
    if (needle.test(lines[i])) return i + 1;
  }
  return 1;
}

export function analyzePlaceLuau(place: Place): LuauDiagnostic[] {
  const diagnostics: LuauDiagnostic[] = [];
  const scannedByPath = new Map<string, ScannedLuau>();
  for (const [p, s] of Object.entries(place.scripts || {})) {
    scannedByPath.set(p, scanLuauCode(s));
  }
  const allCodeNoComments = [...scannedByPath.values()].map((s) => s.codeNoCommentsSrc);
  const createsName = (name: string) =>
    allCodeNoComments.some((s) => s.includes(`Name = "${name}"`) || s.includes(`Name = '${name}'`));
  const createsRemote = (name?: string) =>
    allCodeNoComments.some(
      (s) =>
        (s.includes('Instance.new("RemoteEvent")') || s.includes("Instance.new('RemoteEvent')")) &&
        (!name || s.includes(`"${name}"`) || s.includes(`'${name}'`)),
    );

  for (const [path, source] of Object.entries(place.scripts || {})) {
    const scanned = scannedByPath.get(path)!;
    const codeOnly = scanned.codeOnlySrc;
    const codeNoComments = scanned.codeNoCommentsSrc;
    const isProjectFile = path.includes("/") || /\.luau?$/i.test(path);
    const root = path.split(".")[0] || "";
    if (!isProjectFile && !VALID_SERVICES.has(root)) {
      diagnostics.push({
        path,
        line: 1,
        severity: "error",
        code: "ROBLOX_ROOT",
        message: `Invalid Roblox service root '${root}'`,
      });
    }

    const syn = checkLuauSyntaxDetailed(source);
    if (syn) {
      diagnostics.push({
        path,
        line: syn.line,
        severity: "error",
        code: syn.code,
        message: syn.message,
      });
    }

    // Check infinite `while true do` without task.wait / wait
    if (/\bwhile\s+true\s+do\b/.test(codeOnly) && !/\b(?:task\.wait|wait)\s*\(/.test(codeOnly)) {
      diagnostics.push({
        path,
        line: findLineOf(codeOnly, /\bwhile\s+true\s+do\b/),
        severity: "error",
        code: "LUAU_INFINITE_LOOP",
        message: "Infinite 'while true do' loop missing task.wait() yield",
      });
    }

    // Check DataStore calls missing pcall or BindToClose
    if (/\b(?:GetAsync|SetAsync|UpdateAsync)\s*\(/.test(codeOnly) && !/\bpcall\s*\(/.test(codeOnly)) {
      diagnostics.push({
        path,
        line: findLineOf(codeOnly, /\b(?:GetAsync|SetAsync|UpdateAsync)\s*\(/),
        severity: "warning",
        code: "DS_NO_PCALL",
        message: "DataStoreService call should be wrapped in pcall()",
      });
    }
    if (codeOnly.includes("GetDataStore(") && !codeOnly.includes("BindToClose")) {
      diagnostics.push({
        path,
        line: findLineOf(codeOnly, /GetDataStore\(/),
        severity: "error",
        code: "DS_NO_BINDTOCLOSE",
        message: "DataStore script missing game:BindToClose shutdown save",
      });
    }

    // Check OnServerEvent with extra arguments missing validation
    const serverEventMatch = codeOnly.match(
      new RegExp(`OnServerEvent:Connect\\s*\\(\\s*function\\s*\\(\\s*${IDENT_PAT}\\s*,\\s*(${IDENT_PAT})`, "u"),
    );
    if (serverEventMatch) {
      const hasCheck = /\btypeof\s*\(|\bif\s+not\b|\btype\s*\(/.test(codeOnly);
      if (!hasCheck) {
        diagnostics.push({
          path,
          line: findLineOf(codeOnly, /OnServerEvent:Connect/),
          severity: "warning",
          code: "NET_UNVALIDATED",
          message: `OnServerEvent handler accepts client arg '${serverEventMatch[1]}' without typeof/guard check`,
        });
      }
    }

    // Check workspace.<Member> references
    const wsRe = new RegExp(`(?<![\\p{L}\\p{N}_])workspace\\.(${IDENT_PAT})`, "gu");
    for (const m of codeOnly.matchAll(wsRe)) {
      const member = m[1];
      if (WORKSPACE_BUILTINS.has(member)) continue;
      if (!place.instances[`Workspace.${member}`] && !createsName(member)) {
        diagnostics.push({
          path,
          line: findLineOf(codeOnly, new RegExp(`(?<![\\p{L}\\p{N}_])workspace\\.${escapeRegExp(member)}(?![\\p{L}\\p{N}_])`, "u")),
          severity: "error",
          code: "XREF_WORKSPACE",
          message: `Workspace.${member} is not present in Workspace or created by a script`,
        });
      }
    }

    // Check WaitForChild("...") on ReplicatedStorage remotes (excluding known ModuleScripts)
    const hasScriptNamed = (name: string) =>
      Object.keys(place.scripts || {}).some(
        (k) => k === `ReplicatedStorage.${name}` || scriptShortStem(k).toLowerCase() === name.toLowerCase(),
      );
    for (const m of codeNoComments.matchAll(/ReplicatedStorage:WaitForChild\(\s*["']([^"']+)["']\s*\)/g)) {
      const remoteName = m[1];
      if (!createsRemote(remoteName) && !place.instances[`ReplicatedStorage.${remoteName}`] && !hasScriptNamed(remoteName)) {
        diagnostics.push({
          path,
          line: findLineOf(codeNoComments, new RegExp(escapeRegExp(remoteName))),
          severity: "error",
          code: "XREF_REMOTE",
          message: `ReplicatedStorage.${remoteName} RemoteEvent was never created`,
        });
      }
    }

    // Check Shop charging leaderstats without leaderstats creator
    if (
      (codeNoComments.includes('Instance.new("RemoteEvent")') || codeNoComments.includes("Instance.new('RemoteEvent')")) &&
      codeNoComments.includes("leaderstats") &&
      !createsName("leaderstats")
    ) {
      diagnostics.push({
        path,
        line: findLineOf(codeNoComments, /leaderstats/),
        severity: "error",
        code: "XREF_LEADERSTATS",
        message: "Shop script references leaderstats but no script creates it",
      });
    }
  }

  for (const path of Object.keys(place.instances || {})) {
    const root = path.split(".")[0] || "";
    if (!VALID_SERVICES.has(root)) {
      diagnostics.push({
        path,
        line: 1,
        severity: "error",
        code: "ROBLOX_ROOT",
        message: `Invalid Roblox service root '${root}'`,
      });
    }
  }

  return diagnostics;
}

// --- Luau Syntax Highlighter Tokenizer ---
export type LuauTokenKind = "comment" | "string" | "keyword" | "global" | "number" | "plain";
export type LuauToken = { kind: LuauTokenKind; text: string };

const LUAU_KEYWORDS = new Set([
  "local",
  "function",
  "export",
  "type",
  "end",
  "if",
  "then",
  "else",
  "elseif",
  "for",
  "in",
  "do",
  "while",
  "repeat",
  "until",
  "return",
  "and",
  "or",
  "not",
  "true",
  "false",
  "nil",
  "break",
  "continue",
]);

const LUAU_GLOBALS = new Set([
  "game",
  "workspace",
  "script",
  "Instance",
  "Vector3",
  "CFrame",
  "Color3",
  "UDim2",
  "Enum",
  "PhysicalProperties",
  "task",
  "pcall",
  "ipairs",
  "pairs",
  "typeof",
  "tonumber",
  "tostring",
  "print",
  "warn",
  "math",
  "os",
  "table",
  "string",
]);

export function tokenizeLuauLine(line: string): LuauToken[] {
  const out: LuauToken[] = [];
  let i = 0;
  let plainBuf = "";

  const flushPlain = () => {
    if (plainBuf) {
      out.push({ kind: "plain", text: plainBuf });
      plainBuf = "";
    }
  };

  while (i < line.length) {
    const ch = line[i];
    if (ch === "-" && line[i + 1] === "-") {
      flushPlain();
      out.push({ kind: "comment", text: line.slice(i) });
      break;
    }
    if (ch === '"' || ch === "'") {
      flushPlain();
      const q = ch;
      let j = i + 1;
      while (j < line.length) {
        if (line[j] === "\\" && j + 1 < line.length) {
          j += 2;
          continue;
        }
        if (line[j] === q) {
          j++;
          break;
        }
        j++;
      }
      out.push({ kind: "string", text: line.slice(i, j) });
      i = j;
      continue;
    }
    if (/[0-9]/.test(ch) && (i === 0 || !IDENT_CHAR_RE.test(line[i - 1]))) {
      flushPlain();
      let j = i;
      while (j < line.length && /[0-9._xXa-fA-F]/.test(line[j])) j++;
      out.push({ kind: "number", text: line.slice(i, j) });
      i = j;
      continue;
    }
    if (IDENT_START_RE.test(ch)) {
      let j = i;
      while (j < line.length && IDENT_CHAR_RE.test(line[j])) j++;
      const word = line.slice(i, j);
      if (LUAU_KEYWORDS.has(word)) {
        flushPlain();
        out.push({ kind: "keyword", text: word });
      } else if (LUAU_GLOBALS.has(word)) {
        flushPlain();
        out.push({ kind: "global", text: word });
      } else {
        plainBuf += word;
      }
      i = j;
      continue;
    }
    plainBuf += ch;
    i++;
  }
  flushPlain();
  return out;
}

// --- Place Build Diff Calculator ---
function stableInstJson(inst: PlaceInstance | undefined): string {
  if (!inst) return "";
  return JSON.stringify({ class_name: inst.class_name || "", properties: inst.properties || {} });
}

export function computePlaceDiff(prev: Place | null, next: Place | null): PlaceDiff {
  const added: PlaceDiffItem[] = [];
  const modified: PlaceDiffItem[] = [];
  const removed: PlaceDiffItem[] = [];
  if (!next) return { added, modified, removed };

  const prevInsts = prev?.instances || {};
  const nextInsts = next.instances || {};
  const prevScripts = prev?.scripts || {};
  const nextScripts = next.scripts || {};

  for (const [path, inst] of Object.entries(nextInsts)) {
    if (!(path in prevInsts)) {
      added.push({ kind: "instance", path, detail: inst.class_name || "Instance" });
    } else if (stableInstJson(prevInsts[path]) !== stableInstJson(inst)) {
      modified.push({ kind: "instance", path, detail: `${inst.class_name || "Instance"} props updated` });
    }
  }
  for (const [path, inst] of Object.entries(prevInsts)) {
    if (!(path in nextInsts)) {
      removed.push({ kind: "instance", path, detail: inst.class_name || "Instance" });
    }
  }

  for (const [path, src] of Object.entries(nextScripts)) {
    const lines = src.trim() ? src.trim().split("\n").length : 0;
    if (!(path in prevScripts)) {
      added.push({ kind: "script", path, detail: `+${lines}L Luau` });
    } else if (prevScripts[path] !== src) {
      const oldLines = prevScripts[path].trim() ? prevScripts[path].trim().split("\n").length : 0;
      const delta = lines - oldLines;
      modified.push({
        kind: "script",
        path,
        detail: `${delta >= 0 ? "+" : ""}${delta}L (${lines}L Luau)`,
      });
    }
  }
  for (const [path, src] of Object.entries(prevScripts)) {
    if (!(path in nextScripts)) {
      const lines = src.trim() ? src.trim().split("\n").length : 0;
      removed.push({ kind: "script", path, detail: `-${lines}L Luau` });
    }
  }

  return { added, modified, removed };
}

// --- Client-Side Luau Symbol Index, Cross-File Contract Prover & Definition Lookup ---

function stripScriptExt(p: string): string {
  return p
    .replace(/\\/g, "/")
    .replace(/(?:\.(?:server|client))?\.luau?$/i, "")
    .replace(/\/init$/i, "");
}

function scriptShortStem(p: string): string {
  const clean = stripScriptExt(p);
  const slash = clean.split("/").pop() || clean;
  return slash.split(".").pop() || slash;
}

function splitTopLevelCommas(raw: string): string[] {
  const trimmed = raw.trim();
  if (!trimmed) return [];
  const parts: string[] = [];
  let depthParen = 0;
  let depthBrace = 0;
  let depthBracket = 0;
  let quote: string | null = null;
  let start = 0;
  for (let i = 0; i < trimmed.length; i++) {
    const ch = trimmed[i];
    if (quote) {
      if (ch === "\\" && i + 1 < trimmed.length) {
        i++;
        continue;
      }
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === "`") {
      quote = ch;
      continue;
    }
    if (ch === "(") depthParen++;
    else if (ch === ")") depthParen = Math.max(0, depthParen - 1);
    else if (ch === "{") depthBrace++;
    else if (ch === "}") depthBrace = Math.max(0, depthBrace - 1);
    else if (ch === "[") depthBracket++;
    else if (ch === "]") depthBracket = Math.max(0, depthBracket - 1);
    else if (ch === "," && depthParen === 0 && depthBrace === 0 && depthBracket === 0) {
      parts.push(trimmed.slice(start, i).trim());
      start = i + 1;
    }
  }
  const tail = trimmed.slice(start).trim();
  if (tail) parts.push(tail);
  return parts;
}

function parseParamArity(paramsRaw: string, isMethodDef: boolean): { min: number; max: number | null } {
  const params = splitTopLevelCommas(paramsRaw);
  let min = 0;
  let max: number | null = 0;
  for (let i = 0; i < params.length; i++) {
    const p = params[i];
    if (!p) continue;
    if (i === 0 && !isMethodDef && (p === "self" || p.startsWith("self:"))) {
      continue;
    }
    if (p.startsWith("...")) {
      max = null;
      continue;
    }
    const typeSplit = p.split(":");
    const typePart = typeSplit.slice(1).join(":").trim();
    const isOptional = typePart.endsWith("?") || typePart.includes("| nil") || typePart.includes("nil |");
    if (!isOptional) min++;
    if (max !== null) max++;
  }
  return { min, max };
}

function resolveRequireTarget(expr: string, fromPath: string, scriptPaths: string[]): string | null {
  const stringArgs = [...expr.matchAll(/["']([^"']+)["']/g)].map((m) => m[1]);
  const identParts = expr
    .replace(/game:GetService\(\s*["'][^"']+["']\s*\)/g, "")
    .replace(/WaitForChild|FindFirstChild/g, " ")
    .match(new RegExp(IDENT_PAT, "gu"));

  const candidates: string[] = [];
  if (stringArgs.length > 0) {
    candidates.push(stringArgs[stringArgs.length - 1]);
  }
  if (identParts && identParts.length > 0) {
    const filtered = identParts.filter((w) => w !== "game" && w !== "script" && w !== "Parent" && w !== "ReplicatedStorage" && w !== "ServerStorage" && w !== "ServerScriptService");
    if (filtered.length > 0) candidates.push(filtered[filtered.length - 1]);
  }

  for (const cand of candidates) {
    const lower = cand.toLowerCase();
    // Prefer same directory / relative match first, then stem match
    const fromDir = fromPath.includes("/") ? fromPath.slice(0, fromPath.lastIndexOf("/")) : "";
    if (fromDir) {
      const sibling = scriptPaths.find(
        (p) => p !== fromPath && p.startsWith(`${fromDir}/`) && scriptShortStem(p).toLowerCase() === lower,
      );
      if (sibling) return sibling;
    }
    const exactStem = scriptPaths.find((p) => p !== fromPath && scriptShortStem(p).toLowerCase() === lower);
    if (exactStem) return exactStem;
    const pathMatch = scriptPaths.find((p) => p !== fromPath && stripScriptExt(p).toLowerCase().endsWith(lower));
    if (pathMatch) return pathMatch;
  }
  return null;
}

type ParsedFunctionMeta = {
  symbol: LuauSymbol;
  minArgs: number;
  maxArgs: number | null;
  isMethod: boolean;
  paramTypes: string[];
};

type RemoteEndpoint = {
  remoteName: string;
  path: string;
  line: number;
  direction: "call" | "handler";
  payloadCount: number;
  kind: "remote_event" | "remote_function";
};

function parseRemoteMap(scripts: Record<string, string>, instances?: Record<string, PlaceInstance>) {
  const remoteDefs = new Map<string, LuauSymbol>();
  for (const [dmPath, inst] of Object.entries(instances || {})) {
    if (inst.class_name === "RemoteEvent" || inst.class_name === "RemoteFunction") {
      const name = dmPath.split(".").pop() || dmPath;
      remoteDefs.set(name, {
        name,
        kind: inst.class_name === "RemoteFunction" ? "remote_function" : "remote_event",
        path: dmPath,
        line: 1,
        signature: inst.class_name,
      });
    }
  }
  for (const [path, src] of Object.entries(scripts)) {
    const scanned = scanLuauCode(src);
    const codeOnlyLines = scanned.codeOnlyLines;
    const codeNoCommentsLines = scanned.codeNoCommentsLines;
    const varToKind = new Map<string, "remote_event" | "remote_function">();
    const newRe = new RegExp(`(?<![\\p{L}\\p{N}_])local\\s+(${IDENT_PAT})\\s*=\\s*Instance\\.new\\(\\s*["'](RemoteEvent|RemoteFunction)["']\\s*\\)`, "u");
    const nameAssignRe = new RegExp(`(?<![\\p{L}\\p{N}_])(${IDENT_PAT})\\.Name\\s*=\\s*["']([^"']+)["']`, "u");
    for (let i = 0; i < codeNoCommentsLines.length; i++) {
      if (!codeOnlyLines[i]?.trim()) continue;
      const ln = codeNoCommentsLines[i];
      const newMatch = newRe.exec(ln);
      if (newMatch) {
        const kind = newMatch[2] === "RemoteFunction" ? "remote_function" : "remote_event";
        varToKind.set(newMatch[1], kind);
        const inlineName = new RegExp(`(?<![\\p{L}\\p{N}_])${escapeRegExp(newMatch[1])}\\.Name\\s*=\\s*["']([^"']+)["']`, "u").exec(scanned.codeNoCommentsSrc);
        const rName = inlineName ? inlineName[1] : newMatch[1];
        remoteDefs.set(rName, {
          name: rName,
          kind,
          path,
          line: i + 1,
          signature: kind === "remote_function" ? "RemoteFunction" : "RemoteEvent",
        });
      }
      const nameAssign = nameAssignRe.exec(ln);
      if (nameAssign && varToKind.has(nameAssign[1])) {
        const kind = varToKind.get(nameAssign[1])!;
        remoteDefs.set(nameAssign[2], {
          name: nameAssign[2],
          kind,
          path,
          line: i + 1,
          signature: kind === "remote_function" ? "RemoteFunction" : "RemoteEvent",
        });
      }
    }
  }
  return remoteDefs;
}

function extractBalancedCallArg(line: string, openParenIdx: number): string {
  if (openParenIdx < 0 || line[openParenIdx] !== "(") return "";
  let depth = 1;
  let quote: string | null = null;
  for (let i = openParenIdx + 1; i < line.length; i++) {
    const ch = line[i];
    if (quote) {
      if (ch === "\\" && i + 1 < line.length) {
        i++;
        continue;
      }
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === "`") {
      quote = ch;
      continue;
    }
    if (ch === "(") depth++;
    else if (ch === ")") {
      depth--;
      if (depth === 0) return line.slice(openParenIdx + 1, i);
    }
  }
  return line.slice(openParenIdx + 1);
}

function extractRequireCallArg(codeOnlyLine: string, codeNoCommentLine: string, alias?: string): { alias: string; expr: string } | null {
  const reqRe = alias
    ? new RegExp(`(?<![\\p{L}\\p{N}_])local\\s+(${escapeRegExp(alias)})\\s*=\\s*require\\s*\\(`, "u")
    : new RegExp(`(?<![\\p{L}\\p{N}_])local\\s+(${IDENT_PAT})\\s*=\\s*require\\s*\\(`, "u");
  const m = reqRe.exec(codeOnlyLine);
  if (!m) return null;
  const openIdx = codeNoCommentLine.indexOf("(", m.index + m[0].length - 1);
  if (openIdx < 0) return null;
  return { alias: m[1], expr: extractBalancedCallArg(codeNoCommentLine, openIdx) };
}

export function buildWorkspaceSymbolIndex(
  scripts: Record<string, string>,
  instances?: Record<string, PlaceInstance>,
): WorkspaceSymbolIndex {
  const symbols: LuauSymbol[] = [];
  const mismatches: LuauContractMismatch[] = [];
  const diagnostics = analyzePlaceLuau({ scripts, instances: instances || {} });
  const scriptPaths = Object.keys(scripts);

  const exportsByPath = new Map<string, Map<string, ParsedFunctionMeta>>();
  const exportKeysByPath = new Map<string, Set<string>>();
  const remoteDefs = parseRemoteMap(scripts, instances);
  for (const sym of remoteDefs.values()) {
    if (sym.path in scripts) symbols.push(sym);
  }

  const scannedByPath = new Map<string, ScannedLuau>();
  for (const [path, src] of Object.entries(scripts)) {
    scannedByPath.set(path, scanLuauCode(src));
  }

  const topReturnRe = new RegExp(`^return\\s+(${IDENT_PAT})\\s*$`, "gmu");
  const anyReturnRe = new RegExp(`(?<![\\p{L}\\p{N}_])return\\s+(${IDENT_PAT})\\s*$`, "gmu");
  const typeRe = new RegExp(`^(export\\s+)?type\\s+(${IDENT_PAT}(?:<[^>]+>)?)\\s*=\\s*(.+)$`, "u");
  const modFnRe = new RegExp(`^(?:local\\s+)?function\\s+(${IDENT_PAT})([.:])(${IDENT_PAT})\\s*\\(([^)]*)\\)(?:\\s*:\\s*([^\\n]+))?`, "u");
  const localFnRe = new RegExp(`^(local\\s+)?function\\s+(${IDENT_PAT})\\s*\\(([^)]*)\\)(?:\\s*:\\s*([^\\n]+))?`, "u");
  const assignFnRe = new RegExp(`^(${IDENT_PAT})\\.(${IDENT_PAT})\\s*=\\s*function\\s*\\(([^)]*)\\)`, "u");
  const retTableRe = /(?<![\p{L}\p{N}_])return\s*\{([\s\S]*?)\}\s*$/mu;
  const kvEntryRe = new RegExp(`^(${IDENT_PAT})(?:\\s*=\\s*(${IDENT_PAT}))?$`, "u");

  // Pass 1: extract symbols & exports per file
  for (const [path] of Object.entries(scripts)) {
    const scanned = scannedByPath.get(path)!;
    const codeLines = scanned.codeOnlyLines;
    const noCommentLines = scanned.codeNoCommentsLines;
    const fnMap = new Map<string, ParsedFunctionMeta>();
    const exportNames = new Set<string>();
    const localFns = new Map<string, ParsedFunctionMeta>();

    // Track returned module identifier (e.g. `return Economy`) from comment/string-stripped code
    const topReturnMatches = [...scanned.codeOnlySrc.matchAll(topReturnRe)];
    const anyReturnMatches = topReturnMatches.length > 0 ? topReturnMatches : [...scanned.codeOnlySrc.matchAll(anyReturnRe)];
    const moduleTableName =
      [...anyReturnMatches]
        .reverse()
        .map((m) => m[1])
        .find((id) => id !== "true" && id !== "false" && id !== "nil") ?? null;

    for (let i = 0; i < codeLines.length; i++) {
      const lineNo = i + 1;
      const trimmed = (codeLines[i] || "").trim();
      if (!trimmed) continue;
      const noCommentTrimmed = (noCommentLines[i] || "").trim();

      const typeMatch = typeRe.exec(noCommentTrimmed);
      if (typeMatch && typeRe.test(trimmed)) {
        const cleanName = typeMatch[2].replace(/<.*>$/, "");
        symbols.push({
          name: cleanName,
          kind: "type",
          path,
          line: lineNo,
          signature: typeMatch[3].trim().slice(0, 64),
        });
        if (typeMatch[1]) exportNames.add(cleanName);
        continue;
      }

      const modFnMatch = modFnRe.exec(noCommentTrimmed);
      if (modFnMatch && /^(?:local\s+)?function\b/.test(trimmed)) {
        const tbl = modFnMatch[1];
        const sep = modFnMatch[2];
        const fnName = modFnMatch[3];
        const paramsRaw = modFnMatch[4] || "";
        const retType = modFnMatch[5]?.trim();
        const isExport = !moduleTableName || tbl === moduleTableName || tbl === "M" || tbl === "Module";
        const arity = parseParamArity(paramsRaw, sep === ":");
        const paramTypes = splitTopLevelCommas(paramsRaw).map((p) => (p.includes(":") ? p.split(":").slice(1).join(":").trim() : "any"));
        const sig = `(${paramsRaw.trim()})${retType ? `: ${retType}` : ""}`;
        const sym: LuauSymbol = {
          name: fnName,
          kind: isExport ? "export" : "function",
          path,
          line: lineNo,
          signature: sig,
        };
        symbols.push(sym);
        if (isExport) {
          exportNames.add(fnName);
          fnMap.set(fnName, { symbol: sym, minArgs: arity.min, maxArgs: arity.max, isMethod: sep === ":", paramTypes });
        }
        continue;
      }

      const localFnMatch = localFnRe.exec(noCommentTrimmed);
      if (localFnMatch && /^(?:local\s+)?function\b/.test(trimmed)) {
        const isLocal = Boolean(localFnMatch[1]);
        const fnName = localFnMatch[2];
        const paramsRaw = localFnMatch[3] || "";
        const retType = localFnMatch[4]?.trim();
        const arity = parseParamArity(paramsRaw, false);
        const paramTypes = splitTopLevelCommas(paramsRaw).map((p) => (p.includes(":") ? p.split(":").slice(1).join(":").trim() : "any"));
        const sig = `(${paramsRaw.trim()})${retType ? `: ${retType}` : ""}`;
        const sym: LuauSymbol = {
          name: fnName,
          kind: isLocal ? "function" : "export",
          path,
          line: lineNo,
          signature: sig,
        };
        symbols.push(sym);
        localFns.set(fnName, { symbol: sym, minArgs: arity.min, maxArgs: arity.max, isMethod: false, paramTypes });
        if (!isLocal) {
          exportNames.add(fnName);
          fnMap.set(fnName, localFns.get(fnName)!);
        }
        continue;
      }

      const assignFnMatch = assignFnRe.exec(noCommentTrimmed);
      if (assignFnMatch && assignFnRe.test(trimmed)) {
        const fnName = assignFnMatch[2];
        const paramsRaw = assignFnMatch[3] || "";
        const arity = parseParamArity(paramsRaw, false);
        const paramTypes = splitTopLevelCommas(paramsRaw).map((p) => (p.includes(":") ? p.split(":").slice(1).join(":").trim() : "any"));
        const sym: LuauSymbol = {
          name: fnName,
          kind: "export",
          path,
          line: lineNo,
          signature: `(${paramsRaw.trim()})`,
        };
        symbols.push(sym);
        exportNames.add(fnName);
        fnMap.set(fnName, { symbol: sym, minArgs: arity.min, maxArgs: arity.max, isMethod: false, paramTypes });
      }
    }

    // Detect `return { Foo = Foo, Bar = ... }` table exports
    const retTableMatch = retTableRe.exec(scanned.codeOnlySrc);
    if (retTableMatch) {
      const entries = splitTopLevelCommas(retTableMatch[1]);
      for (const entry of entries) {
        const kv = kvEntryRe.exec(entry.trim());
        if (kv) {
          const expName = kv[1];
          const refName = kv[2] || kv[1];
          exportNames.add(expName);
          if (!fnMap.has(expName)) {
            const lf = localFns.get(refName);
            if (lf) {
              fnMap.set(expName, lf);
              lf.symbol.kind = "export";
            } else {
              symbols.push({ name: expName, kind: "export", path, line: codeLines.length, signature: null });
            }
          }
        }
      }
    }

    exportsByPath.set(path, fnMap);
    exportKeysByPath.set(path, exportNames);
  }

  // Pass 2: cross-file require() contracts & RemoteEvent/RemoteFunction payload contracts
  const remoteEndpoints: RemoteEndpoint[] = [];
  const remBindRe = new RegExp(`(?<![\\p{L}\\p{N}_])local\\s+(${IDENT_PAT})\\s*=\\s*[^\\n]*?(?:WaitForChild|FindFirstChild)\\(\\s*["']([^"']+)["']\\s*\\)`, "u");
  const remBindCodeRe = new RegExp(`(?<![\\p{L}\\p{N}_])local\\s+(${IDENT_PAT})\\s*=\\s*[^\\n]*?(?:WaitForChild|FindFirstChild)\\(`, "u");

  for (const [callerPath] of Object.entries(scripts)) {
    const scanned = scannedByPath.get(callerPath)!;
    const codeLines = scanned.codeOnlyLines;
    const noCommentLines = scanned.codeNoCommentsLines;
    const requireAliases = new Map<string, string>();
    const remoteAliases = new Map<string, string>();

    for (let i = 0; i < codeLines.length; i++) {
      const lineNo = i + 1;
      const codeLine = codeLines[i] || "";
      const trimmed = codeLine.trim();
      if (!trimmed) continue;
      const noCommentLine = noCommentLines[i] || "";

      const reqMatch = extractRequireCallArg(codeLine, noCommentLine);
      if (reqMatch) {
        const target = resolveRequireTarget(reqMatch.expr, callerPath, scriptPaths);
        if (target) requireAliases.set(reqMatch.alias, target);
      }

      if (remBindCodeRe.test(codeLine)) {
        const remBindMatch = remBindRe.exec(noCommentLine);
        if (remBindMatch && (remoteDefs.has(remBindMatch[2]) || /Remote|Event|Func|Buy|Shop|Roll/i.test(remBindMatch[2]))) {
          remoteAliases.set(remBindMatch[1], remBindMatch[2]);
        }
      }

      // Check require() alias calls: Alias.Fn(args) or Alias:Fn(args)
      for (const [alias, targetPath] of requireAliases.entries()) {
        const escAlias = escapeRegExp(alias);
        const callRe = new RegExp(`(?<![\\p{L}\\p{N}_])${escAlias}([.:])(${IDENT_PAT})\\s*\\(([^)]*)\\)`, "gu");
        for (const m of trimmed.matchAll(callRe)) {
          const isColon = m[1] === ":";
          const fnName = m[2];
          const args = splitTopLevelCommas(m[3] || "");
          const targetExports = exportKeysByPath.get(targetPath);
          const targetFns = exportsByPath.get(targetPath);
          if (targetExports && targetExports.size > 0 && !targetExports.has(fnName)) {
            mismatches.push({
              caller_path: callerPath,
              caller_line: lineNo,
              target_path: targetPath,
              kind: "missing_export",
              message: `${scriptShortStem(targetPath)} does not export '${fnName}'`,
            });
            continue;
          }
          const fnMeta = targetFns?.get(fnName);
          if (fnMeta) {
            // If method is defined with `:` and called with `.`, caller must pass `self` (+1 arg)
            const passedCount = isColon && !fnMeta.isMethod ? args.length + 1 : !isColon && fnMeta.isMethod ? Math.max(0, args.length - 1) : args.length;
            if (passedCount < fnMeta.minArgs || (fnMeta.maxArgs !== null && passedCount > fnMeta.maxArgs)) {
              const expectedStr = fnMeta.maxArgs === null ? `${fnMeta.minArgs}+` : fnMeta.minArgs === fnMeta.maxArgs ? `${fnMeta.minArgs}` : `${fnMeta.minArgs}..${fnMeta.maxArgs}`;
              mismatches.push({
                caller_path: callerPath,
                caller_line: lineNo,
                target_path: targetPath,
                kind: "arity_mismatch",
                message: `'${fnName}' expects ${expectedStr} argument(s), got ${passedCount}`,
              });
            }
          }
        }
      }

      // Collect RemoteEvent / RemoteFunction callers and handlers
      for (const [alias, remoteName] of remoteAliases.entries()) {
        const escAlias = escapeRegExp(alias);
        const fireServer = new RegExp(`(?<![\\p{L}\\p{N}_])${escAlias}:(FireServer|InvokeServer)\\s*\\(([^)]*)\\)`, "u").exec(trimmed);
        if (fireServer) {
          remoteEndpoints.push({
            remoteName,
            path: callerPath,
            line: lineNo,
            direction: "call",
            payloadCount: splitTopLevelCommas(fireServer[2] || "").length,
            kind: fireServer[1] === "InvokeServer" ? "remote_function" : "remote_event",
          });
        }
        const fireClient = new RegExp(`(?<![\\p{L}\\p{N}_])${escAlias}:FireClient\\s*\\(([^)]*)\\)`, "u").exec(trimmed);
        if (fireClient) {
          const args = splitTopLevelCommas(fireClient[1] || "");
          remoteEndpoints.push({
            remoteName,
            path: callerPath,
            line: lineNo,
            direction: "call",
            payloadCount: Math.max(0, args.length - 1),
            kind: "remote_event",
          });
        }
        const srvHandler = new RegExp(`(?<![\\p{L}\\p{N}_])${escAlias}\\.OnServerEvent:Connect\\s*\\(\\s*function\\s*\\(([^)]*)\\)`, "u").exec(trimmed);
        if (srvHandler) {
          const params = splitTopLevelCommas(srvHandler[1] || "");
          remoteEndpoints.push({
            remoteName,
            path: callerPath,
            line: lineNo,
            direction: "handler",
            payloadCount: Math.max(0, params.length - 1),
            kind: "remote_event",
          });
        }
        const srvInvoke = new RegExp(`(?<![\\p{L}\\p{N}_])${escAlias}\\.OnServerInvoke\\s*=\\s*function\\s*\\(([^)]*)\\)`, "u").exec(trimmed);
        if (srvInvoke) {
          const params = splitTopLevelCommas(srvInvoke[1] || "");
          remoteEndpoints.push({
            remoteName,
            path: callerPath,
            line: lineNo,
            direction: "handler",
            payloadCount: Math.max(0, params.length - 1),
            kind: "remote_function",
          });
        }
      }
    }
  }

  // Compare remote calls vs handlers
  const handlersByRemote = new Map<string, RemoteEndpoint[]>();
  for (const ep of remoteEndpoints) {
    if (ep.direction === "handler") {
      const list = handlersByRemote.get(ep.remoteName) || [];
      list.push(ep);
      handlersByRemote.set(ep.remoteName, list);
    }
  }
  for (const ep of remoteEndpoints) {
    if (ep.direction !== "call") continue;
    const handlers = handlersByRemote.get(ep.remoteName);
    if (!handlers || handlers.length === 0) continue;
    const h = handlers[0];
    if (ep.payloadCount !== h.payloadCount) {
      mismatches.push({
        caller_path: ep.path,
        caller_line: ep.line,
        target_path: h.path,
        kind: "remote_arg_mismatch",
        message: `Remote '${ep.remoteName}' expects ${h.payloadCount} payload arg(s), caller passes ${ep.payloadCount}`,
      });
    }
  }

  return {
    ok: diagnostics.every((d) => d.severity !== "error") && mismatches.length === 0,
    luau_analyze_available: false,
    diagnostics_source: "builtin",
    symbols,
    mismatches,
    diagnostics,
  };
}

function identAtColumn(lineText: string, col1: number): { word: string; qualifier: string | null } {
  if (!lineText) return { word: "", qualifier: null };
  const idx = Math.max(0, Math.min(lineText.length - 1, col1 - 1));
  let s = idx;
  let e = idx;
  while (s > 0 && IDENT_CHAR_RE.test(lineText[s - 1])) s--;
  while (e < lineText.length && IDENT_CHAR_RE.test(lineText[e])) e++;
  const word = lineText.slice(s, e);
  let qualifier: string | null = null;
  if (s >= 2 && (lineText[s - 1] === "." || lineText[s - 1] === ":")) {
    let qs = s - 1;
    while (qs > 0 && IDENT_CHAR_RE.test(lineText[qs - 1])) qs--;
    const q = lineText.slice(qs, s - 1);
    if (q) qualifier = q;
  }
  return { word, qualifier };
}

function lineColToOffset(src: string, line1: number, col1: number): number {
  const lines = src.split("\n");
  const row = Math.max(1, Math.min(line1, lines.length));
  let offset = 0;
  for (let i = 0; i < row - 1; i++) {
    offset += lines[i].length + 1;
  }
  const lineLen = lines[row - 1]?.length ?? 0;
  return offset + Math.max(0, Math.min(Math.max(0, col1 - 1), lineLen));
}

export function resolveLuauDefinitionClient(
  scripts: Record<string, string>,
  path: string,
  line: number,
  col: number,
  instances?: Record<string, PlaceInstance>,
): LuauDefinitionResult {
  const src = scripts[path];
  if (src === undefined) {
    return { ok: false, target_path: null, line: 1, col: 1, symbol: null };
  }
  const scanned = scanLuauCode(src);
  const offset = lineColToOffset(src, line, col);
  if (scanned.isCommentOffset[offset] === 1) {
    return { ok: false, target_path: null, line: 1, col: 1, symbol: null };
  }

  const index = buildWorkspaceSymbolIndex(scripts, instances);
  const lines = src.split("\n");
  const lineIdx = Math.max(0, Math.min(lines.length - 1, line - 1));
  const lineText = lines[lineIdx] || "";
  const codeOnlyLine = scanned.codeOnlyLines[lineIdx] || "";
  const codeNoCommentLine = scanned.codeNoCommentsLines[lineIdx] || "";
  const scriptPaths = Object.keys(scripts);
  const inString = scanned.isStringOffset[offset] === 1;

  // Check real-code require(...) on the current line
  const reqCodeMatch = /(?<![\p{L}\p{N}_])require\s*\(/u.exec(codeOnlyLine);
  const { word, qualifier } = identAtColumn(lineText, col);
  if (reqCodeMatch) {
    const openIdx = codeNoCommentLine.indexOf("(", reqCodeMatch.index);
    const reqExpr = openIdx >= 0 ? extractBalancedCallArg(codeNoCommentLine, openIdx) : "";
    const reqCallEnd = openIdx >= 0 ? openIdx + reqExpr.length + 2 : codeNoCommentLine.length;
    const col0 = Math.max(0, col - 1);
    if (col0 >= reqCodeMatch.index && col0 <= reqCallEnd) {
      const target = resolveRequireTarget(reqExpr, path, scriptPaths);
      if (target) {
        const sym = index.symbols.find((s) => s.path === target) || null;
        return { ok: true, target_path: target, line: sym?.line ?? 1, col: 1, symbol: sym };
      }
    }
  }

  // Check remote name inside WaitForChild / FindFirstChild / Instance.new string
  const quoted = /["']([^"']+)["']/.exec(codeNoCommentLine);
  if (quoted && /(?:WaitForChild|FindFirstChild|Instance\.new)/.test(codeOnlyLine)) {
    const rSym = index.symbols.find((s) => (s.kind === "remote_event" || s.kind === "remote_function") && s.name === quoted[1]);
    if (rSym) {
      return { ok: true, target_path: rSym.path, line: rSym.line, col: 1, symbol: rSym };
    }
  }

  // Never resolve plain identifier jumps when cursor is inside a string literal
  if (inString) {
    return { ok: false, target_path: null, line: 1, col: 1, symbol: null };
  }

  // Check qualified call like Alias.Fn or Alias:Fn
  if (qualifier && word) {
    for (let i = 0; i < scanned.codeOnlyLines.length; i++) {
      const reqMatch = extractRequireCallArg(scanned.codeOnlyLines[i], scanned.codeNoCommentsLines[i], qualifier);
      if (reqMatch) {
        const target = resolveRequireTarget(reqMatch.expr, path, scriptPaths);
        if (target) {
          const sym = index.symbols.find((s) => s.path === target && s.name === word) || null;
          return { ok: true, target_path: target, line: sym?.line ?? 1, col: 1, symbol: sym };
        }
      }
    }
  }

  if (word) {
    // Check if word is a require alias or remote alias itself
    const remLineRe = new RegExp(`(?<![\\p{L}\\p{N}_])local\\s+${escapeRegExp(word)}\\s*=\\s*[^\\n]*?(?:WaitForChild|FindFirstChild)\\(\\s*["']([^"']+)["']\\s*\\)`, "u");
    const remCodeRe = new RegExp(`(?<![\\p{L}\\p{N}_])local\\s+${escapeRegExp(word)}\\s*=\\s*[^\\n]*?(?:WaitForChild|FindFirstChild)\\(`, "u");
    for (let i = 0; i < scanned.codeOnlyLines.length; i++) {
      const reqMatch = extractRequireCallArg(scanned.codeOnlyLines[i], scanned.codeNoCommentsLines[i], word);
      if (reqMatch) {
        const target = resolveRequireTarget(reqMatch.expr, path, scriptPaths);
        if (target) {
          const sym = index.symbols.find((s) => s.path === target) || null;
          return { ok: true, target_path: target, line: sym?.line ?? 1, col: 1, symbol: sym };
        }
      }
      if (remCodeRe.test(scanned.codeOnlyLines[i])) {
        const remMatch = remLineRe.exec(scanned.codeNoCommentsLines[i]);
        if (remMatch) {
          const rSym = index.symbols.find((s) => (s.kind === "remote_event" || s.kind === "remote_function") && s.name === remMatch[1]);
          if (rSym) {
            return { ok: true, target_path: rSym.path, line: rSym.line, col: 1, symbol: rSym };
          }
        }
      }
    }

    // Same-file symbol match
    const localSym = index.symbols.find((s) => s.path === path && s.name === word);
    if (localSym) {
      return { ok: true, target_path: localSym.path, line: localSym.line, col: 1, symbol: localSym };
    }

    // Cross-file exported symbol match
    const wsSym = index.symbols.find((s) => s.name === word && (s.kind === "export" || s.kind === "type" || s.kind === "remote_event" || s.kind === "remote_function"));
    if (wsSym) {
      return { ok: true, target_path: wsSym.path, line: wsSym.line, col: 1, symbol: wsSym };
    }
  }

  return { ok: false, target_path: null, line: 1, col: 1, symbol: null };
}
