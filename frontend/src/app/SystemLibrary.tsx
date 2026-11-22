import { useMemo, useState } from "react";
import { Check, ChevronDown, Layers, Plus, Search, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { Pill } from "@/components/kibo-ui/pill";
import { Expandable, ExpandableContent, ExpandableTrigger } from "@/components/ui/expandable";
import type { Place } from "./data";
import {
  CATEGORY_TAG_CLS,
  GAME_SYSTEMS,
  SYSTEM_CATEGORIES,
  isSystemInPlace,
  type GameSystemDef,
  type SystemCategory,
} from "./systems";

export function SystemLibrary({
  place,
  canBuild,
  busy,
  buildingTitle,
  onAddSystem,
  defaultExpanded = false,
}: {
  place: Place | null | "error";
  canBuild: boolean;
  busy: boolean;
  buildingTitle?: string;
  onAddSystem: (sys: GameSystemDef) => void;
  defaultExpanded?: boolean;
}) {
  const [expanded, setExpanded] = useState(defaultExpanded);
  const [category, setCategory] = useState<"All" | SystemCategory>("All");
  const [query, setQuery] = useState("");

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return GAME_SYSTEMS.filter((sys) => {
      if (category !== "All" && sys.category !== category) return false;
      if (!q) return true;
      return `${sys.title} ${sys.id} ${sys.category} ${sys.summary} ${sys.adds.join(" ")} ${sys.apiTags.join(" ")}`
        .toLowerCase()
        .includes(q);
    });
  }, [category, query]);

  const installedCount = useMemo(
    () => GAME_SYSTEMS.filter((s) => isSystemInPlace(s, place)).length,
    [place],
  );

  return (
    <section
      aria-label="22 Game Systems Library"
      className="rounded-card border border-hairline bg-surface"
    >
      <Expandable
        expandDirection="vertical"
        expandBehavior="push"
        expanded={expanded}
        onToggle={() => setExpanded((v) => !v)}
      >
        {({ isExpanded }) => (
          <>
            <div className="flex flex-wrap items-center justify-between gap-2 bg-board/55 px-4 py-3">
              <div className="flex min-w-0 flex-wrap items-center gap-2">
                <Layers className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                <h3 className="text-ui font-medium text-ink">Game Systems Library</h3>
                <Pill className="bg-tag-sky-bg px-2 py-0.5 font-mono text-xs tabular-nums text-tag-sky-fg">
                  22 Game Systems
                </Pill>
                {installedCount > 0 && (
                  <Pill className="bg-pill-active-bg px-2 py-0.5 font-mono text-xs tabular-nums text-pill-active-fg">
                    {installedCount} in place
                  </Pill>
                )}
              </div>

              <ExpandableTrigger
                aria-label={isExpanded ? "Hide 22 Game Systems Library" : "Browse all 22 Game Systems"}
                className="inline-flex min-h-11 items-center gap-1.5 rounded-lg border border-hairline bg-surface px-3 text-xs font-medium text-ink outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-board md:min-h-8"
              >
                <span>{isExpanded ? "Hide library" : "Browse 22 systems"}</span>
                <ChevronDown
                  className={`size-3.5 transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`}
                  strokeWidth={1.5}
                  aria-hidden
                />
              </ExpandableTrigger>
            </div>

            <ExpandableContent preset="fade">
              <div className="flex flex-col gap-3 border-t border-hairline p-4">
                <div className="flex flex-col gap-2.5 sm:flex-row sm:items-center sm:justify-between">
                  <div className="flex flex-wrap items-center gap-1.5" aria-label="Filter systems by category">
                    {(["All", ...SYSTEM_CATEGORIES] as const).map((cat) => {
                      const active = category === cat;
                      const count =
                        cat === "All"
                          ? GAME_SYSTEMS.length
                          : GAME_SYSTEMS.filter((s) => s.category === cat).length;
                      return (
                        <Button
                          key={cat}
                          variant={active ? "default" : "outline"}
                          size="xs"
                          aria-pressed={active}
                          onClick={() => setCategory(cat)}
                          className="max-md:min-h-11"
                        >
                          <span>{cat}</span>
                          <span className={`font-mono text-xs tabular-nums ${active ? "text-blue-fg" : "text-ink-2"}`}>
                            {count}
                          </span>
                        </Button>
                      );
                    })}
                  </div>

                  <InputGroup className="w-full bg-board/50 sm:w-56">
                    <InputGroupAddon align="inline-start" className="ps-2.5">
                      <Search className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
                    </InputGroupAddon>
                    <InputGroupInput
                      type="search"
                      value={query}
                      onChange={(e) => setQuery(e.target.value)}
                      aria-label="Search 22 game systems"
                      placeholder="Search systems…"
                      className="[&_input]:max-md:!min-h-11 [&_input]:md:!h-8"
                    />
                    {query && (
                      <InputGroupAddon align="inline-end" className="has-[>button]:me-0 pe-1">
                        <Button
                          variant="ghost"
                          size="icon-xs"
                          onClick={() => setQuery("")}
                          aria-label="Clear systems search"
                          className="max-md:size-11"
                        >
                          <X className="size-3.5" strokeWidth={1.5} aria-hidden />
                        </Button>
                      </InputGroupAddon>
                    )}
                  </InputGroup>
                </div>

                {filtered.length === 0 ? (
                  <p className="rounded-card border border-hairline bg-board p-3 text-sm text-ink-2">
                    No Roblox game systems match &ldquo;{query}&rdquo;.
                  </p>
                ) : (
                  <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
                    {filtered.map((sys) => {
                      const active = isSystemInPlace(sys, place);
                      const isBuilding = buildingTitle === sys.title;
                      return (
                        <div
                          key={sys.id}
                          className="flex flex-col justify-between gap-2.5 rounded-card border border-hairline bg-board/40 p-3"
                        >
                          <div className="flex flex-col gap-1.5">
                            <div className="flex flex-wrap items-center justify-between gap-1.5">
                              <Badge className={`border-transparent px-2 py-0.5 text-xs font-medium ${CATEGORY_TAG_CLS[sys.category]}`}>
                                {sys.category}
                              </Badge>
                              <code className="rounded-chip bg-surface px-1.5 py-0.5 font-mono text-xs text-ink-2">
                                {sys.id}
                              </code>
                            </div>
                            <h4 className="text-sm font-medium text-ink">{sys.title}</h4>
                            <p className="text-xs leading-relaxed text-ink-2">{sys.summary}</p>
                            <div className="flex flex-wrap gap-1 pt-0.5">
                              {sys.adds.map((a) => (
                                <Pill key={a} className="bg-surface px-1.5 py-0 text-xs text-ink-2">
                                  {a}
                                </Pill>
                              ))}
                            </div>
                          </div>

                          <div className="border-t border-hairline pt-2">
                            {active ? (
                              <div className="inline-flex min-h-8 items-center gap-1.5 rounded-md bg-pill-active-bg px-2.5 py-1 text-xs font-medium text-pill-active-fg">
                                <Check className="size-3.5" strokeWidth={2} aria-hidden />
                                <span>In your game</span>
                              </div>
                            ) : (
                              <Button
                                variant="outline"
                                size="xs"
                                loading={isBuilding}
                                disabled={!canBuild || busy}
                                onClick={() => onAddSystem(sys)}
                                aria-label={`Add ${sys.title} to game`}
                                className="w-full justify-center bg-surface max-md:min-h-11"
                              >
                                <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />
                                Add to game
                              </Button>
                            )}
                          </div>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            </ExpandableContent>
          </>
        )}
      </Expandable>
    </section>
  );
}
