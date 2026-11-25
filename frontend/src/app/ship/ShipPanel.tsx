import { useEffect, useState } from "react";
import { Check, FolderOpen, FlaskConical, Images, Palette, Rocket, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { call, projUrl, type StudyStatus } from "./api";
import { PublishPanel } from "./PublishPanel";
import { AssetsPanel } from "./AssetsPanel";
import { ArtStudioPanel } from "./ArtStudioPanel";
import { PlaytestPanel } from "./PlaytestPanel";
import type { Ship } from "./useShip";

type Section = "publish" | "assets" | "art" | "playtests";
const SECTIONS: { id: Section; label: string; Icon: typeof Rocket }[] = [
  { id: "publish", label: "Publish & Monetize", Icon: Rocket },
  { id: "assets", label: "Assets", Icon: Images },
  { id: "art", label: "Art & Refs", Icon: Palette },
  { id: "playtests", label: "Playtests", Icon: FlaskConical },
];

/** The center "Ship" surface: publish and products, the asset pipeline, and playtest reports for the open project. */
export function ShipPanel({ ship, studioOk, onOpenProjects, onOpenSettings }: { ship: Ship; studioOk: boolean; onOpenProjects: () => void; onOpenSettings: () => void }) {
  const [section, setSection] = useState<Section>("publish");
  const [study, setStudy] = useState<StudyStatus | null>(null);
  const activeId = ship.active?.id;

  useEffect(() => {
    let live = true;
    if (!activeId || !ship.available) {
      setStudy(null);
      return;
    }
    void call<StudyStatus>(projUrl(activeId, "/study/status")).then((r) => {
      if (!live) return;
      if (r.ok && Array.isArray(r.stages)) setStudy(r as StudyStatus);
      else setStudy(null);
    });
    return () => { live = false; };
  }, [activeId, ship.available, section]);

  if (!ship.available) {
    return <div className="card-stock m-1 p-6 text-sm text-ink-2">This OpenRO build does not have the publish tools yet.</div>;
  }
  if (!ship.active) {
    return (
      <div className="card-stock m-1 flex flex-col items-start gap-3 p-6" data-ship-empty>
        <h2 className="font-heading text-title font-semibold">Open a project to ship it</h2>
        <p className="max-w-[56ch] text-ui text-ink-2">Pick or create a project to publish, manage assets, and run playtests.</p>
        <Button variant="default" size="lg" onClick={onOpenProjects} className="active:opacity-80 disabled:opacity-50 max-md:min-h-11"><FolderOpen className="size-4" strokeWidth={1.5} aria-hidden />Show projects</Button>
      </div>
    );
  }
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden" data-ship>
      <div className="flex min-h-9 flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <h2 className="min-w-0 truncate font-heading text-title font-semibold">{ship.active.name}</h2>
          {study && (
            <div className="flex flex-wrap items-center gap-1.5" data-study-status-bar>
              <div className="inline-flex items-center gap-1 rounded-pill border border-hairline bg-board/60 px-2 py-0.5 text-xs" aria-label="Project readiness stages">
                {study.stages.map((st, idx) => {
                  const active = study.stage === st.id;
                  return (
                    <span
                      key={st.id}
                      className={`inline-flex items-center gap-1 ${
                        st.done ? "font-medium text-pill-active-fg" : active ? "font-medium text-ink" : "text-ink-3"
                      }`}
                    >
                      {idx > 0 && <span className="text-ink-3" aria-hidden>&rarr;</span>}
                      {st.done && <Check className="size-3" strokeWidth={2} aria-hidden />}
                      <span>{st.label}</span>
                    </span>
                  );
                })}
              </div>
              <Pill className={study.clean_room ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg" : "bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg"} data-clean-room-badge>
                <ShieldCheck className="size-3" strokeWidth={1.5} aria-hidden />
                <span>{study.clean_room ? "Clean Room" : "Deep Mode"}</span>
              </Pill>
            </div>
          )}
        </div>
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
        {section === "art" && <ArtStudioPanel ship={ship} />}
        {section === "playtests" && <PlaytestPanel ship={ship} studioOk={studioOk} />}
      </div>
    </div>
  );
}
