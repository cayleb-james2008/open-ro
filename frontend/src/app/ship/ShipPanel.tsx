import { useState } from "react";
import { FolderOpen, FlaskConical, Images, Rocket } from "lucide-react";
import { Button } from "@/components/ui/button";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { PublishPanel } from "./PublishPanel";
import { AssetsPanel } from "./AssetsPanel";
import { PlaytestPanel } from "./PlaytestPanel";
import type { Ship } from "./useShip";

type Section = "publish" | "assets" | "playtests";
const SECTIONS: { id: Section; label: string; Icon: typeof Rocket }[] = [
  { id: "publish", label: "Publish & Monetize", Icon: Rocket },
  { id: "assets", label: "Assets", Icon: Images },
  { id: "playtests", label: "Playtests", Icon: FlaskConical },
];

/** The center "Ship" surface: publish and products, the asset pipeline, and playtest reports for the open project. */
export function ShipPanel({ ship, studioOk, onOpenProjects, onOpenSettings }: { ship: Ship; studioOk: boolean; onOpenProjects: () => void; onOpenSettings: () => void }) {
  const [section, setSection] = useState<Section>("publish");
  if (!ship.available) {
    return <div className="card-stock m-1 p-6 text-sm text-ink-2">This OpenRO build does not have the publish tools yet.</div>;
  }
  if (!ship.active) {
    return (
      <div className="card-stock m-1 flex flex-col items-start gap-3 p-6" data-ship-empty>
        <h2 className="font-heading text-title font-semibold">Open a project to ship it</h2>
        <p className="max-w-[56ch] text-ui text-ink-2">Publishing, asset uploads and playtest reports belong to a project. Pick one from the Projects list, or make a new one.</p>
        <Button variant="default" size="lg" onClick={onOpenProjects} className="max-md:min-h-11"><FolderOpen className="size-4" strokeWidth={1.5} aria-hidden />Show projects</Button>
      </div>
    );
  }
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden" data-ship>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="min-w-0 truncate font-heading text-title font-semibold">{ship.active.name}</h2>
        <SegmentedControl aria-label="Ship section" size="sm" className="max-w-full flex-wrap">
          {SECTIONS.map(({ id, label, Icon }) => (
            <SegmentedControlItem key={id} data-checked={section === id ? "" : undefined} aria-pressed={section === id} onClick={() => setSection(id)} className="gap-1 max-md:!h-11 max-md:px-2">
              <Icon className="size-3.5" strokeWidth={1.5} aria-hidden /><span>{label}</span>
            </SegmentedControlItem>
          ))}
        </SegmentedControl>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto pb-4">
        {section === "publish" && <PublishPanel ship={ship} onOpenSettings={onOpenSettings} />}
        {section === "assets" && <AssetsPanel ship={ship} onOpenSettings={onOpenSettings} />}
        {section === "playtests" && <PlaytestPanel ship={ship} studioOk={studioOk} />}
      </div>
    </div>
  );
}
