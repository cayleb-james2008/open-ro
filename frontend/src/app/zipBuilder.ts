// Pure zero-dependency PKZIP 2.0 (STORE method 0, exact IEEE 802.3 CRC-32) archive writer
// & Roblox Studio + Rojo v7 Ship Pack (.zip) builder.
// Produces genuine standard .zip archives verified readable by Python zipfile and unzip -t.
import type { Place, ProjectAssetRecord } from "./data";
import type { LuauDiagnostic } from "./luauAudit";
import { serializeRbxlx } from "./rbxlxCodec";

export type ZipInputEntry = {
  name: string;
  data: Uint8Array | string;
};

const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let i = 0; i < 256; i++) {
    let c = i;
    for (let k = 0; k < 8; k++) {
      c = (c & 1) !== 0 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
    table[i] = c >>> 0;
  }
  return table;
})();

export function crc32(bytes: Uint8Array): number {
  let c = 0xffffffff;
  for (let i = 0; i < bytes.length; i++) {
    c = CRC_TABLE[(c ^ bytes[i]) & 0xff] ^ (c >>> 8);
  }
  return (c ^ 0xffffffff) >>> 0;
}

function dosTimeDate(now = new Date()): { time: number; date: number } {
  const year = Math.max(1980, now.getFullYear());
  const date = ((year - 1980) << 9) | ((now.getMonth() + 1) << 5) | now.getDate();
  const time = (now.getHours() << 11) | (now.getMinutes() << 5) | (now.getSeconds() >> 1);
  return { time, date };
}

export function buildZipArchive(entries: ZipInputEntry[]): Uint8Array {
  const enc = new TextEncoder();
  const { time, date } = dosTimeDate();

  const prepared = entries.map((e) => {
    const cleanName = e.name.replace(/^\/+/, "").replace(/\\/g, "/");
    const nameBytes = enc.encode(cleanName);
    const dataBytes = typeof e.data === "string" ? enc.encode(e.data) : e.data;
    return {
      nameBytes,
      dataBytes,
      crc: crc32(dataBytes),
      offset: 0,
    };
  });

  let localTotal = 0;
  let centralTotal = 0;
  for (const item of prepared) {
    item.offset = localTotal;
    localTotal += 30 + item.nameBytes.length + item.dataBytes.length;
    centralTotal += 46 + item.nameBytes.length;
  }

  const out = new Uint8Array(localTotal + centralTotal + 22);
  const view = new DataView(out.buffer);
  let ptr = 0;

  // 1. Local file headers + stored payloads
  for (const item of prepared) {
    view.setUint32(ptr, 0x04034b50, true); // Local file header signature
    view.setUint16(ptr + 4, 20, true); // Version needed (2.0)
    view.setUint16(ptr + 6, 0x0800, true); // UTF-8 filename flag
    view.setUint16(ptr + 8, 0, true); // Compression method: 0 (STORE)
    view.setUint16(ptr + 10, time, true);
    view.setUint16(ptr + 12, date, true);
    view.setUint32(ptr + 14, item.crc, true);
    view.setUint32(ptr + 18, item.dataBytes.length, true);
    view.setUint32(ptr + 22, item.dataBytes.length, true);
    view.setUint16(ptr + 26, item.nameBytes.length, true);
    view.setUint16(ptr + 28, 0, true); // Extra field length
    out.set(item.nameBytes, ptr + 30);
    out.set(item.dataBytes, ptr + 30 + item.nameBytes.length);
    ptr += 30 + item.nameBytes.length + item.dataBytes.length;
  }

  const centralStart = ptr;

  // 2. Central directory records
  for (const item of prepared) {
    view.setUint32(ptr, 0x02014b50, true); // Central directory signature
    view.setUint16(ptr + 4, 20, true); // Version made by
    view.setUint16(ptr + 6, 20, true); // Version needed
    view.setUint16(ptr + 8, 0x0800, true); // UTF-8 flag
    view.setUint16(ptr + 10, 0, true); // Method: STORE
    view.setUint16(ptr + 12, time, true);
    view.setUint16(ptr + 14, date, true);
    view.setUint32(ptr + 16, item.crc, true);
    view.setUint32(ptr + 20, item.dataBytes.length, true);
    view.setUint32(ptr + 24, item.dataBytes.length, true);
    view.setUint16(ptr + 28, item.nameBytes.length, true);
    view.setUint16(ptr + 30, 0, true); // Extra len
    view.setUint16(ptr + 32, 0, true); // Comment len
    view.setUint16(ptr + 34, 0, true); // Disk number
    view.setUint16(ptr + 36, 0, true); // Internal attrs
    view.setUint32(ptr + 38, 0, true); // External attrs
    view.setUint32(ptr + 42, item.offset, true); // Local header offset
    out.set(item.nameBytes, ptr + 46);
    ptr += 46 + item.nameBytes.length;
  }

  // 3. End of Central Directory Record (EOCD)
  view.setUint32(ptr, 0x06054b50, true);
  view.setUint16(ptr + 4, 0, true);
  view.setUint16(ptr + 6, 0, true);
  view.setUint16(ptr + 8, prepared.length, true);
  view.setUint16(ptr + 10, prepared.length, true);
  view.setUint32(ptr + 12, centralTotal, true);
  view.setUint32(ptr + 16, centralStart, true);
  view.setUint16(ptr + 20, 0, true);

  return out;
}

function decodeDataUrlBytes(dataUrl: string): Uint8Array | null {
  const idx = dataUrl.indexOf("base64,");
  if (idx < 0) return null;
  try {
    const raw = atob(dataUrl.slice(idx + 7));
    const bytes = new Uint8Array(raw.length);
    for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
    return bytes;
  } catch {
    return null;
  }
}

function sanitizeFileSegment(raw: string): string {
  return raw.replace(/[^a-zA-Z0-9._-]+/g, "_").replace(/^_+|_+$/g, "") || "Script";
}

export function buildRojoShipPackZip(opts: {
  gameName: string;
  place: Place;
  assets: ProjectAssetRecord[];
  diagnostics: LuauDiagnostic[];
}): Uint8Array {
  const safeName = (opts.gameName || "OpenRO Place").trim() || "OpenRO Place";
  const entries: ZipInputEntry[] = [];

  // 1. game.rbxlx (full Roblox Studio XML)
  const xml = serializeRbxlx(safeName, opts.place);
  entries.push({ name: "game.rbxlx", data: xml });

  // 2. Rojo v7 default.project.json
  const rojoProject = {
    name: safeName,
    tree: {
      $className: "DataModel",
      ServerScriptService: {
        $className: "ServerScriptService",
        $path: "src/server",
      },
      StarterPlayer: {
        $className: "StarterPlayer",
        StarterPlayerScripts: {
          $className: "StarterPlayerScripts",
          $path: "src/client",
        },
      },
      ReplicatedStorage: {
        $className: "ReplicatedStorage",
        Shared: {
          $className: "Folder",
          $path: "src/shared",
        },
      },
    },
  };
  entries.push({ name: "default.project.json", data: JSON.stringify(rojoProject, null, 2) + "\n" });

  // 3. src/{server,client,shared}/*.luau
  const usedPaths = new Set<string>();
  const addUnique = (name: string, data: string | Uint8Array) => {
    if (!usedPaths.has(name)) {
      usedPaths.add(name);
      entries.push({ name, data });
    }
  };

  let serverCount = 0;
  let clientCount = 0;
  let sharedCount = 0;

  for (const [scriptPath, src] of Object.entries(opts.place.scripts || {})) {
    const segs = scriptPath.split(".");
    const root = segs[0] || "ServerScriptService";
    const leaf = sanitizeFileSegment(segs.slice(1).join("_") || scriptPath);
    if (root === "StarterGui" || root === "StarterPlayer") {
      addUnique(`src/client/${leaf}.client.luau`, src);
      clientCount++;
    } else if (root === "ReplicatedStorage") {
      addUnique(`src/shared/${leaf}.luau`, src);
      sharedCount++;
    } else {
      addUnique(`src/server/${leaf}.server.luau`, src);
      serverCount++;
    }
  }

  if (serverCount === 0) {
    addUnique("src/server/init.server.luau", "-- OpenRO server entrypoint\nprint('OpenRO server initialized')\n");
  }
  if (clientCount === 0) {
    addUnique("src/client/init.client.luau", "-- OpenRO client entrypoint\nprint('OpenRO client ready')\n");
  }
  if (sharedCount === 0) {
    addUnique("src/shared/Config.luau", "return { GameName = " + JSON.stringify(safeName) + " }\n");
  }

  // 4. asset-manifest.json + binary assets/*.webp|png
  const manifestAssets: Array<Record<string, unknown>> = [];
  for (const a of opts.assets) {
    const fileName = sanitizeFileSegment(a.file || `${a.id}.webp`);
    let bytesIncluded = 0;
    if (a.image_url && a.image_url.startsWith("data:image/")) {
      const bin = decodeDataUrlBytes(a.image_url);
      if (bin && bin.length > 0) {
        addUnique(`assets/${fileName}`, bin);
        bytesIncluded = bin.length;
      }
    }
    manifestAssets.push({
      id: a.id,
      title: a.title,
      file: `assets/${fileName}`,
      bytes: bytesIncluded,
      source: a.source,
    });
  }
  entries.push({
    name: "asset-manifest.json",
    data: JSON.stringify({ game: safeName, version: "0.9.0", count: manifestAssets.length, assets: manifestAssets }, null, 2) + "\n",
  });

  // 5. luau-audit-report.json
  const errors = opts.diagnostics.filter((d) => d.severity === "error").length;
  const warnings = opts.diagnostics.filter((d) => d.severity === "warning").length;
  entries.push({
    name: "luau-audit-report.json",
    data:
      JSON.stringify(
        {
          game: safeName,
          generated_at: new Date().toISOString(),
          clean: errors === 0 && warnings === 0,
          errors,
          warnings,
          parts_count: Object.keys(opts.place.instances || {}).length,
          scripts_count: Object.keys(opts.place.scripts || {}).length,
          diagnostics: opts.diagnostics,
        },
        null,
        2,
      ) + "\n",
  });

  // 6. STUDIO-UPLOAD-CHECKLIST.md
  const checklist = [
    `# ${safeName} — Roblox Studio & Rojo Ship Pack`,
    "",
    "Generated by OpenRO v0.9.0.",
    "",
    "## Contents",
    `- \`game.rbxlx\` — Ready-to-open Roblox Studio XML place (${Object.keys(opts.place.instances || {}).length} parts, ${Object.keys(opts.place.scripts || {}).length} Luau scripts).`,
    "- `default.project.json` + `src/{server,client,shared}/*.luau` — Rojo v7 project layout for live Studio sync.",
    `- \`asset-manifest.json\` + \`assets/\` — ${manifestAssets.length} project asset record(s).`,
    `- \`luau-audit-report.json\` — Static Luau verification report (${errors} error(s), ${warnings} warning(s)).`,
    "",
    "## Quick Publish Checklist",
    "1. Double-click `game.rbxlx` in Roblox Studio (or run `rojo serve default.project.json`).",
    "2. Upload any images in `assets/` via Roblox Studio Asset Manager to obtain permanent `rbxassetid://` IDs and assign them to your `Decal` / `ImageLabel` textures.",
    "3. Enable **Allow Studio Access to API Services** under *Game Settings -> Security* if your experience uses `DataStoreService`.",
    "4. Press **Play (F5)** in Roblox Studio to verify client/server logs, then click **File -> Publish to Roblox**.",
    "",
  ].join("\n");
  entries.push({ name: "STUDIO-UPLOAD-CHECKLIST.md", data: checklist });

  return buildZipArchive(entries);
}
