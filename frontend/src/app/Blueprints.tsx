import { Check, Hammer, Layers } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import { ADDS, blueprintMeta, plain, type Idea } from "./data";

export function BlueprintCard({
  idea,
  canBuild,
  busy,
  building,
  unavailable,
  featured,
  onBuild,
}: {
  idea: Idea;
  canBuild: boolean;
  busy: boolean;
  building: boolean;
  unavailable?: "built" | "offer";
  featured?: boolean;
  onBuild: () => void;
}) {
  const meta = blueprintMeta(idea);
  const pitch = plain(idea.pitch || "Generates Luau scripts, places instances in Studio, and runs a playtest.");
  const adds = [...new Set(idea.skills.map((s) => ADDS[s] || s.replace(/_/g, " ")))];

  return (
    <article
      className={`flex h-full min-w-0 flex-col justify-between gap-2.5 rounded-card border bg-surface p-3.5 transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) ${
        building
          ? "border-blue ring-2 ring-blue/30"
          : "border-hairline hover:border-edge/70"
      }`}
    >
      <div className="flex min-w-0 flex-col gap-2">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <Badge className={`border-transparent px-2 py-0.5 text-xs font-medium ${meta.tagClass}`}>
            {meta.category}
          </Badge>
          <span className="rounded-chip bg-board px-2 py-0.5 font-mono text-xs tabular-nums text-ink-2">
            {meta.counts}
          </span>
        </div>

        <h3 className="text-ui font-medium leading-snug text-ink [overflow-wrap:anywhere]">
          {idea.title}
        </h3>

        <p className="text-sm leading-snug text-ink-2 [overflow-wrap:anywhere]">
          {pitch}
        </p>

        {adds.length > 0 && (
          <div className="flex flex-col gap-1">
            <span className="text-xs font-medium text-ink-3">Adds to Studio</span>
            <ul className="flex flex-wrap gap-1" aria-label={`Features added by ${idea.title}`}>
              {adds.map((tag) => (
                <li key={tag} className="min-w-0">
                  <Pill className="max-w-full bg-board px-2 py-0.5 text-xs font-normal text-ink [overflow-wrap:anywhere]">
                    {tag}
                  </Pill>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>

      <div className="mt-0.5 flex flex-col gap-2 border-t border-hairline pt-2.5">
        {unavailable === "built" ? (
          <div className="inline-flex min-h-9 items-center gap-1.5 rounded-lg bg-pill-active-bg px-3 py-1.5 text-xs font-medium text-pill-active-fg">
            <Check className="size-3.5 shrink-0" strokeWidth={1.75} aria-hidden />
            <span>Already in your game</span>
          </div>
        ) : unavailable === "offer" ? (
          <p className="text-xs text-ink-2">Already in your game, or superseded by a newer blueprint.</p>
        ) : (
          <Button
            variant={featured ? "default" : "outline"}
            loading={building}
            disabled={!canBuild || busy}
            onClick={onBuild}
            aria-label={`Build ${idea.title} in Studio`}
            data-focus-key={`idea:${idea.title}`}
            className="w-full max-md:min-h-11"
          >
            <Hammer className="size-4" strokeWidth={1.5} aria-hidden />
            Build in Studio
          </Button>
        )}
      </div>
    </article>
  );
}

export function BlueprintWorkbench({
  ideas,
  builtTitles,
  canBuild,
  cantBuildWhy,
  busy,
  buildingTitle,
  onBuild,
}: {
  ideas: Idea[];
  builtTitles: string[];
  canBuild: boolean;
  cantBuildWhy: string;
  busy: boolean;
  buildingTitle?: string;
  onBuild: (idea: Idea) => void;
}) {
  const hasBuilt = builtTitles.length > 0;
  return (
    <section aria-labelledby="stage-title" data-answers="1" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-end justify-between gap-2">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Layers className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
            <h2 id="stage-title" className="text-title font-medium text-ink">
              {hasBuilt ? "Add a feature blueprint" : "Build in Roblox Studio"}
            </h2>
          </div>
          <p className="mt-1 text-sm text-ink-2">
            {hasBuilt
              ? "Pick a compatible system for your place, or describe any custom mechanic below."
              : "Pick a complete starter blueprint to generate Luau, place instances, and run a playtest, or describe your own game below."}
          </p>
        </div>
      </div>

      {!canBuild && cantBuildWhy && (
        <div role="status" className="rounded-card border border-hairline bg-board px-3.5 py-2.5 text-sm text-ink-2">
          {cantBuildWhy}
        </div>
      )}

      {ideas.length === 0 ? (
        <div className="rounded-card border border-hairline bg-board p-4 text-sm text-ink-2">
          All built-in starter blueprints are already in this place. Describe your next change in the prompt box below.
        </div>
      ) : (
        <div className="grid grid-cols-1 items-stretch gap-3 lg:grid-cols-3">
          {ideas.map((idea, index) => (
            <BlueprintCard
              key={idea.id}
              idea={idea}
              canBuild={canBuild}
              busy={busy}
              building={buildingTitle === idea.title}
              unavailable={builtTitles.includes(idea.title) ? "built" : undefined}
              featured={index === 0 && !builtTitles.includes(idea.title)}
              onBuild={() => onBuild(idea)}
            />
          ))}
        </div>
      )}
    </section>
  );
}
