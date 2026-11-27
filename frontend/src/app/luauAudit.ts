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
    if (/[A-Za-z_]/.test(c)) {
      const start = i;
      const prevMember = start > 0 && (source[start - 1] === "." || source[start - 1] === ":");
      while (i < source.length && /[A-Za-z0-9_]/.test(source[i])) i++;
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

    // Check WaitForChild("...") on ReplicatedStorage remotes (excluding known ModuleScripts)
    const hasScriptNamed = (name: string) =>
      Object.keys(place.scripts || {}).some(
        (k) => k === `ReplicatedStorage.${name}` || scriptShortStem(k).toLowerCase() === name.toLowerCase(),
      );
    for (const m of source.matchAll(/ReplicatedStorage:WaitForChild\(\s*["']([^"']+)["']\s*\)/g)) {
      const remoteName = m[1];
      if (!createsRemote(remoteName) && !place.instances[`ReplicatedStorage.${remoteName}`] && !hasScriptNamed(remoteName)) {
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
    .match(/[A-Za-z_][A-Za-z0-9_]*/g);

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
    const lines = src.split("\n");
    const varToKind = new Map<string, "remote_event" | "remote_function">();
    for (let i = 0; i < lines.length; i++) {
      const ln = lines[i];
      const newMatch = /\blocal\s+([A-Za-z_]\w*)\s*=\s*Instance\.new\(\s*["'](RemoteEvent|RemoteFunction)["']\s*\)/.exec(ln);
      if (newMatch) {
        const kind = newMatch[2] === "RemoteFunction" ? "remote_function" : "remote_event";
        varToKind.set(newMatch[1], kind);
        const inlineName = new RegExp(`\\b${newMatch[1]}\\.Name\\s*=\\s*["']([^"']+)["']`).exec(src);
        const rName = inlineName ? inlineName[1] : newMatch[1];
        remoteDefs.set(rName, {
          name: rName,
          kind,
          path,
          line: i + 1,
          signature: kind === "remote_function" ? "RemoteFunction" : "RemoteEvent",
        });
      }
      const nameAssign = /\b([A-Za-z_]\w*)\.Name\s*=\s*["']([^"']+)["']/.exec(ln);
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

  // Pass 1: extract symbols & exports per file
  for (const [path, src] of Object.entries(scripts)) {
    const lines = src.split("\n");
    const fnMap = new Map<string, ParsedFunctionMeta>();
    const exportNames = new Set<string>();
    const localFns = new Map<string, ParsedFunctionMeta>();

    // Track returned module identifier (e.g. `return Economy`)
    const returnIdentMatch = /\breturn\s+([A-Za-z_]\w*)\s*(?:--.*)?$/m.exec(src);
    const moduleTableName = returnIdentMatch ? returnIdentMatch[1] : null;

    for (let i = 0; i < lines.length; i++) {
      const lineNo = i + 1;
      const raw = lines[i];
      const trimmed = raw.trim();
      if (!trimmed || trimmed.startsWith("--")) continue;

      const typeMatch = /^(export\s+)?type\s+([A-Za-z_]\w*(?:<[^>]+>)?)\s*=\s*(.+)$/.exec(trimmed);
      if (typeMatch) {
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

      const modFnMatch = /^(?:local\s+)?function\s+([A-Za-z_]\w*)([.:])([A-Za-z_]\w*)\s*\(([^)]*)\)(?:\s*:\s*([^-\n]+))?/.exec(trimmed);
      if (modFnMatch) {
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

      const localFnMatch = /^(local\s+)?function\s+([A-Za-z_]\w*)\s*\(([^)]*)\)(?:\s*:\s*([^-\n]+))?/.exec(trimmed);
      if (localFnMatch) {
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

      const assignFnMatch = /^([A-Za-z_]\w*)\.([A-Za-z_]\w*)\s*=\s*function\s*\(([^)]*)\)/.exec(trimmed);
      if (assignFnMatch) {
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
    const retTableMatch = /\breturn\s*\{([\s\S]*?)\}\s*(?:--.*)?$/m.exec(src);
    if (retTableMatch) {
      const entries = splitTopLevelCommas(retTableMatch[1]);
      for (const entry of entries) {
        const kv = /^([A-Za-z_]\w*)(?:\s*=\s*([A-Za-z_]\w*))?$/.exec(entry.trim());
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
              symbols.push({ name: expName, kind: "export", path, line: lines.length, signature: null });
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

  for (const [callerPath, src] of Object.entries(scripts)) {
    const lines = src.split("\n");
    const requireAliases = new Map<string, string>();
    const remoteAliases = new Map<string, string>();

    for (let i = 0; i < lines.length; i++) {
      const lineNo = i + 1;
      const trimmed = lines[i].trim();
      if (!trimmed || trimmed.startsWith("--")) continue;

      const reqMatch = /\blocal\s+([A-Za-z_]\w*)\s*=\s*require\((.+)\)\s*(?:--.*)?$/.exec(trimmed);
      if (reqMatch) {
        const alias = reqMatch[1];
        const target = resolveRequireTarget(reqMatch[2], callerPath, scriptPaths);
        if (target) requireAliases.set(alias, target);
      }

      const remBindMatch = /\blocal\s+([A-Za-z_]\w*)\s*=\s*[^-\n]*?(?:WaitForChild|FindFirstChild)\(\s*["']([^"']+)["']\s*\)/.exec(trimmed);
      if (remBindMatch && (remoteDefs.has(remBindMatch[2]) || /Remote|Event|Func|Buy|Shop|Roll/i.test(remBindMatch[2]))) {
        remoteAliases.set(remBindMatch[1], remBindMatch[2]);
      }

      // Check require() alias calls: Alias.Fn(args) or Alias:Fn(args)
      for (const [alias, targetPath] of requireAliases.entries()) {
        const callRe = new RegExp(`\\b${alias}([.:])([A-Za-z_]\\w*)\\s*\\(([^)]*)\\)`, "g");
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
        const fireServer = new RegExp(`\\b${alias}:(FireServer|InvokeServer)\\s*\\(([^)]*)\\)`).exec(trimmed);
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
        const fireClient = new RegExp(`\\b${alias}:FireClient\\s*\\(([^)]*)\\)`).exec(trimmed);
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
        const srvHandler = new RegExp(`\\b${alias}\\.OnServerEvent:Connect\\s*\\(\\s*function\\s*\\(([^)]*)\\)`).exec(trimmed);
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
        const srvInvoke = new RegExp(`\\b${alias}\\.OnServerInvoke\\s*=\\s*function\\s*\\(([^)]*)\\)`).exec(trimmed);
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
  const idx = Math.max(0, Math.min(lineText.length - 1, col1 - 1));
  let s = idx;
  let e = idx;
  while (s > 0 && /[A-Za-z0-9_]/.test(lineText[s - 1])) s--;
  while (e < lineText.length && /[A-Za-z0-9_]/.test(lineText[e])) e++;
  const word = lineText.slice(s, e);
  let qualifier: string | null = null;
  if (s >= 2 && (lineText[s - 1] === "." || lineText[s - 1] === ":")) {
    let qs = s - 1;
    while (qs > 0 && /[A-Za-z0-9_]/.test(lineText[qs - 1])) qs--;
    const q = lineText.slice(qs, s - 1);
    if (q) qualifier = q;
  }
  return { word, qualifier };
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
  const index = buildWorkspaceSymbolIndex(scripts, instances);
  const lines = src.split("\n");
  const lineText = lines[Math.max(0, Math.min(lines.length - 1, line - 1))] || "";
  const scriptPaths = Object.keys(scripts);

  // Check require(...) on the current line
  const reqOnLine = /require\((.+)\)/.exec(lineText);
  const { word, qualifier } = identAtColumn(lineText, col);
  if (reqOnLine && (word === "require" || reqOnLine[0].includes(word) || !word)) {
    const target = resolveRequireTarget(reqOnLine[1], path, scriptPaths);
    if (target) {
      const sym = index.symbols.find((s) => s.path === target) || null;
      return { ok: true, target_path: target, line: sym?.line ?? 1, col: 1, symbol: sym };
    }
  }

  // Check qualified call like Alias.Fn or Alias:Fn
  if (qualifier && word) {
    for (const ln of lines) {
      const reqMatch = new RegExp(`\\blocal\\s+${qualifier}\\s*=\\s*require\\((.+)\\)`).exec(ln);
      if (reqMatch) {
        const target = resolveRequireTarget(reqMatch[1], path, scriptPaths);
        if (target) {
          const sym = index.symbols.find((s) => s.path === target && s.name === word) || null;
          return { ok: true, target_path: target, line: sym?.line ?? 1, col: 1, symbol: sym };
        }
      }
    }
  }

  // Check remote alias or quoted remote name on line
  const quoted = /["']([^"']+)["']/.exec(lineText);
  if (quoted) {
    const rSym = index.symbols.find((s) => (s.kind === "remote_event" || s.kind === "remote_function") && s.name === quoted[1]);
    if (rSym) {
      return { ok: true, target_path: rSym.path, line: rSym.line, col: 1, symbol: rSym };
    }
  }

  if (word) {
    // Check if word is a require alias itself
    for (const ln of lines) {
      const reqMatch = new RegExp(`\\blocal\\s+${word}\\s*=\\s*require\\((.+)\\)`).exec(ln);
      if (reqMatch) {
        const target = resolveRequireTarget(reqMatch[1], path, scriptPaths);
        if (target) {
          const sym = index.symbols.find((s) => s.path === target) || null;
          return { ok: true, target_path: target, line: sym?.line ?? 1, col: 1, symbol: sym };
        }
      }
      const remMatch = new RegExp(`\\blocal\\s+${word}\\s*=\\s*[^-\\n]*?(?:WaitForChild|FindFirstChild)\\(\\s*["']([^"']+)["']\\s*\\)`).exec(ln);
      if (remMatch) {
        const rSym = index.symbols.find((s) => (s.kind === "remote_event" || s.kind === "remote_function") && s.name === remMatch[1]);
        if (rSym) {
          return { ok: true, target_path: rSym.path, line: rSym.line, col: 1, symbol: rSym };
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
