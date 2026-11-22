// Real Luau static syntax, Roblox service-root, and cross-script runtime wiring validator
// for OpenRO's in-browser Roblox Studio engine (mirrors src/place.rs).
import type { PlaceState } from "./luau-skills";

const VALID_SERVICES = new Set([
  "Workspace",
  "ServerScriptService",
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
  "Name",
  "Parent",
]);

export function checkLuauSyntax(source: string): string | null {
  if (!source.trim()) return "1: empty Luau script";
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
        if (!closed) return `${startLine}: unclosed block comment --[[`;
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
        if (source[i] === "\n") return `${startLine}: unclosed string literal`;
        if (source[i] === q) {
          i++;
          closed = true;
          break;
        }
        i++;
      }
      if (!closed) return `${startLine}: unclosed string literal`;
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
      if (!closed) return `${startLine}: unclosed long string [[`;
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
      if (!top) return `${line}: unexpected closing delimiter '${c}'`;
      if (top[0] !== want) return `${line}: mismatched delimiter '${c}' (expected closing for '${top[0]}' from line ${top[1]})`;
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
    return `${l}: unclosed delimiter '${ch}'`;
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
      if (!top || top[0] !== "repeat") return `${tokLine}: unexpected 'until'`;
    } else if (word === "end") {
      const top = blocks.pop();
      if (!top) return `${tokLine}: unexpected 'end' with no matching block`;
      if (top[0] === "repeat") return `${tokLine}: expected 'until' for 'repeat' from line ${top[1]}`;
    }
  }
  if (blocks.length) {
    const [kind, l] = blocks[blocks.length - 1];
    return `${l}: unclosed '${kind}' block (missing 'end')`;
  }
  return null;
}

export function validatePlace(place: PlaceState): { passed: boolean; console: string; errors: string[] } {
  const errors: string[] = [];
  const allSources = Object.values(place.scripts);
  const createsName = (name: string) =>
    allSources.some((s) => s.includes(`Name = "${name}"`) || s.includes(`Name = '${name}'`));
  const createsRemote = allSources.some(
    (s) => s.includes('Instance.new("RemoteEvent")') || s.includes("Instance.new('RemoteEvent')"),
  );

  for (const [path, source] of Object.entries(place.scripts)) {
    const root = path.split(".")[0] || "";
    if (!VALID_SERVICES.has(root)) {
      errors.push(`${path}:1: invalid Roblox service root '${root}'`);
    }
    const syn = checkLuauSyntax(source);
    if (syn) errors.push(`${path}:${syn}`);

    const clean = source.replace(/--[^\n]*/g, "").replace(/"[^"\n]*"|'[^'\n]*'/g, " ");
    for (const m of clean.matchAll(/\bworkspace\.([A-Za-z_][A-Za-z0-9_]*)/g)) {
      const member = m[1];
      if (WORKSPACE_BUILTINS.has(member)) continue;
      if (!place.instances[`Workspace.${member}`] && !createsName(member)) {
        errors.push(`Workspace.${member} is not a valid member of Workspace (referenced in ${path})`);
      }
    }

    if (source.includes("GetDataStore(") && !source.includes("BindToClose")) {
      errors.push(`${path}: DataStore save failed: BindToClose was never bound`);
    }
    if (source.includes('WaitForChild("OpenROShopBuy")') && !createsRemote) {
      errors.push(`${path}: OpenROShopBuy RemoteEvent was never created in ReplicatedStorage`);
    }
    if (
      (source.includes('Instance.new("RemoteEvent")') || source.includes("Instance.new('RemoteEvent')")) &&
      source.includes("leaderstats") &&
      !createsName("leaderstats")
    ) {
      errors.push(`${path}: no leaderstats exists for the shop to charge`);
    }
  }

  for (const path of Object.keys(place.instances)) {
    const root = path.split(".")[0] || "";
    if (!VALID_SERVICES.has(root)) errors.push(`${path}: invalid Roblox service root '${root}'`);
  }

  if (errors.length === 0) {
    return {
      passed: true,
      errors: [],
      console: `[OpenRO Playtest] Verified ${Object.keys(place.scripts).length} Luau script(s) and ${Object.keys(place.instances).length} Workspace instance(s) — syntax OK, Roblox service roots OK, instance & RemoteEvent wiring OK (0 errors).`,
    };
  }
  return { passed: false, errors, console: errors.join("\n") };
}
