// Genuine Roblox Studio XML (.rbxlx, <roblox version="4">) bidirectional codec:
// BOTH Serializer (export) AND Parser (import) with full CFrame, Vector3 Size,
// Color3uint8, Anchored, Material, Transparency, CanCollide, and ProtectedString CDATA.
import type { Place, PlaceInstance } from "./data";

const MATERIAL_NAME_TO_ENUM: Record<string, number> = {
  Plastic: 256,
  SmoothPlastic: 272,
  Neon: 288,
  Wood: 512,
  Slate: 800,
  Concrete: 816,
  CorrodedMetal: 1040,
  DiamondPlate: 1056,
  Foil: 1072,
  Grass: 1280,
  Ice: 1536,
  Marble: 784,
  Granite: 832,
  Brick: 848,
  Pebble: 864,
  Sand: 1296,
  Fabric: 1312,
  Metal: 1088,
  Glass: 1568,
  ForceField: 1584,
};

const MATERIAL_ENUM_TO_NAME: Record<number, string> = Object.fromEntries(
  Object.entries(MATERIAL_NAME_TO_ENUM).map(([k, v]) => [v, k]),
);

function xmlEsc(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
}

function cdata(s: string): string {
  return `<![CDATA[${s.replace(/]]>/g, "]]]]><![CDATA[>")}]]>`;
}

function vec3(v: unknown, fallback: [number, number, number]): [number, number, number] {
  if (Array.isArray(v) && v.length >= 3 && v.every((n) => typeof n === "number" && Number.isFinite(n))) {
    return [v[0], v[1], v[2]];
  }
  return fallback;
}

export function hexToColor3uint8(hex: string): number | null {
  const clean = hex.trim().replace(/^#/, "");
  if (/^[0-9a-fA-F]{6}$/.test(clean)) {
    return (0xff000000 | parseInt(clean, 16)) >>> 0;
  }
  return null;
}

export function color3uint8ToHex(val: number): string {
  const rgb = (val >>> 0) & 0xffffff;
  return "#" + rgb.toString(16).toUpperCase().padStart(6, "0");
}

function defaultColorU32(className: string, name: string, props: Record<string, unknown>): number {
  if (typeof props.Color === "string") {
    const parsed = hexToColor3uint8(props.Color);
    if (parsed !== null) return parsed;
  }
  const low = name.toLowerCase();
  if (low.includes("lava") || low.includes("kill") || low.includes("laser") || low.includes("acid")) return 0xffe0532a;
  if (low.includes("ice") || low.includes("frost")) return 0xff67b7e8;
  if (low.includes("jump") || low.includes("bounce")) return 0xff2d9d78;
  if (low.includes("moving") || low.includes("platform")) return 0xff4b7fb3;
  if (low.includes("portal") || low.includes("teleport")) return 0xff7c52d4;
  if (low.includes("dropper") || low.includes("tycoon") || low.includes("coin") || low.includes("gold")) return 0xffe5b42a;
  if (low.includes("checkpoint") || low.includes("stage")) return 0xff2f8f5b;
  if (low.includes("npc") || low.includes("guard")) return 0xffc96b3b;
  if (low.includes("vehicle")) return 0xff4d6be0;
  if (className === "SpawnLocation" || low.includes("spawn")) return 0xff3b6ce2;
  return 0xffa3a2a5;
}

export function serializeRbxlx(gameName: string, place: Place): string {
  let refCounter = 1;
  const nextRef = () => `RBX${(refCounter++).toString(16).toUpperCase().padStart(8, "0")}`;

  const lines: string[] = [
    '<?xml version="1.0" encoding="utf-8"?>',
    '<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd" version="4">',
    '  <Meta name="ExplicitAutoJoints">true</Meta>',
    `  <Meta name="OpenROGameName">${xmlEsc(gameName)}</Meta>`,
    "  <External>null</External>",
    "  <External>nil</External>",
  ];

  const writeScript = (indent: string, cls: string, name: string, src: string) => {
    lines.push(
      `${indent}<Item class="${cls}" referent="${nextRef()}">`,
      `${indent}  <Properties>`,
      `${indent}    <string name="Name">${xmlEsc(name)}</string>`,
      `${indent}    <bool name="Disabled">false</bool>`,
      `${indent}    <ProtectedString name="Source">${cdata(src)}</ProtectedString>`,
      `${indent}  </Properties>`,
      `${indent}</Item>`,
    );
  };

  const writeInstance = (
    indent: string,
    cls: string,
    name: string,
    inst: PlaceInstance,
    idx: number,
    childScripts: Array<[string, string]>,
  ) => {
    const props = inst.properties || {};
    const anchored = props.Anchored !== false;
    const canCollide = props.CanCollide !== false;
    const transparency =
      typeof props.Transparency === "number" && Number.isFinite(props.Transparency)
        ? Math.max(0, Math.min(1, props.Transparency))
        : 0;
    const matToken =
      typeof props.Material === "string" && MATERIAL_NAME_TO_ENUM[props.Material]
        ? MATERIAL_NAME_TO_ENUM[props.Material]
        : typeof props.Material === "number"
          ? props.Material
          : 272;
    const isPart = ["Part", "SpawnLocation", "WedgePart", "TrussPart", "VehicleSeat", "Seat"].includes(cls);
    const [sx, sy, sz] = vec3(props.Size, cls === "SpawnLocation" ? [8, 1, 8] : [10, 1, 10]);
    const [px, py, pz] = vec3(props.Position, [0, 1, idx * 16]);
    const col = defaultColorU32(cls, name, props);

    lines.push(
      `${indent}<Item class="${xmlEsc(cls)}" referent="${nextRef()}">`,
      `${indent}  <Properties>`,
      `${indent}    <string name="Name">${xmlEsc(name)}</string>`,
    );
    if (isPart) {
      lines.push(
        `${indent}    <bool name="Anchored">${anchored}</bool>`,
        `${indent}    <bool name="CanCollide">${canCollide}</bool>`,
        `${indent}    <float name="Transparency">${transparency}</float>`,
        `${indent}    <token name="Material">${matToken}</token>`,
        `${indent}    <CoordinateFrame name="CFrame">`,
        `${indent}      <X>${px}</X><Y>${py}</Y><Z>${pz}</Z>`,
        `${indent}      <R00>1</R00><R01>0</R01><R02>0</R02>`,
        `${indent}      <R10>0</R10><R11>1</R11><R12>0</R12>`,
        `${indent}      <R20>0</R20><R21>0</R21><R22>1</R22>`,
        `${indent}    </CoordinateFrame>`,
        `${indent}    <Vector3 name="size"><X>${sx}</X><Y>${sy}</Y><Z>${sz}</Z></Vector3>`,
        `${indent}    <Color3uint8 name="Color3uint8">${col}</Color3uint8>`,
      );
      if (cls === "SpawnLocation") lines.push(`${indent}    <bool name="Neutral">true</bool>`);
    }
    lines.push(`${indent}  </Properties>`);
    if (typeof props.DecalTexture === "string" && props.DecalTexture.trim()) {
      const decalName =
        typeof props.CardTitle === "string" && props.CardTitle.trim()
          ? `${props.CardTitle.trim()}Art`
          : `${name}Decal`;
      lines.push(
        `${indent}  <Item class="Decal" referent="${nextRef()}">`,
        `${indent}    <Properties>`,
        `${indent}      <string name="Name">${xmlEsc(decalName)}</string>`,
        `${indent}      <Content name="Texture"><url>${xmlEsc(props.DecalTexture.trim())}</url></Content>`,
        `${indent}      <token name="Face">5</token>`,
        `${indent}    </Properties>`,
        `${indent}  </Item>`,
      );
    }
    for (const [scName, scSrc] of childScripts) {
      writeScript(`${indent}  `, "Script", scName, scSrc);
    }
    lines.push(`${indent}</Item>`);
  };

  const wsChildren = new Map<string, Array<[string, string]>>();
  const wsRoot: Array<[string, string]> = [];
  const serverScripts: Array<[string, string]> = [];
  const guiScripts: Array<[string, string]> = [];
  const playerScripts: Array<[string, string]> = [];
  const otherServices = new Map<string, Array<[string, string]>>();

  for (const [path, src] of Object.entries(place.scripts || {})) {
    const segs = path.split(".");
    if (segs[0] === "Workspace" && segs.length === 3) {
      const key = `Workspace.${segs[1]}`;
      const list = wsChildren.get(key) || [];
      list.push([segs[2], src]);
      wsChildren.set(key, list);
    } else if (segs[0] === "Workspace" && segs.length === 2) {
      wsRoot.push([segs[1], src]);
    } else if (segs[0] === "ServerScriptService" && segs.length >= 2) {
      serverScripts.push([segs.slice(1).join("."), src]);
    } else if (segs[0] === "StarterGui" && segs.length >= 2) {
      guiScripts.push([segs.slice(1).join("."), src]);
    } else if (segs[0] === "StarterPlayer" && segs.length >= 2) {
      const rest = segs[1] === "StarterPlayerScripts" ? segs.slice(2) : segs.slice(1);
      playerScripts.push([rest.join(".") || "ClientScript", src]);
    } else if (segs.length >= 2) {
      const list = otherServices.get(segs[0]) || [];
      list.push([segs.slice(1).join("."), src]);
      otherServices.set(segs[0], list);
    } else {
      serverScripts.push([path, src]);
    }
  }

  // Workspace
  lines.push(
    `  <Item class="Workspace" referent="${nextRef()}">`,
    '    <Properties><string name="Name">Workspace</string><float name="Gravity">196.2</float></Properties>',
  );
  const placedChildKeys = new Set<string>();
  let idx = 0;
  for (const [path, inst] of Object.entries(place.instances || {})) {
    const segs = path.split(".");
    if (segs[0] !== "Workspace") continue;
    const name = segs.slice(1).join(".") || "Part";
    const children = wsChildren.get(path) || [];
    if (children.length) placedChildKeys.add(path);
    writeInstance("    ", inst.class_name || "Part", name, inst, idx++, children);
  }
  for (const [parentPath, children] of wsChildren.entries()) {
    if (placedChildKeys.has(parentPath)) continue;
    const name = parentPath.split(".").pop() || "Part";
    writeInstance("    ", "Part", name, { class_name: "Part", properties: { Anchored: true } }, idx++, children);
  }
  for (const [name, src] of wsRoot) writeScript("    ", "Script", name, src);
  lines.push("  </Item>");

  // ServerScriptService
  lines.push(
    `  <Item class="ServerScriptService" referent="${nextRef()}">`,
    '    <Properties><string name="Name">ServerScriptService</string></Properties>',
  );
  for (const [name, src] of serverScripts) writeScript("    ", "Script", name, src);
  lines.push("  </Item>");

  // StarterGui
  lines.push(
    `  <Item class="StarterGui" referent="${nextRef()}">`,
    '    <Properties><string name="Name">StarterGui</string></Properties>',
  );
  for (const [name, src] of guiScripts) writeScript("    ", "LocalScript", name, src);
  lines.push("  </Item>");

  // StarterPlayer
  lines.push(
    `  <Item class="StarterPlayer" referent="${nextRef()}">`,
    '    <Properties><string name="Name">StarterPlayer</string></Properties>',
    `    <Item class="StarterPlayerScripts" referent="${nextRef()}">`,
    '      <Properties><string name="Name">StarterPlayerScripts</string></Properties>',
  );
  for (const [name, src] of playerScripts) writeScript("      ", "LocalScript", name, src);
  lines.push("    </Item>", "  </Item>");

  for (const svc of ["ReplicatedStorage", "Lighting", "SoundService"]) {
    lines.push(
      `  <Item class="${svc}" referent="${nextRef()}">`,
      `    <Properties><string name="Name">${svc}</string></Properties>`,
    );
    for (const [name, src] of otherServices.get(svc) || []) writeScript("    ", "Script", name, src);
    lines.push("  </Item>");
  }

  lines.push("</roblox>");
  return lines.join("\n") + "\n";
}

const SERVICE_CLASSES = new Set([
  "Workspace",
  "ServerScriptService",
  "ServerStorage",
  "StarterGui",
  "StarterPlayer",
  "ReplicatedStorage",
  "Lighting",
  "SoundService",
]);

const SCRIPT_CLASSES = new Set(["Script", "LocalScript", "ModuleScript"]);

function childByTag(el: Element, tag: string): Element | null {
  for (let i = 0; i < el.children.length; i++) {
    if (el.children[i].tagName === tag) return el.children[i];
  }
  return null;
}

function directChildrenByTag(el: Element, tag: string): Element[] {
  const out: Element[] = [];
  for (let i = 0; i < el.children.length; i++) {
    if (el.children[i].tagName === tag) out.push(el.children[i]);
  }
  return out;
}

function readNum(parent: Element, tag: string, fallback = 0): number {
  const child = childByTag(parent, tag);
  const n = child ? Number(child.textContent?.trim()) : NaN;
  return Number.isFinite(n) ? n : fallback;
}

export function parseRbxlxXml(xml: string): { ok: true; gameName: string; place: Place } | { ok: false; error: string } {
  const trimmed = xml.trim();
  if (!trimmed || !trimmed.includes("<roblox")) {
    return { ok: false, error: "File does not look like a Roblox Studio XML (.rbxlx) document." };
  }
  if (typeof DOMParser === "undefined") {
    return { ok: false, error: "Browser XML parser is unavailable." };
  }
  const doc = new DOMParser().parseFromString(trimmed, "application/xml");
  const parseErr = doc.querySelector("parsererror");
  if (parseErr) {
    return { ok: false, error: `Invalid XML: ${(parseErr.textContent || "malformed document").slice(0, 140)}` };
  }
  const root = doc.documentElement;
  if (!root || root.tagName !== "roblox") {
    return { ok: false, error: "Missing <roblox> root element." };
  }

  let gameName = "Imported Place";
  for (const meta of directChildrenByTag(root, "Meta")) {
    if (meta.getAttribute("name") === "OpenROGameName" && meta.textContent?.trim()) {
      gameName = meta.textContent.trim();
    }
  }

  const place: Place = { scripts: {}, instances: {} };

  const walkItem = (el: Element, parentPath: string) => {
    const cls = el.getAttribute("class") || "Instance";
    const propsEl = childByTag(el, "Properties");
    let itemName = cls;
    let scriptSource: string | null = null;
    const props: Record<string, unknown> = {};

    if (propsEl) {
      for (let i = 0; i < propsEl.children.length; i++) {
        const p = propsEl.children[i];
        const pName = p.getAttribute("name") || "";
        const tag = p.tagName;
        if (pName === "Name" && p.textContent?.trim()) {
          itemName = p.textContent.trim();
        } else if (tag === "ProtectedString" && pName === "Source") {
          scriptSource = p.textContent ?? "";
        } else if (tag === "bool") {
          props[pName] = p.textContent?.trim().toLowerCase() === "true";
        } else if (tag === "float" || tag === "double" || tag === "int") {
          const v = Number(p.textContent?.trim());
          if (Number.isFinite(v)) props[pName] = v;
        } else if (tag === "token" && pName === "Material") {
          const code = Number(p.textContent?.trim());
          props.Material = MATERIAL_ENUM_TO_NAME[code] ?? code;
        } else if (tag === "Color3uint8") {
          const code = Number(p.textContent?.trim());
          if (Number.isFinite(code)) props.Color = color3uint8ToHex(code);
        } else if (tag === "CoordinateFrame" && pName === "CFrame") {
          props.Position = [readNum(p, "X"), readNum(p, "Y"), readNum(p, "Z")];
        } else if (tag === "Vector3" && (pName === "size" || pName === "Size")) {
          props.Size = [readNum(p, "X", 4), readNum(p, "Y", 1), readNum(p, "Z", 4)];
        } else if (tag === "Vector3" && pName === "Position") {
          props.Position = [readNum(p, "X"), readNum(p, "Y"), readNum(p, "Z")];
        } else if (tag === "Content" && (pName === "Texture" || pName === "Image")) {
          const urlChild = childByTag(p, "url");
          const rawUrl = (urlChild?.textContent ?? p.textContent ?? "").trim();
          if (rawUrl) props[pName] = rawUrl;
        }
      }
    }

    const isServiceRoot = !parentPath && SERVICE_CLASSES.has(cls);
    const fullPath = isServiceRoot ? cls : parentPath ? `${parentPath}.${itemName}` : itemName;

    if (SCRIPT_CLASSES.has(cls)) {
      place.scripts[fullPath] = scriptSource ?? "";
    } else if (!isServiceRoot && cls !== "StarterPlayerScripts" && cls !== "StarterCharacterScripts") {
      place.instances[fullPath] = { class_name: cls, properties: props };
    }

    for (const child of directChildrenByTag(el, "Item")) {
      walkItem(child, fullPath);
    }
  };

  for (const top of directChildrenByTag(root, "Item")) {
    walkItem(top, "");
  }

  return { ok: true, gameName, place };
}
