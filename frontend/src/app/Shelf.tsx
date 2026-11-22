import { useMemo, useState } from "react";
import { toast } from "sonner";
import {
  Archive,
  Box,
  ChevronDown,
  Copy,
  FileCode,
  FolderTree,
  Layers,
  Monitor,
  PanelRightClose,
  PanelRightOpen,
  Search,
  User,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { Expandable, ExpandableContent, ExpandableTrigger } from "@/components/ui/expandable";
import { SlidingNumber } from "@/components/ui/sliding-number";
import { Pill, PillIndicator, PillStatus } from "@/components/kibo-ui/pill";
import type { Place, PlaceInstance } from "./data";

type KindInfo = { label: string; service: string; Icon: typeof FileCode; cls: string };

const KINDS: Record<string, KindInfo> = {
  ServerScriptService: { label: "server script", service: "ServerScriptService", Icon: FileCode, cls: "bg-tag-sky-bg text-tag-sky-fg" },
  ServerStorage: { label: "stored script", service: "ServerStorage", Icon: Archive, cls: "bg-tag-neutral-bg text-tag-neutral-fg" },
  StarterGui: { label: "screen GUI", service: "StarterGui", Icon: Monitor, cls: "bg-tag-design-bg text-tag-design-fg" },
  StarterPlayer: { label: "player script", service: "StarterPlayer", Icon: User, cls: "bg-tag-research-bg text-tag-research-fg" },
  ReplicatedStorage: { label: "shared module", service: "ReplicatedStorage", Icon: Layers, cls: "bg-tag-ops-bg text-tag-ops-fg" },
  Workspace: { label: "in the world", service: "Workspace", Icon: Box, cls: "bg-tag-mint-bg text-tag-mint-fg" },
};

const FALLBACK_KIND: KindInfo = { label: "instance", service: "Workspace", Icon: Box, cls: "bg-board text-ink-2" };

export type ExplorerEntry = {
  path: string;
  name: string;
  kind: KindInfo;
  detail: string;
  metaChip?: string;
};

const PART_SCRIPT_KIND: KindInfo = { label: "part script", service: "Workspace", Icon: FileCode, cls: "bg-tag-sky-bg text-tag-sky-fg" };

function parsePath(path: string, isScript = false): { name: string; kind: KindInfo } {
  const segs = path.split(".");
  const rawLast = segs[segs.length - 1] || path;
  const spaced = rawLast.replace(/([a-z])([A-Z])/g, "$1 $2");
  if (isScript && segs[0] === "Workspace") return { name: spaced, kind: PART_SCRIPT_KIND };
  return { name: spaced, kind: KINDS[segs[0]] ?? FALLBACK_KIND };
}

function formatInstanceDetail(path: string, inst?: PlaceInstance): { detail: string; metaChip?: string } {
  const cls = inst?.class_name || "Instance";
  const props = inst?.properties ? Object.entries(inst.properties) : [];
  const propLines = props.length
    ? "\n" + props.map(([k, v]) => `${k} = ${JSON.stringify(v)}`).join("\n")
    : "";
  return { detail: `${path} (${cls})${propLines}`, metaChip: cls };
}

function formatScriptDetail(path: string, source: string): { detail: string; metaChip: string } {
  const lines = source ? source.trim().split("\n") : [];
  const preview = lines.slice(0, 8).join("\n");
  return {
    detail: preview ? `${path}\n\n${preview}${lines.length > 8 ? "\n-- …" : ""}` : path,
    metaChip: `${Math.max(1, lines.length)}L Luau`,
  };
}

export function buildGameSummary(gameName: string, builtTitles: string[], place: Place | null | "error"): string {
  const scripts = place && place !== "error" ? Object.keys(place.scripts || {}) : [];
  const parts = place && place !== "error" ? Object.keys(place.instances || {}) : [];
  const total = scripts.length + parts.length;
  if (!builtTitles.length && total === 0) return "";
  const titlePart = builtTitles.length ? builtTitles.join(", ") : (gameName || "Custom Studio place");
  return `My Roblox game has ${titlePart}, ${total} ${total === 1 ? "piece" : "pieces"} in all (${parts.length} Workspace ${parts.length === 1 ? "part" : "parts"}, ${scripts.length} Luau ${scripts.length === 1 ? "script" : "scripts"}), built and playtested with OpenRO.`;
}

export function PartsShelf({
  gameName,
  builtTitles,
  place,
  error,
  collapsed,
  onToggle,
  answerAttr,
}: {
  gameName: string;
  builtTitles: string[];
  place: Place | null | "error";
  error: boolean;
  collapsed?: boolean;
  onToggle?: () => void;
  answerAttr?: boolean;
}) {
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();

  const { workspaceEntries, scriptEntries, total } = useMemo(() => {
    if (!place || place === "error") {
      return { workspaceEntries: [] as ExplorerEntry[], scriptEntries: [] as ExplorerEntry[], total: 0 };
    }
    const instEntries: ExplorerEntry[] = Object.entries(place.instances || {}).map(([p, inst]) => {
      const { name, kind } = parsePath(p);
      const { detail, metaChip } = formatInstanceDetail(p, inst);
      return { path: p, name, kind, detail, metaChip };
    });
    const scrEntries: ExplorerEntry[] = Object.entries(place.scripts || {}).map(([p, src]) => {
      const { name, kind } = parsePath(p, true);
      const { detail, metaChip } = formatScriptDetail(p, src);
      return { path: p, name, kind, detail, metaChip };
    });
    return {
      workspaceEntries: instEntries,
      scriptEntries: scrEntries,
      total: instEntries.length + scrEntries.length,
    };
  }, [place]);

  const filteredWorkspace = useMemo(
    () => (q ? workspaceEntries.filter((e) => `${e.name} ${e.path} ${e.kind.label} ${e.metaChip ?? ""}`.toLowerCase().includes(q)) : workspaceEntries),
    [workspaceEntries, q],
  );
  const filteredScripts = useMemo(
    () => (q ? scriptEntries.filter((e) => `${e.name} ${e.path} ${e.kind.label} ${e.metaChip ?? ""}`.toLowerCase().includes(q)) : scriptEntries),
    [scriptEntries, q],
  );

  const summary = useMemo(() => buildGameSummary(gameName, builtTitles, place), [gameName, builtTitles, place]);

  const copySummary = () => {
    if (!summary) return;
    navigator.clipboard.writeText(summary).then(
      () => toast.success("Copied the game summary"),
      () => toast.error("Could not copy automatically. Select the text instead."),
    );
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

      {total > 0 && !error && (
        <div className="flex-none border-b border-hairline px-3 py-2.5">
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
        </div>
      )}

      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-3">
        {error ? (
          <p className="rounded-card border border-hairline bg-board p-3 text-sm text-pill-pro-fg">
            Could not read your game place tree. Check that Roblox Studio is connected, then use Check again.
          </p>
        ) : !place ? (
          <div className="grid gap-2" aria-label="Loading Studio Explorer">
            <span className="h-10 rounded-card bg-board" />
            <span className="h-10 rounded-card bg-board" />
          </div>
        ) : total === 0 ? (
          <div className="rounded-card border border-hairline bg-board p-3.5 text-sm leading-relaxed text-ink-2">
            No parts or scripts in your Studio place yet. Build a blueprint or send a prompt and your Workspace instances and Luau scripts appear here.
          </div>
        ) : filteredWorkspace.length === 0 && filteredScripts.length === 0 ? (
          <div className="rounded-card border border-hairline bg-board p-3 text-sm text-ink-2">
            No Explorer items match &ldquo;{query}&rdquo;.
          </div>
        ) : (
          <>
            {filteredWorkspace.length > 0 && (
              <ExplorerGroup title="Workspace Parts" entries={filteredWorkspace} />
            )}
            {filteredScripts.length > 0 && (
              <ExplorerGroup title="Luau Scripts" entries={filteredScripts} />
            )}
          </>
        )}
      </div>

      <div className="flex-none border-t border-hairline bg-surface p-3">
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

function ExplorerGroup({ title, entries }: { title: string; entries: ExplorerEntry[] }) {
  // For huge 1000+ part stress tests (replay.mjs), render the first 60 rows cleanly + count summary so DOM stays instant!
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
        {visible.map((entry) => {
          const { Icon } = entry.kind;
          return (
            <li key={entry.path} className="rounded-card border border-hairline bg-board/45 px-2.5 py-2">
              <Expandable expandDirection="vertical" expandBehavior="push">
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
                      <pre className="mt-2 max-h-36 overflow-auto whitespace-pre-wrap break-all rounded-sm border border-hairline bg-surface p-2 font-mono text-xs text-ink-2">
                        {entry.detail}
                      </pre>
                    </ExpandableContent>
                  </>
                )}
              </Expandable>
            </li>
          );
        })}
      </ul>

      {hiddenCount > 0 && (
        <p className="px-1 font-mono text-xs text-ink-3">
          +{hiddenCount} more in Studio (use filter above to inspect by name)
        </p>
      )}
    </section>
  );
}

export function StudioChip({ connected }: { connected: boolean }) {
  return (
    <Pill className={`px-2.5 py-1 text-xs font-medium ${connected ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-pro-bg text-pill-pro-fg"}`}>
      <PillStatus className="border-transparent">
        <PillIndicator variant={connected ? "success" : "error"} />
        <span>{connected ? "Studio connected" : "Studio not connected"}</span>
      </PillStatus>
    </Pill>
  );
}
