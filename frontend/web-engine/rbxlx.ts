// Genuine Roblox Studio XML (.rbxlx, <roblox version="4">) serializer for OpenRO's in-browser engine.
import type { PlaceInstance, PlaceState } from "./luau-skills";

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

function colorU32(className: string, name: string, props: Record<string, unknown>): number {
  if (typeof props.Color === "string") {
    const clean = props.Color.replace(/^#/, "");
    if (/^[0-9a-fA-F]{6}$/.test(clean)) return (0xff000000 | parseInt(clean, 16)) >>> 0;
  }
  const low = name.toLowerCase();
  if (low.includes("lava") || low.includes("kill") || low.includes("laser")) return 0xffe0532a;
  if (low.includes("ice") || low.includes("frost")) return 0xff67b7e8;
  if (low.includes("jump") || low.includes("bounce")) return 0xff2d9d78;
  if (low.includes("coin") || low.includes("gold")) return 0xffe5b42a;
  if (className === "SpawnLocation" || low.includes("spawn")) return 0xff3b6ce2;
  return 0xffa3a2a5;
}

export function serializeRbxlx(gameName: string, place: PlaceState): string {
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
    const isPart = ["Part", "SpawnLocation", "WedgePart", "TrussPart"].includes(cls);
    const [sx, sy, sz] = vec3(props.Size, cls === "SpawnLocation" ? [8, 1, 8] : [10, 1, 10]);
    const [px, py, pz] = vec3(props.Position, [0, 1, idx * 16]);
    const col = colorU32(cls, name, props);

    lines.push(
      `${indent}<Item class="${xmlEsc(cls)}" referent="${nextRef()}">`,
      `${indent}  <Properties>`,
      `${indent}    <string name="Name">${xmlEsc(name)}</string>`,
    );
    if (isPart) {
      lines.push(
        `${indent}    <bool name="Anchored">${anchored}</bool>`,
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
    const name = segs[segs.length - 1] || "Part";
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
