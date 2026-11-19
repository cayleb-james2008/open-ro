import { ChevronDown } from "lucide-react";
import { SlidingNumber } from "@/components/ui/sliding-number";
import { Pill } from "@/components/kibo-ui/pill";
import { PillStatus, PillIndicator } from "@/components/kibo-ui/pill";
import { Expandable, ExpandableTrigger, ExpandableContent } from "@/components/ui/expandable";
import type { Place } from "./data";

const PLACE_KIND: Record<string, string> = {
  ServerScriptService: "server script", ServerStorage: "stored script", StarterGui: "screen",
  StarterPlayer: "player script", ReplicatedStorage: "shared script", Workspace: "in the world",
};
const niceName = (path: string) => {
  const parts = path.split(".");
  return { name: parts[parts.length - 1].replace(/([a-z])([A-Z])/g, "$1 $2"), kind: PLACE_KIND[parts[0]] || "part" };
};

/** "In your game": the kraft shelf. Scripts and parts as stickers, each with its raw path behind Details. */
export function PartsShelf({ place, error, collapsed, onToggle }: { place: Place | null | "error"; error: boolean; collapsed?: boolean; onToggle?: () => void }) {
  const scripts = place && place !== "error" ? Object.keys(place.scripts || {}) : [];
  const parts = place && place !== "error" ? Object.keys(place.instances || {}) : [];
  const total = scripts.length + parts.length;
  return (
    <aside aria-label="In your game" className={`flex h-full min-h-0 flex-col bg-board text-ink ${collapsed ? "w-auto" : "w-[300px]"}`}>
      <div className="flex min-h-14 items-center justify-between gap-2 border-b border-hairline px-4">
        <h2 className="font-heading text-ui font-semibold">In your game</h2>
        {onToggle && (
          <button
            type="button"
            onClick={onToggle}
            aria-expanded={!collapsed}
            aria-label={collapsed ? "Expand In your game" : "Collapse In your game"}
            className="grid size-11 cursor-pointer place-items-center rounded-sm text-icon outline-none transition-colors hover:bg-track hover:text-ink focus-visible:ring-2 focus-visible:ring-blue"
          >
            <ChevronDown className={`size-4 transition-transform duration-(--dur-swap) ease-(--ease-house) ${collapsed ? "rotate-90" : ""}`} strokeWidth={1.5} aria-hidden />
          </button>
        )}
      </div>
      {!collapsed && (
        <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-4">
          {error ? (
            <p className="text-sm text-ink-2">Could not read your game. Check that Studio is connected, then try again.</p>
          ) : !place ? (
            <div className="grid gap-2" aria-hidden><span className="h-9 rounded-sm bg-track" /><span className="h-9 rounded-sm bg-track" /></div>
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
        <span className="font-mono tabular-nums text-ink"><SlidingNumber value={names.length} /></span>
      </h3>
      <ul className="flex flex-col gap-2">
        {names.map((n) => {
          const { name, kind } = niceName(n);
          return (
            <li key={n} className="rounded-card border border-hairline bg-surface p-3 shadow-card">
              <Expandable expandDirection="vertical" expandBehavior="push">
                {({ isExpanded }) => (
                  <>
                    <div className="flex items-center justify-between gap-2">
                      <span className="min-w-0 break-words text-ui font-medium">{name}</span>
                      <Pill className="bg-track text-sm text-ink-2">{kind}</Pill>
                    </div>
                    <ExpandableTrigger aria-expanded={isExpanded} className="mt-1 cursor-pointer rounded-sm text-sm text-ink-3 outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-blue">Details</ExpandableTrigger>
                    <ExpandableContent preset="fade">
                      <code className="mt-1 block break-all font-mono text-sm text-ink-2">{n}</code>
                    </ExpandableContent>
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
        <span className={connected ? "max-sm:sr-only" : "max-sm:sr-only"}>{connected ? "Roblox Studio connected" : "Studio not connected"}</span>
      </PillStatus>
    </Pill>
  );
}
