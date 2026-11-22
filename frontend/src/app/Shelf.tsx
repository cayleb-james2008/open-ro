import { useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import {
  AlertTriangle,
  Archive,
  Box,
  Check,
  ChevronDown,
  Copy,
  CopyPlus,
  Download,
  FileCode,
  FileDiff,
  FolderArchive,
  FolderTree,
  History,
  Layers,
  Monitor,
  PanelRightClose,
  PanelRightOpen,
  Pencil,
  Play,
  Save,
  Search,
  ShieldCheck,
  Trash2,
  Upload,
  User,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { Expandable, ExpandableContent, ExpandableTrigger } from "@/components/ui/expandable";
import { SlidingNumber } from "@/components/ui/sliding-number";
import { Pill, PillIndicator, PillStatus } from "@/components/kibo-ui/pill";
import type {
  InstanceEditPayload,
  Place,
  PlaceDiff,
  PlaceInstance,
  WorkspaceMode,
} from "./data";
import { analyzePlaceLuau, tokenizeLuauLine, type LuauDiagnostic, type LuauTokenKind } from "./luauAudit";

type KindInfo = { label: string; service: string; Icon: typeof FileCode; cls: string };

const KINDS: Record<string, KindInfo> = {
  ServerScriptService: { label: "server script", service: "ServerScriptService", Icon: FileCode, cls: "bg-tag-sky-bg text-tag-sky-fg" },
  ServerStorage: { label: "stored script", service: "ServerStorage", Icon: Archive, cls: "bg-tag-neutral-bg text-tag-neutral-fg" },
  StarterGui: { label: "screen GUI", service: "StarterGui", Icon: Monitor, cls: "bg-tag-design-bg text-tag-design-fg" },
  StarterPlayer: { label: "player script", service: "StarterPlayer", Icon: User, cls: "bg-tag-research-bg text-tag-research-fg" },
  ReplicatedStorage: { label: "shared module", service: "ReplicatedStorage", Icon: Layers, cls: "bg-tag-ops-bg text-tag-ops-fg" },
  Lighting: { label: "lighting", service: "Lighting", Icon: Layers, cls: "bg-tag-dev-bg text-tag-dev-fg" },
  SoundService: { label: "audio", service: "SoundService", Icon: Layers, cls: "bg-tag-research-bg text-tag-research-fg" },
  Workspace: { label: "in the world", service: "Workspace", Icon: Box, cls: "bg-tag-mint-bg text-tag-mint-fg" },
};

const FALLBACK_KIND: KindInfo = { label: "instance", service: "Workspace", Icon: Box, cls: "bg-board text-ink-2" };
const PART_SCRIPT_KIND: KindInfo = { label: "part script", service: "Workspace", Icon: FileCode, cls: "bg-tag-sky-bg text-tag-sky-fg" };

const TOKEN_CLS: Record<LuauTokenKind, string> = {
  comment: "text-ink-3",
  string: "text-tag-mint-fg",
  keyword: "font-medium text-tag-research-fg",
  global: "text-tag-ops-fg",
  number: "text-tag-plan-fg",
  plain: "text-ink",
};

export type ExplorerEntry = {
  path: string;
  name: string;
  kind: KindInfo;
  rawCopy: string;
  copyLabel: string;
  metaChip?: string;
  isScript: boolean;
  source?: string;
  instance?: PlaceInstance;
  diagnostics: LuauDiagnostic[];
};

function parsePath(path: string, isScript = false): { name: string; kind: KindInfo } {
  const segs = path.split(".");
  const rawLast = segs[segs.length - 1] || path;
  const spaced = rawLast.replace(/([a-z])([A-Z])/g, "$1 $2");
  if (isScript && segs[0] === "Workspace") return { name: spaced, kind: PART_SCRIPT_KIND };
  return { name: spaced, kind: KINDS[segs[0]] ?? FALLBACK_KIND };
}

function readVec3(val: unknown, fallback: [number, number, number]): [number, number, number] {
  if (Array.isArray(val) && val.length >= 3 && val.every((n) => typeof n === "number" && Number.isFinite(n))) {
    return [val[0], val[1], val[2]];
  }
  return fallback;
}

function slugName(gameName: string): string {
  const slug = (gameName || "openro-place")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return `${slug || "openro-place"}.rbxlx`;
}

export function buildGameSummary(gameName: string, builtTitles: string[], place: Place | null | "error"): string {
  const scripts = place && place !== "error" ? Object.keys(place.scripts || {}) : [];
  const parts = place && place !== "error" ? Object.keys(place.instances || {}) : [];
  const total = scripts.length + parts.length;
  if (!builtTitles.length && total === 0) return "";
  const titlePart = builtTitles.length ? builtTitles.join(", ") : (gameName || "Custom Studio place");
  return `My Roblox game has ${titlePart}, ${total} ${total === 1 ? "piece" : "pieces"} in all (${parts.length} Workspace ${parts.length === 1 ? "part" : "parts"}, ${scripts.length} Luau ${scripts.length === 1 ? "script" : "scripts"}), built and playtested with OpenRO.`;
}

function LuauCodeView({ source }: { source: string }) {
  const lines = useMemo(() => (source ? source.replace(/\r\n/g, "\n").split("\n") : ["-- empty"]), [source]);
  return (
    <div
      className="max-h-60 overflow-auto rounded-sm border border-hairline bg-surface p-2 font-mono text-xs leading-relaxed"
      aria-label="Syntax-highlighted Luau source"
    >
      <table className="w-full border-collapse">
        <tbody>
          {lines.map((line, i) => (
            <tr key={i}>
              <td className="select-none pe-2.5 text-right align-top font-mono text-xs tabular-nums text-ink-3">
                {i + 1}
              </td>
              <td className="whitespace-pre-wrap break-all align-top text-ink">
                {line.length === 0 ? (
                  " "
                ) : (
                  tokenizeLuauLine(line).map((tok, j) => (
                    <span key={j} className={TOKEN_CLS[tok.kind]}>
                      {tok.text}
                    </span>
                  ))
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function LuauScriptEditor({
  path,
  source,
  placeInstances,
  onSaveScript,
  onRunPlaytest,
}: {
  path: string;
  source: string;
  placeInstances?: Record<string, PlaceInstance>;
  onSaveScript?: (path: string, source: string) => Promise<void> | void;
  onRunPlaytest?: () => Promise<void> | void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(source);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    if (!editing) setDraft(source);
  }, [source, editing]);

  const liveDiags = useMemo(() => {
    if (!editing) return [] as LuauDiagnostic[];
    return analyzePlaceLuau({ scripts: { [path]: draft }, instances: placeInstances || {} });
  }, [draft, editing, path, placeInstances]);

  const liveErrs = liveDiags.filter((d) => d.severity === "error").length;
  const liveWarns = liveDiags.filter((d) => d.severity === "warning").length;
  const lineCount = Math.max(1, draft.replace(/\r\n/g, "\n").split("\n").length);
  const dirty = draft !== source;

  const handleSave = async () => {
    if (!onSaveScript || saving) return;
    setSaving(true);
    try {
      await onSaveScript(path, draft);
    } finally {
      setSaving(false);
    }
  };

  const handlePlaytest = async () => {
    if (!onRunPlaytest || testing) return;
    setTesting(true);
    try {
      if (dirty && onSaveScript) {
        await onSaveScript(path, draft);
      }
      await onRunPlaytest();
    } finally {
      setTesting(false);
    }
  };

  if (!editing) {
    return (
      <div className="flex flex-col gap-1.5">
        {onSaveScript && (
          <div className="flex items-center justify-between gap-2">
            <span className="font-mono text-xs text-ink-3">{lineCount} lines</span>
            <Button
              variant="outline"
              size="xs"
              aria-label={`Edit script ${path}`}
              onClick={() => {
                setDraft(source);
                setEditing(true);
              }}
              className="bg-surface max-md:min-h-11"
            >
              <Pencil className="size-3" strokeWidth={1.5} aria-hidden />
              Edit script
            </Button>
          </div>
        )}
        <LuauCodeView source={source || `-- ${path}`} />
      </div>
    );
  }

  return (
    <div data-luau-editor={path} className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center justify-between gap-1.5">
        <Pill
          data-luau-live-badge
          className={`px-2 py-0.5 font-mono text-xs ${
            liveErrs > 0
              ? "bg-pill-pro-bg text-pill-pro-fg"
              : liveWarns > 0
                ? "bg-pill-warn-bg text-pill-warn-fg"
                : "bg-pill-active-bg text-pill-active-fg"
          }`}
        >
          {liveErrs > 0 ? `${liveErrs} err` : liveWarns > 0 ? `${liveWarns} warn` : "Clean"}
        </Pill>
        <div className="flex flex-wrap items-center gap-1">
          <Button
            variant="default"
            size="xs"
            disabled={saving || !onSaveScript}
            aria-label={`Save and check ${path}`}
            onClick={() => void handleSave()}
            className="max-md:min-h-11"
          >
            <Save className="size-3" strokeWidth={1.5} aria-hidden />
            {saving ? "Saving…" : "Save & Check"}
          </Button>
          {onRunPlaytest && (
            <Button
              variant="outline"
              size="xs"
              disabled={testing}
              aria-label={`Playtest ${path}`}
              onClick={() => void handlePlaytest()}
              className="bg-surface max-md:min-h-11"
            >
              <Play className="size-3" strokeWidth={1.5} aria-hidden />
              {testing ? "Testing…" : "Playtest"}
            </Button>
          )}
          <Button
            variant="ghost"
            size="xs"
            aria-label={`Close script editor for ${path}`}
            onClick={() => setEditing(false)}
            className="max-md:min-h-11"
          >
            Done
          </Button>
        </div>
      </div>

      <div className="flex max-h-64 items-start overflow-y-auto rounded-sm border border-hairline bg-surface">
        <div
          aria-hidden
          className="shrink-0 select-none border-r border-hairline bg-board/50 px-2 py-2 text-right font-mono text-xs leading-5 tabular-nums text-ink-3"
        >
          {Array.from({ length: lineCount }, (_, idx) => (
            <div key={idx} className="h-5 leading-5">
              {idx + 1}
            </div>
          ))}
        </div>
        <Textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          aria-label={`Edit Luau source for ${path}`}
          spellCheck={false}
          wrap="off"
          rows={Math.max(6, lineCount)}
          style={{ height: `${Math.max(6, lineCount) * 20 + 16}px` }}
          className="min-w-0 flex-1 rounded-none border-0 bg-transparent shadow-none before:hidden [&_textarea]:!field-sizing-fixed [&_textarea]:!min-h-0 [&_textarea]:resize-none [&_textarea]:overflow-x-auto [&_textarea]:overflow-y-hidden [&_textarea]:whitespace-pre [&_textarea]:!p-2 [&_textarea]:font-mono [&_textarea]:text-xs [&_textarea]:leading-5"
        />
      </div>

      {liveDiags.length > 0 && (
        <ul data-luau-live-diags className="flex flex-col gap-1 rounded-sm border border-hairline bg-surface p-2 text-xs">
          {liveDiags.map((d, idx) => (
            <li
              key={idx}
              className={`flex items-start gap-1.5 ${
                d.severity === "error" ? "text-pill-pro-fg" : "text-pill-warn-fg"
              }`}
            >
              <AlertTriangle className="mt-0.5 size-3 flex-none" strokeWidth={1.5} aria-hidden />
              <span className="min-w-0 [overflow-wrap:anywhere]">
                <strong className="font-mono font-medium">L{d.line} [{d.code}]</strong> {d.message}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function InstancePropertyEditor({
  path,
  instance,
  onSave,
  onDuplicate,
  onDelete,
}: {
  path: string;
  instance: PlaceInstance;
  onSave?: (payload: InstanceEditPayload) => Promise<void>;
  onDuplicate?: (path: string) => Promise<void> | void;
  onDelete?: (path: string) => Promise<void> | void;
}) {
  const props = instance.properties || {};
  const initPos = readVec3(props.Position, [0, 1, 0]);
  const initSize = readVec3(props.Size, instance.class_name === "SpawnLocation" ? [8, 1, 8] : [10, 1, 10]);
  const initColor = typeof props.Color === "string" && props.Color.trim() ? props.Color.trim() : "#A3A2A5";
  const initAnchored = props.Anchored !== false;

  const [pos, setPos] = useState<[string, string, string]>([String(initPos[0]), String(initPos[1]), String(initPos[2])]);
  const [size, setSize] = useState<[string, string, string]>([String(initSize[0]), String(initSize[1]), String(initSize[2])]);
  const [color, setColor] = useState(initColor);
  const [anchored, setAnchored] = useState(initAnchored);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    const p = readVec3(instance.properties?.Position, [0, 1, 0]);
    const s = readVec3(instance.properties?.Size, instance.class_name === "SpawnLocation" ? [8, 1, 8] : [10, 1, 10]);
    setPos([String(p[0]), String(p[1]), String(p[2])]);
    setSize([String(s[0]), String(s[1]), String(s[2])]);
    setColor(typeof instance.properties?.Color === "string" ? instance.properties.Color : "#A3A2A5");
    setAnchored(instance.properties?.Anchored !== false);
  }, [instance]);

  const handleSave = async () => {
    if (!onSave || saving) return;
    setSaving(true);
    try {
      const parseTuple = (t: [string, string, string], fb: [number, number, number]): [number, number, number] => [
        Number.isFinite(Number(t[0])) ? Number(t[0]) : fb[0],
        Number.isFinite(Number(t[1])) ? Number(t[1]) : fb[1],
        Number.isFinite(Number(t[2])) ? Number(t[2]) : fb[2],
      ];
      await onSave({
        path,
        class_name: instance.class_name || "Part",
        properties: {
          ...(instance.properties || {}),
          Position: parseTuple(pos, initPos),
          Size: parseTuple(size, initSize),
          Color: /^#[0-9a-fA-F]{6}$/.test(color.trim()) ? color.trim().toUpperCase() : initColor,
          Anchored: anchored,
        },
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex flex-col gap-2 rounded-sm border border-hairline bg-surface p-2.5 text-xs">
      <div className="flex flex-col gap-1">
        <span className="font-medium text-ink-2">Position (X, Y, Z)</span>
        <div className="grid grid-cols-3 gap-1.5">
          {(["X", "Y", "Z"] as const).map((axis, idx) => (
            <Input
              key={axis}
              value={pos[idx]}
              onChange={(e) => {
                const next: [string, string, string] = [...pos];
                next[idx] = e.target.value;
                setPos(next);
              }}
              aria-label={`${path} Position ${axis}`}
              className="h-7 font-mono text-xs tabular-nums"
            />
          ))}
        </div>
      </div>

      <div className="flex flex-col gap-1">
        <span className="font-medium text-ink-2">Size (X, Y, Z)</span>
        <div className="grid grid-cols-3 gap-1.5">
          {(["X", "Y", "Z"] as const).map((axis, idx) => (
            <Input
              key={axis}
              value={size[idx]}
              onChange={(e) => {
                const next: [string, string, string] = [...size];
                next[idx] = e.target.value;
                setSize(next);
              }}
              aria-label={`${path} Size ${axis}`}
              className="h-7 font-mono text-xs tabular-nums"
            />
          ))}
        </div>
      </div>

      <div className="flex flex-wrap items-center justify-between gap-2">
        <label className="flex min-w-0 flex-1 items-center gap-1.5 font-medium text-ink-2">
          <span>Color</span>
          <span
            className="size-4 flex-none rounded-sm border border-hairline"
            style={{ backgroundColor: /^#[0-9a-fA-F]{6}$/.test(color) ? color : "#A3A2A5" }}
            aria-hidden
          />
          <Input
            value={color}
            onChange={(e) => setColor(e.target.value)}
            aria-label={`${path} Color hex`}
            className="h-7 w-24 font-mono text-xs"
          />
        </label>

        <Button
          variant={anchored ? "default" : "outline"}
          size="xs"
          aria-pressed={anchored}
          aria-label={`Toggle Anchored for ${path}`}
          onClick={() => setAnchored((v) => !v)}
          className="max-md:min-h-11"
        >
          {anchored ? "Anchored" : "Unanchored"}
        </Button>
      </div>

      {onSave && (
        <Button
          variant="outline"
          size="xs"
          disabled={saving}
          onClick={() => void handleSave()}
          aria-label={`Save properties for ${path}`}
          className="w-full justify-center max-md:min-h-11"
        >
          <Save className="size-3" strokeWidth={1.5} aria-hidden />
          {saving ? "Saving…" : "Save properties"}
        </Button>
      )}

      {(onDuplicate || onDelete) && (
        <div className="grid grid-cols-2 gap-1.5">
          {onDuplicate && (
            <Button
              variant="outline"
              size="xs"
              disabled={saving}
              onClick={() => void onDuplicate(path)}
              aria-label={`Duplicate ${path}`}
              className="w-full justify-center bg-surface max-md:min-h-11"
            >
              <CopyPlus className="size-3" strokeWidth={1.5} aria-hidden />
              Duplicate
            </Button>
          )}
          {onDelete && (
            <Button
              variant="outline"
              size="xs"
              disabled={saving}
              onClick={() => void onDelete(path)}
              aria-label={`Delete ${path}`}
              className="w-full justify-center bg-surface max-md:min-h-11"
            >
              <Trash2 className="size-3" strokeWidth={1.5} aria-hidden />
              Delete
            </Button>
          )}
        </div>
      )}
    </div>
  );
}

export function PartsShelf({
  gameName,
  builtTitles,
  place,
  error,
  collapsed,
  onToggle,
  answerAttr,
  selectedPath,
  onSelectPath,
  onSaveInstance,
  onSaveScript,
  onDuplicatePart,
  onDeletePart,
  onRunPlaytest,
  onDownloadShipPack,
  onImportClick,
  onOpenSnapshots,
  snapshotCount = 0,
  buildDiff,
  serverDiagnostics,
}: {
  gameName: string;
  builtTitles: string[];
  place: Place | null | "error";
  error: boolean;
  collapsed?: boolean;
  onToggle?: () => void;
  answerAttr?: boolean;
  selectedPath?: string | null;
  onSelectPath?: (path: string | null) => void;
  onSaveInstance?: (payload: InstanceEditPayload) => Promise<void>;
  onSaveScript?: (path: string, source: string) => Promise<void> | void;
  onDuplicatePart?: (path: string) => Promise<void> | void;
  onDeletePart?: (path: string) => Promise<void> | void;
  onRunPlaytest?: () => Promise<void> | void;
  onDownloadShipPack?: () => Promise<void> | void;
  onImportClick?: () => void;
  onOpenSnapshots?: () => void;
  snapshotCount?: number;
  buildDiff?: PlaceDiff | null;
  serverDiagnostics?: LuauDiagnostic[];
}) {
  const [query, setQuery] = useState("");
  const [exporting, setExporting] = useState(false);
  const [exportingZip, setExportingZip] = useState(false);
  const q = query.trim().toLowerCase();

  const diagnostics = useMemo(() => {
    if (!place || place === "error") return [] as LuauDiagnostic[];
    const local = analyzePlaceLuau(place);
    if (!serverDiagnostics || serverDiagnostics.length === 0) return local;
    const seen = new Set(local.map((d) => `${d.path}:${d.line}:${d.code}`));
    const merged = [...local];
    for (const sd of serverDiagnostics) {
      const k = `${sd.path}:${sd.line}:${sd.code}`;
      if (!seen.has(k)) merged.push(sd);
    }
    return merged;
  }, [place, serverDiagnostics]);

  const diagByPath = useMemo(() => {
    const m = new Map<string, LuauDiagnostic[]>();
    for (const d of diagnostics) {
      const list = m.get(d.path) || [];
      list.push(d);
      m.set(d.path, list);
    }
    return m;
  }, [diagnostics]);

  const { workspaceEntries, scriptEntries, total, errCount, warnCount } = useMemo(() => {
    if (!place || place === "error") {
      return {
        workspaceEntries: [] as ExplorerEntry[],
        scriptEntries: [] as ExplorerEntry[],
        total: 0,
        errCount: 0,
        warnCount: 0,
      };
    }
    const instEntries: ExplorerEntry[] = Object.entries(place.instances || {}).map(([p, inst]) => {
      const { name, kind } = parsePath(p);
      const cls = inst?.class_name || "Instance";
      const props = inst?.properties ? Object.entries(inst.properties) : [];
      const propLines = props.length ? "\n" + props.map(([k, v]) => `${k} = ${JSON.stringify(v)}`).join("\n") : "";
      return {
        path: p,
        name,
        kind,
        rawCopy: `${p} (${cls})${propLines}`,
        copyLabel: "Copy props",
        metaChip: cls,
        isScript: false,
        instance: inst,
        diagnostics: diagByPath.get(p) || [],
      };
    });
    const scrEntries: ExplorerEntry[] = Object.entries(place.scripts || {}).map(([p, src]) => {
      const { name, kind } = parsePath(p, true);
      const clean = src ? src.trim() : "";
      const lineCount = clean ? clean.split("\n").length : 0;
      return {
        path: p,
        name,
        kind,
        rawCopy: clean || `-- ${p}`,
        copyLabel: "Copy Luau",
        metaChip: `${Math.max(1, lineCount)}L Luau`,
        isScript: true,
        source: clean,
        diagnostics: diagByPath.get(p) || [],
      };
    });
    const errs = diagnostics.filter((d) => d.severity === "error").length;
    const warns = diagnostics.filter((d) => d.severity === "warning").length;
    return {
      workspaceEntries: instEntries,
      scriptEntries: scrEntries,
      total: instEntries.length + scrEntries.length,
      errCount: errs,
      warnCount: warns,
    };
  }, [diagByPath, diagnostics, place]);

  const filteredWorkspace = useMemo(
    () => (q ? workspaceEntries.filter((e) => `${e.name} ${e.path} ${e.kind.label} ${e.metaChip ?? ""}`.toLowerCase().includes(q)) : workspaceEntries),
    [workspaceEntries, q],
  );
  const filteredScripts = useMemo(
    () => (q ? scriptEntries.filter((e) => `${e.name} ${e.path} ${e.kind.label} ${e.metaChip ?? ""}`.toLowerCase().includes(q)) : scriptEntries),
    [scriptEntries, q],
  );

  const summary = useMemo(() => buildGameSummary(gameName, builtTitles, place), [gameName, builtTitles, place]);
  const diffTotal = buildDiff ? buildDiff.added.length + buildDiff.modified.length + buildDiff.removed.length : 0;

  const copySummary = () => {
    if (!summary) return;
    navigator.clipboard.writeText(summary).then(
      () => toast.success("Copied the game summary"),
      () => toast.error("Could not copy automatically. Select the text instead."),
    );
  };

  const exportRbxlx = async () => {
    if (exporting) return;
    setExporting(true);
    try {
      const r = await fetch("/api/export/rbxlx", { cache: "no-store" });
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      const xml = await r.text();
      if (!xml.includes("<roblox")) throw new Error("unexpected XML payload");
      const filename = slugName(gameName);
      const blob = new Blob([xml], { type: "application/xml;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = filename;
      document.body.appendChild(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      toast.success(`Exported ${filename}`);
    } catch {
      toast.error("Could not export .rbxlx right now.");
    } finally {
      setExporting(false);
    }
  };

  const handleShipPackClick = async () => {
    if (!onDownloadShipPack || exportingZip) return;
    setExportingZip(true);
    try {
      await onDownloadShipPack();
    } finally {
      setExportingZip(false);
    }
  };

  if (collapsed) {
    return (
      <aside
        aria-label="In your game"
        className="flex h-full flex-col items-center gap-3 bg-surface px-2 py-3 text-ink"
      >
        {onToggle && (
          <Button
            variant="ghost"
            size="icon"
            onClick={onToggle}
            aria-label="Expand In your game"
            className="max-md:size-11"
          >
            <PanelRightOpen className="size-4" strokeWidth={1.5} aria-hidden />
          </Button>
        )}
        <span className="rounded-chip bg-board px-1.5 py-1 font-mono text-xs tabular-nums text-ink-2">
          <SlidingNumber value={total} />
        </span>
      </aside>
    );
  }

  return (
    <aside
      aria-label="In your game"
      data-answers={answerAttr ? "3" : undefined}
      className="flex h-full min-h-0 w-full flex-col bg-surface text-ink"
    >
      <div className="flex min-h-14 flex-none items-center justify-between gap-2 border-b border-hairline px-3.5">
        <div className="min-w-0">
          <div className="flex items-center gap-1.5">
            <FolderTree className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
            <h2 className="text-ui font-medium text-ink">In your game</h2>
          </div>
          <div className="inline-flex items-center gap-1.5 whitespace-nowrap text-xs text-ink-2">
            <span className="font-mono font-medium tabular-nums text-ink">
              <SlidingNumber value={total} />
            </span>
            {`${total === 1 ? "piece" : "pieces"} in your game`}
          </div>
        </div>

        <div className="flex items-center gap-1">
          {onOpenSnapshots && (
            <Button
              variant="ghost"
              size="xs"
              onClick={onOpenSnapshots}
              aria-label="Open version snapshots"
              className="gap-1 text-ink-2 hover:text-ink max-md:min-h-11"
            >
              <History className="size-3.5" strokeWidth={1.5} aria-hidden />
              {snapshotCount > 0 && <span className="font-mono text-xs tabular-nums">{snapshotCount}</span>}
            </Button>
          )}
          {onToggle && (
            <Button
              variant="ghost"
              size="icon"
              onClick={onToggle}
              aria-label="Collapse In your game"
              className="max-md:size-11"
            >
              <PanelRightClose className="size-4" strokeWidth={1.5} aria-hidden />
            </Button>
          )}
        </div>
      </div>

      {total > 0 && !error && (
        <div className="flex flex-none flex-col gap-2 border-b border-hairline px-3 py-2.5">
          <InputGroup className="bg-board/50">
            <InputGroupAddon align="inline-start" className="ps-2.5">
              <Search className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
            </InputGroupAddon>
            <InputGroupInput
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              aria-label="Filter Explorer"
              placeholder="Filter parts & scripts"
              className="[&_input]:max-md:!min-h-11 [&_input]:md:!h-8"
            />
            {query && (
              <InputGroupAddon align="inline-end" className="has-[>button]:me-0 pe-1">
                <Button
                  variant="ghost"
                  size="icon-xs"
                  onClick={() => setQuery("")}
                  aria-label="Clear Explorer filter"
                  className="max-md:size-11"
                >
                  <X className="size-3.5" strokeWidth={1.5} aria-hidden />
                </Button>
              </InputGroupAddon>
            )}
          </InputGroup>

          {/* Static Luau Audit Summary Badge */}
          {scriptEntries.length > 0 && (
            <div className="flex flex-wrap items-center justify-between gap-1.5 text-xs">
              <span className="inline-flex items-center gap-1 text-ink-2">
                <ShieldCheck className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                Static Luau Audit
              </span>
              {errCount === 0 && warnCount === 0 ? (
                <Pill className="bg-pill-active-bg px-2 py-0 font-mono text-xs text-pill-active-fg">
                  <Check className="size-3" strokeWidth={2} aria-hidden />
                  Clean
                </Pill>
              ) : (
                <div className="flex items-center gap-1">
                  {errCount > 0 && (
                    <Pill className="bg-pill-pro-bg px-1.5 py-0 font-mono text-xs tabular-nums text-pill-pro-fg">
                      {errCount} err
                    </Pill>
                  )}
                  {warnCount > 0 && (
                    <Pill className="bg-pill-warn-bg px-1.5 py-0 font-mono text-xs tabular-nums text-pill-warn-fg">
                      {warnCount} warn
                    </Pill>
                  )}
                </div>
              )}
            </div>
          )}
        </div>
      )}

      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-3">
        {/* Expandable Build Diff on latest build */}
        {buildDiff && diffTotal > 0 && (
          <section aria-label="Latest Build Diff" className="rounded-card border border-hairline bg-board/50 px-2.5 py-2">
            <Expandable expandDirection="vertical" expandBehavior="push">
              {({ isExpanded }) => (
                <>
                  <div className="flex items-center justify-between gap-2">
                    <div className="flex min-w-0 items-center gap-1.5">
                      <FileDiff className="size-3.5 flex-none text-icon" strokeWidth={1.5} aria-hidden />
                      <span className="truncate text-xs font-medium text-ink">Latest Build Diff</span>
                      <div className="flex items-center gap-1">
                        {buildDiff.added.length > 0 && (
                          <span className="rounded-chip bg-pill-active-bg px-1.5 py-0 font-mono text-xs tabular-nums text-pill-active-fg">
                            +{buildDiff.added.length}
                          </span>
                        )}
                        {buildDiff.modified.length > 0 && (
                          <span className="rounded-chip bg-pill-warn-bg px-1.5 py-0 font-mono text-xs tabular-nums text-pill-warn-fg">
                            ~{buildDiff.modified.length}
                          </span>
                        )}
                        {buildDiff.removed.length > 0 && (
                          <span className="rounded-chip bg-pill-pro-bg px-1.5 py-0 font-mono text-xs tabular-nums text-pill-pro-fg">
                            -{buildDiff.removed.length}
                          </span>
                        )}
                      </div>
                    </div>
                    <ExpandableTrigger
                      aria-label={isExpanded ? "Hide build diff" : "Show build diff"}
                      className="inline-flex min-h-11 min-w-11 items-center justify-center rounded-sm text-ink-2 outline-none hover:bg-surface hover:text-ink md:min-h-7 md:min-w-7"
                    >
                      <ChevronDown
                        className={`size-3.5 transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`}
                        strokeWidth={1.5}
                        aria-hidden
                      />
                    </ExpandableTrigger>
                  </div>
                  <ExpandableContent preset="fade">
                    <ul className="mt-2 flex max-h-40 flex-col gap-1 overflow-y-auto border-t border-hairline pt-2 font-mono text-xs">
                      {buildDiff.added.map((it) => (
                        <li key={`a:${it.path}`} className="flex items-center justify-between gap-2 text-pill-active-fg">
                          <span className="truncate">+ {it.path}</span>
                          <span className="shrink-0 text-ink-2">{it.detail}</span>
                        </li>
                      ))}
                      {buildDiff.modified.map((it) => (
                        <li key={`m:${it.path}`} className="flex items-center justify-between gap-2 text-pill-warn-fg">
                          <span className="truncate">~ {it.path}</span>
                          <span className="shrink-0 text-ink-2">{it.detail}</span>
                        </li>
                      ))}
                      {buildDiff.removed.map((it) => (
                        <li key={`r:${it.path}`} className="flex items-center justify-between gap-2 text-pill-pro-fg">
                          <span className="truncate">- {it.path}</span>
                          <span className="shrink-0 text-ink-2">{it.detail}</span>
                        </li>
                      ))}
                    </ul>
                  </ExpandableContent>
                </>
              )}
            </Expandable>
          </section>
        )}

        {error ? (
          <p className="rounded-card border border-hairline bg-board p-3 text-sm text-pill-pro-fg">
            Could not read your game place tree. Check your Studio or Local Place connection in Settings, then use Check again.
          </p>
        ) : !place ? (
          <div className="grid gap-2" aria-label="Loading Studio Explorer">
            <span className="h-10 rounded-card bg-board" />
            <span className="h-10 rounded-card bg-board" />
          </div>
        ) : total === 0 ? (
          <div className="rounded-card border border-hairline bg-board p-3.5 text-sm leading-relaxed text-ink-2">
            No parts or scripts in your place yet. Build a blueprint, drag-and-drop a .rbxlx file, or describe any game below.
          </div>
        ) : filteredWorkspace.length === 0 && filteredScripts.length === 0 ? (
          <div className="rounded-card border border-hairline bg-board p-3 text-sm text-ink-2">
            No Explorer items match &ldquo;{query}&rdquo;.
          </div>
        ) : (
          <>
            {filteredWorkspace.length > 0 && (
              <ExplorerGroup
                title="Workspace Parts"
                entries={filteredWorkspace}
                selectedPath={selectedPath}
                onSelectPath={onSelectPath}
                onSaveInstance={onSaveInstance}
                onSaveScript={onSaveScript}
                onDuplicatePart={onDuplicatePart}
                onDeletePart={onDeletePart}
                onRunPlaytest={onRunPlaytest}
              />
            )}
            {filteredScripts.length > 0 && (
              <ExplorerGroup
                title="Luau Scripts"
                entries={filteredScripts}
                placeInstances={place && place !== "error" ? place.instances : undefined}
                selectedPath={selectedPath}
                onSelectPath={onSelectPath}
                onSaveInstance={onSaveInstance}
                onSaveScript={onSaveScript}
                onDuplicatePart={onDuplicatePart}
                onDeletePart={onDeletePart}
                onRunPlaytest={onRunPlaytest}
              />
            )}
          </>
        )}
      </div>

      <div className="flex flex-none flex-col gap-1.5 border-t border-hairline bg-surface p-3">
        <div className="grid grid-cols-2 gap-1.5">
          <Button
            variant="default"
            disabled={total === 0 || error || exporting}
            onClick={() => void exportRbxlx()}
            className="w-full justify-center max-md:min-h-11"
          >
            <Download className="size-3.5" strokeWidth={1.5} aria-hidden />
            {exporting ? "Exporting…" : "Export .rbxlx"}
          </Button>
          {onImportClick ? (
            <Button
              variant="outline"
              onClick={onImportClick}
              aria-label="Import .rbxlx"
              className="w-full justify-center max-md:min-h-11"
            >
              <Upload className="size-3.5" strokeWidth={1.5} aria-hidden />
              Import .rbxlx
            </Button>
          ) : (
            <Button
              variant="outline"
              disabled={!summary}
              onClick={copySummary}
              className="w-full justify-center max-md:min-h-11"
            >
              <Copy className="size-3.5" strokeWidth={1.5} aria-hidden />
              Copy summary
            </Button>
          )}
        </div>
        {onDownloadShipPack && (
          <Button
            variant="outline"
            disabled={total === 0 || error || exportingZip}
            onClick={() => void handleShipPackClick()}
            aria-label="Download Studio and Rojo Ship Pack (.zip)"
            className="w-full justify-center bg-board/60 font-medium text-ink hover:bg-board max-md:min-h-11"
          >
            <FolderArchive className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
            {exportingZip ? "Packing .zip…" : "Rojo Ship Pack (.zip)"}
          </Button>
        )}
        <Button
          variant="outline"
          disabled={!summary}
          onClick={copySummary}
          className="w-full justify-center max-md:min-h-11"
        >
          <Copy className="size-3.5" strokeWidth={1.5} aria-hidden />
          Copy game summary
        </Button>
      </div>
    </aside>
  );
}

function ExplorerItemRow({
  entry,
  placeInstances,
  isSelected,
  onSelect,
  onSaveInstance,
  onSaveScript,
  onDuplicatePart,
  onDeletePart,
  onRunPlaytest,
}: {
  entry: ExplorerEntry;
  placeInstances?: Record<string, PlaceInstance>;
  isSelected: boolean;
  onSelect?: () => void;
  onSaveInstance?: (payload: InstanceEditPayload) => Promise<void>;
  onSaveScript?: (path: string, source: string) => Promise<void> | void;
  onDuplicatePart?: (path: string) => Promise<void> | void;
  onDeletePart?: (path: string) => Promise<void> | void;
  onRunPlaytest?: () => Promise<void> | void;
}) {
  const [expanded, setExpanded] = useState(isSelected);
  const itemRef = useRef<HTMLLIElement>(null);

  useEffect(() => {
    if (isSelected) {
      setExpanded(true);
      itemRef.current?.scrollIntoView({ block: "nearest" });
    }
  }, [isSelected]);

  const { Icon } = entry.kind;
  const errN = entry.diagnostics.filter((d) => d.severity === "error").length;
  const warnN = entry.diagnostics.filter((d) => d.severity === "warning").length;

  const copyEntry = () => {
    navigator.clipboard.writeText(entry.rawCopy).then(
      () => toast.success(`Copied ${entry.name}`),
      () => toast.error("Could not copy automatically."),
    );
  };

  return (
    <li
      ref={itemRef}
      data-explorer-path={entry.path}
      className={`rounded-card border px-2.5 py-2 transition-[border-color,background-color] duration-(--dur-press) ease-(--ease-house) ${
        isSelected
          ? "border-blue bg-blue-soft/60 ring-2 ring-blue/35"
          : "border-hairline bg-board/45"
      }`}
    >
      <Expandable
        expandDirection="vertical"
        expandBehavior="push"
        expanded={expanded}
        onToggle={() => {
          const next = !expanded;
          setExpanded(next);
          if (next) onSelect?.();
        }}
      >
        {({ isExpanded }) => (
          <>
            <div className="flex items-center gap-2">
              <span className={`grid size-7 flex-none place-items-center rounded-sm ${entry.kind.cls}`}>
                <Icon className="size-3.5" strokeWidth={1.5} aria-hidden />
              </span>
              <div className="flex min-w-0 flex-1 flex-col items-start gap-0.5">
                <span className="w-full truncate text-sm font-medium text-ink">{entry.name}</span>
                <div className="flex flex-wrap items-center gap-1">
                  <Pill className={`px-1.5 py-0 text-xs ${entry.kind.cls}`}>{entry.kind.label}</Pill>
                  {entry.metaChip && (
                    <span className="font-mono text-xs text-ink-3">{entry.metaChip}</span>
                  )}
                  {entry.isScript && errN > 0 && (
                    <Pill className="bg-pill-pro-bg px-1.5 py-0 font-mono text-xs text-pill-pro-fg">
                      {errN} err
                    </Pill>
                  )}
                  {entry.isScript && errN === 0 && warnN > 0 && (
                    <Pill className="bg-pill-warn-bg px-1.5 py-0 font-mono text-xs text-pill-warn-fg">
                      {warnN} warn
                    </Pill>
                  )}
                </div>
              </div>
              <ExpandableTrigger
                aria-label={`Hierarchy details for ${entry.name}`}
                className="inline-flex min-h-11 min-w-11 flex-none items-center justify-center rounded-sm text-ink-2 outline-none hover:bg-surface hover:text-ink md:min-h-7 md:min-w-7"
              >
                <ChevronDown
                  className={`size-3.5 transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`}
                  strokeWidth={1.5}
                  aria-hidden
                />
              </ExpandableTrigger>
            </div>
            <ExpandableContent preset="fade">
              <div className="mt-2 flex flex-col gap-2">
                <div className="flex items-center justify-between gap-2">
                  <code className="min-w-0 truncate font-mono text-xs text-ink-2">{entry.path}</code>
                  <Button
                    variant="ghost"
                    size="xs"
                    onClick={copyEntry}
                    aria-label={`${entry.copyLabel} for ${entry.name}`}
                    className="shrink-0 text-ink-2 hover:text-ink max-md:min-h-11"
                  >
                    <Copy className="size-3" strokeWidth={1.5} aria-hidden />
                    {entry.copyLabel}
                  </Button>
                </div>

                {entry.diagnostics.length > 0 && (
                  <ul className="flex flex-col gap-1 rounded-sm border border-hairline bg-surface p-2 text-xs">
                    {entry.diagnostics.map((d, idx) => (
                      <li
                        key={idx}
                        className={`flex items-start gap-1.5 ${
                          d.severity === "error" ? "text-pill-pro-fg" : "text-pill-warn-fg"
                        }`}
                      >
                        <AlertTriangle className="mt-0.5 size-3 flex-none" strokeWidth={1.5} aria-hidden />
                        <span className="min-w-0 [overflow-wrap:anywhere]">
                          <strong className="font-mono font-medium">L{d.line} [{d.code}]</strong> {d.message}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}

                {entry.isScript ? (
                  <LuauScriptEditor
                    path={entry.path}
                    source={entry.source || `-- ${entry.path}`}
                    placeInstances={placeInstances}
                    onSaveScript={onSaveScript}
                    onRunPlaytest={onRunPlaytest}
                  />
                ) : entry.instance ? (
                  <InstancePropertyEditor
                    path={entry.path}
                    instance={entry.instance}
                    onSave={onSaveInstance}
                    onDuplicate={onDuplicatePart}
                    onDelete={onDeletePart}
                  />
                ) : (
                  <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded-sm border border-hairline bg-surface p-2 font-mono text-xs text-ink">
                    {entry.rawCopy}
                  </pre>
                )}
              </div>
            </ExpandableContent>
          </>
        )}
      </Expandable>
    </li>
  );
}

function ExplorerGroup({
  title,
  entries,
  placeInstances,
  selectedPath,
  onSelectPath,
  onSaveInstance,
  onSaveScript,
  onDuplicatePart,
  onDeletePart,
  onRunPlaytest,
}: {
  title: string;
  entries: ExplorerEntry[];
  placeInstances?: Record<string, PlaceInstance>;
  selectedPath?: string | null;
  onSelectPath?: (path: string | null) => void;
  onSaveInstance?: (payload: InstanceEditPayload) => Promise<void>;
  onSaveScript?: (path: string, source: string) => Promise<void> | void;
  onDuplicatePart?: (path: string) => Promise<void> | void;
  onDeletePart?: (path: string) => Promise<void> | void;
  onRunPlaytest?: () => Promise<void> | void;
}) {
  const visible = entries.slice(0, 60);
  const hiddenCount = entries.length - visible.length;

  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center justify-between px-1">
        <h3 className="text-xs font-medium text-ink-2">{title}</h3>
        <Pill className="bg-board px-2 py-0.5 font-mono text-xs tabular-nums text-ink">
          <SlidingNumber value={entries.length} />
        </Pill>
      </div>

      <ul className="flex flex-col gap-1.5">
        {visible.map((entry) => (
          <ExplorerItemRow
            key={entry.path}
            entry={entry}
            placeInstances={placeInstances}
            isSelected={selectedPath === entry.path}
            onSelect={() => onSelectPath?.(entry.path)}
            onSaveInstance={onSaveInstance}
            onSaveScript={onSaveScript}
            onDuplicatePart={onDuplicatePart}
            onDeletePart={onDeletePart}
            onRunPlaytest={onRunPlaytest}
          />
        ))}
      </ul>

      {hiddenCount > 0 && (
        <p className="px-1 font-mono text-xs text-ink-3">
          +{hiddenCount} more in Studio (use filter above to inspect by name)
        </p>
      )}
    </section>
  );
}

export function StudioChip({
  connected,
  workspaceMode = "studio_mcp",
}: {
  connected: boolean;
  workspaceMode?: WorkspaceMode;
}) {
  const label = !connected
    ? "Studio not connected"
    : workspaceMode === "local_place"
      ? "Local Place ready"
      : workspaceMode === "browser_place"
        ? "Browser Place ready"
        : "Studio connected";
  return (
    <Pill className={`px-2.5 py-1 text-xs font-medium ${connected ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-pro-bg text-pill-pro-fg"}`}>
      <PillStatus className="border-transparent">
        <PillIndicator variant={connected ? "success" : "error"} />
        <span>{label}</span>
      </PillStatus>
    </Pill>
  );
}
