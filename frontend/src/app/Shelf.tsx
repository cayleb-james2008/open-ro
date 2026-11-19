import { ChevronDown, FileCode, Box, Monitor, User, Layers, Archive } from "lucide-react";
import { SlidingNumber } from "@/components/ui/sliding-number";
import { Pill, PillStatus, PillIndicator } from "@/components/kibo-ui/pill";
import { Expandable, ExpandableTrigger } from "@/components/ui/expandable";
import type { Place } from "./data";

type Kind = { label: string; Icon: typeof FileCode; cls: string };
const KINDS: Record<string, Kind> = {
  ServerScriptService: { label: "server script", Icon: FileCode, cls: "bg-tag-sky-bg text-tag-sky-fg" },
  ServerStorage: { label: "stored script", Icon: Archive, cls: "bg-tag-neutral-bg text-tag-neutral-fg" },
  StarterGui: { label: "screen", Icon: Monitor, cls: "bg-tag-design-bg text-tag-design-fg" },
  StarterPlayer: { label: "player script", Icon: User, cls: "bg-tag-research-bg text-tag-research-fg" },
  ReplicatedStorage: { label: "shared script", Icon: Layers, cls: "bg-tag-ops-bg text-tag-ops-fg" },
  Workspace: { label: "in the world", Icon: Box, cls: "bg-tag-mint-bg text-tag-mint-fg" },
};
const FALLBACK: Kind = { label: "part", Icon: Box, cls: "bg-track text-ink-2" };
const niceName = (path: string) => {
  const parts = path.split(".");
  return { name: parts[parts.length - 1].replace(/([a-z])([A-Z])/g, "$1 $2"), kind: KINDS[parts[0]] ?? FALLBACK };
};

/** "In your game": a kraft shelf of stickers. Counts roll; each row keeps its raw path behind Details. */
export function PartsShelf({ place, error, collapsed, onToggle }: { place: Place | null | "error"; error: boolean; collapsed?: boolean; onToggle?: () => void }) {
  const scripts = place && place !== "error" ? Object.keys(place.scripts || {}) : [];
  const parts = place && place !== "error" ? Object.keys(place.instances || {}) : [];
  const total = scripts.length + parts.length;
  return (
    <aside aria-label="In your game" className={`flex h-full min-h-0 flex-col bg-board text-ink ${collapsed ? "w-auto" : "w-full"}`}>
      <div className="flex min-h-14 items-center justify-between gap-2 border-b border-hairline px-4">
        <h2 className="font-heading text-ui font-semibold">In your game</h2>
        {onToggle && (
          <button type="button" onClick={onToggle} aria-expanded={!collapsed} aria-label={collapsed ? "Expand In your game" : "Collapse In your game"}
            className="grid size-11 cursor-pointer place-items-center rounded-sm text-icon outline-none transition-colors hover:bg-track hover:text-ink focus-visible:ring-2 focus-visible:ring-focus">
            <ChevronDown className={`size-4 transition-transform duration-(--dur-swap) ease-(--ease-house) ${collapsed ? "-rotate-90" : ""}`} strokeWidth={1.5} aria-hidden />
          </button>
        )}
      </div>
      {!collapsed && (
        <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-4">
          {error ? (
            <p className="text-sm text-pill-pro-fg">Could not read your game. Check that Studio is connected, then try again.</p>
          ) : !place ? (
            <div className="grid gap-2" aria-hidden><span className="h-11 rounded-card bg-track" /><span className="h-11 rounded-card bg-track" /></div>
          ) : total === 0 ? (
            <p className="text-sm text-ink-2">Nothing built yet. When OpenRO builds something, its scripts and parts show here.</p>
          ) : (
            <>
              {scripts.length > 0 && <Section title="Scripts" names={scripts} />}
              {parts.length > 0 && <Section title="Parts" names={parts} />}
            </>
          )}
        </div>
      )}
    </aside>
  );
}

function Section({ title, names }: { title: string; names: string[] }) {
  return (
    <section>
      <h3 className="mb-2 flex items-center justify-between text-sm font-medium text-ink-2">
        {title}
        <Pill className="bg-raised font-mono text-sm tabular-nums text-ink"><SlidingNumber value={names.length} /></Pill>
      </h3>
      <ul className="flex flex-col gap-2">
        {names.map((n) => {
          const { name, kind } = niceName(n);
          const { Icon } = kind;
          return (
            <li key={n} className="card-stock p-3">
              <Expandable expandDirection="vertical" expandBehavior="push">
                {({ isExpanded }) => (
                  <>
                    <div className="flex items-center gap-2">
                      <span className={`grid size-8 flex-none place-items-center rounded-sm ${kind.cls}`}><Icon className="size-4" strokeWidth={1.5} aria-hidden /></span>
                      <span className="min-w-0 flex-1 break-words text-ui font-medium">{name}</span>
                    </div>
                    <div className="mt-1 flex items-center justify-between gap-2">
                      <Pill className={`text-sm ${kind.cls}`}>{kind.label}</Pill>
                      <ExpandableTrigger aria-expanded={isExpanded} aria-label={`Details for ${name}`} className="inline-flex min-h-11 cursor-pointer items-center gap-1 rounded-sm text-sm text-ink-3 outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-focus">
                        Details<ChevronDown className={`size-4 transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`} strokeWidth={1.5} aria-hidden />
                      </ExpandableTrigger>
                    </div>
                    {isExpanded && <code className="mt-2 block break-all font-mono text-sm text-ink-2">{n}</code>}
                  </>
                )}
              </Expandable>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

export function StudioChip({ connected }: { connected: boolean }) {
  return (
    <Pill className={connected ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-pro-bg text-pill-pro-fg"}>
      <PillStatus className="border-transparent">
        <PillIndicator variant={connected ? "success" : "error"} />
        <span>{connected ? "Roblox Studio connected" : "Studio not connected"}</span>
      </PillStatus>
    </Pill>
  );
}
