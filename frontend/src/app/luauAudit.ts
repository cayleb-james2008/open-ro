// Static Luau Analyzer, Syntax Highlighter Tokenizer, and Place Build Diff engine.
// Mirrors src/luau_check.rs checks for instant client-side diagnostics + standalone web-engine parity.
import type { Place, PlaceDiff, PlaceDiffItem, PlaceInstance } from "./data";

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

export function checkLuauSyntaxDetailed(source: string): { line: number; code: string; message: string } | null {
  if (!source.trim()) return { line: 1, code: "LUAU_EMPTY", message: "Empty Luau script" };
  let line = 1;
  let i = 0;
  const delims: Array<[string, number]> = [];
  const blocks: Array<[string, number]> = [];
  const tokens: Array<[string, number, boolean]> = [];

  while (i < source.length) {
    const c = source[i];
    if (c === "\n") {
      line++;
      i++;
      continue;
    }
    if (c === "-" && source[i + 1] === "-") {
      if (source[i + 2] === "[" && source[i + 3] === "[") {
        const startLine = line;
        i += 4;
        let closed = false;
        while (i + 1 < source.length) {
          if (source[i] === "\n") line++;
          if (source[i] === "]" && source[i + 1] === "]") {
            i += 2;
            closed = true;
            break;
          }
          i++;
        }
        if (!closed) return { line: startLine, code: "LUAU_COMMENT", message: "Unclosed block comment --[[" };
      } else {
        while (i < source.length && source[i] !== "\n") i++;
      }
      continue;
    }
    if (c === '"' || c === "'") {
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
        if (source[i] === "\n") return { line: startLine, code: "LUAU_STRING", message: "Unclosed string literal" };
        if (source[i] === q) {
          i++;
          closed = true;
          break;
        }
        i++;
      }
      if (!closed) return { line: startLine, code: "LUAU_STRING", message: "Unclosed string literal" };
      continue;
    }
    if (c === "[" && source[i + 1] === "[") {
      const startLine = line;
      i += 2;
      let closed = false;
      while (i + 1 < source.length) {
        if (source[i] === "\n") line++;
        if (source[i] === "]" && source[i + 1] === "]") {
          i += 2;
          closed = true;
          break;
        }
        i++;
      }
      if (!closed) return { line: startLine, code: "LUAU_LONG_STR", message: "Unclosed long string [[" };
      continue;
    }
    if (c === "(" || c === "[" || c === "{") {
      delims.push([c, line]);
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
      i++;
      continue;
    }
    if (/[A-Za-z_]/.test(c)) {
      const start = i;
      const prevMember = start > 0 && (source[start - 1] === "." || source[start - 1] === ":");
      while (i < source.length && /[A-Za-z0-9_]/.test(source[i])) i++;
      tokens.push([source.slice(start, i), line, prevMember]);
      continue;
    }
    i++;
  }

  if (delims.length) {
    const [ch, l] = delims[delims.length - 1];
    return { line: l, code: "LUAU_DELIM", message: `Unclosed delimiter '${ch}'` };
  }

  let expectLoopDo = false;
  for (const [word, tokLine, isMember] of tokens) {
    if (isMember) continue;
    if (word === "function") blocks.push(["function", tokLine]);
    else if (word === "if") blocks.push(["if", tokLine]);
    else if (word === "for" || word === "while") {
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
  const allSources = Object.values(place.scripts || {});
  const createsName = (name: string) =>
    allSources.some((s) => s.includes(`Name = "${name}"`) || s.includes(`Name = '${name}'`));
  const createsRemote = (name?: string) =>
    allSources.some(
      (s) =>
        (s.includes('Instance.new("RemoteEvent")') || s.includes("Instance.new('RemoteEvent')")) &&
        (!name || s.includes(`"${name}"`) || s.includes(`'${name}'`)),
    );

  for (const [path, source] of Object.entries(place.scripts || {})) {
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
    if (/\bwhile\s+true\s+do\b/.test(source) && !/\b(?:task\.wait|wait)\s*\(/.test(source)) {
      diagnostics.push({
        path,
        line: findLineOf(source, /\bwhile\s+true\s+do\b/),
        severity: "error",
        code: "LUAU_INFINITE_LOOP",
        message: "Infinite 'while true do' loop missing task.wait() yield",
      });
    }

    // Check DataStore calls missing pcall or BindToClose
    if (/\b(?:GetAsync|SetAsync|UpdateAsync)\s*\(/.test(source) && !/\bpcall\s*\(/.test(source)) {
      diagnostics.push({
        path,
        line: findLineOf(source, /\b(?:GetAsync|SetAsync|UpdateAsync)\s*\(/),
        severity: "warning",
        code: "DS_NO_PCALL",
        message: "DataStoreService call should be wrapped in pcall()",
      });
    }
    if (source.includes("GetDataStore(") && !source.includes("BindToClose")) {
      diagnostics.push({
        path,
        line: findLineOf(source, /GetDataStore\(/),
        severity: "error",
        code: "DS_NO_BINDTOCLOSE",
        message: "DataStore script missing game:BindToClose shutdown save",
      });
    }

    // Check OnServerEvent with extra arguments missing validation
    const serverEventMatch = source.match(/OnServerEvent:Connect\s*\(\s*function\s*\(\s*[A-Za-z_]\w*\s*,\s*([A-Za-z_]\w*)/);
    if (serverEventMatch) {
      const hasCheck = /\btypeof\s*\(|\bif\s+not\b|\btype\s*\(/.test(source);
      if (!hasCheck) {
        diagnostics.push({
          path,
          line: findLineOf(source, /OnServerEvent:Connect/),
          severity: "warning",
          code: "NET_UNVALIDATED",
          message: `OnServerEvent handler accepts client arg '${serverEventMatch[1]}' without typeof/guard check`,
        });
      }
    }

    // Check workspace.<Member> references
    const clean = source.replace(/--[^\n]*/g, "").replace(/"[^"\n]*"|'[^'\n]*'/g, " ");
    for (const m of clean.matchAll(/\bworkspace\.([A-Za-z_][A-Za-z0-9_]*)/g)) {
      const member = m[1];
      if (WORKSPACE_BUILTINS.has(member)) continue;
      if (!place.instances[`Workspace.${member}`] && !createsName(member)) {
        diagnostics.push({
          path,
          line: findLineOf(source, new RegExp(`\\bworkspace\\.${member}\\b`)),
          severity: "error",
          code: "XREF_WORKSPACE",
          message: `Workspace.${member} is not present in Workspace or created by a script`,
        });
      }
    }

    // Check WaitForChild("...") on ReplicatedStorage remotes
    for (const m of source.matchAll(/ReplicatedStorage:WaitForChild\(\s*["']([^"']+)["']\s*\)/g)) {
      const remoteName = m[1];
      if (!createsRemote(remoteName) && !place.instances[`ReplicatedStorage.${remoteName}`]) {
        diagnostics.push({
          path,
          line: findLineOf(source, new RegExp(remoteName)),
          severity: "error",
          code: "XREF_REMOTE",
          message: `ReplicatedStorage.${remoteName} RemoteEvent was never created`,
        });
      }
    }

    // Check Shop charging leaderstats without leaderstats creator
    if (
      (source.includes('Instance.new("RemoteEvent")') || source.includes("Instance.new('RemoteEvent')")) &&
      source.includes("leaderstats") &&
      !createsName("leaderstats")
    ) {
      diagnostics.push({
        path,
        line: findLineOf(source, /leaderstats/),
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
    if (/[0-9]/.test(ch) && (i === 0 || !/[A-Za-z0-9_]/.test(line[i - 1]))) {
      flushPlain();
      let j = i;
      while (j < line.length && /[0-9._xXa-fA-F]/.test(line[j])) j++;
      out.push({ kind: "number", text: line.slice(i, j) });
      i = j;
      continue;
    }
    if (/[A-Za-z_]/.test(ch)) {
      let j = i;
      while (j < line.length && /[A-Za-z0-9_]/.test(line[j])) j++;
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
