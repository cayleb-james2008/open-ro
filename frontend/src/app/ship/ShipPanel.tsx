import { memo, useEffect, useState } from "react";
import { Check, FolderOpen, FlaskConical, Images, Palette, RefreshCw, Rocket, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { call, projUrl, type StudyStatus } from "./api";
import { PublishPanel } from "./PublishPanel";
import { AssetsPanel } from "./AssetsPanel";
import { ArtStudioPanel } from "./ArtStudioPanel";
import { PlaytestPanel } from "./PlaytestPanel";
import { ReadinessAutopilot } from "./ReadinessAutopilot";
import { MonetizationPackCard } from "../MonetizationPackCard";
import type { Place } from "../data";
import type { Ship } from "./useShip";

type Section = "publish" | "assets" | "art" | "playtests";
const SECTIONS: { id: Section; label: string; Icon: typeof Rocket }[] = [
  { id: "publish", label: "Publish & Monetize", Icon: Rocket },
  { id: "assets", label: "Assets", Icon: Images },
  { id: "art", label: "Art & Refs", Icon: Palette },
  { id: "playtests", label: "Playtests", Icon: FlaskConical },
];

/** The Ship & Monetize surface: readiness autopilot, publish readiness, monetization pack, assets, and playtests. */
export const ShipPanel = memo(function ShipPanel({
  ship,
  studioOk,
  place,
  onOpenProjects,
  onOpenSettings,
  onInstalledMonetization,
  onRunPlaytest,
  onOpenCodeDiff,
  initialSection = "publish",
}: {
  ship: Ship;
  studioOk: boolean;
  place?: Place | null | "error";
  onOpenProjects: () => void;
  onOpenSettings: () => void;
  onInstalledMonetization?: () => Promise<void> | void;
  onRunPlaytest?: () => Promise<void> | void;
  onOpenCodeDiff?: (scriptPath: string) => void;
  initialSection?: Section;
}) {
  const [section, setSection] = useState<Section>(initialSection);
  const [study, setStudy] = useState<StudyStatus | null>(null);
  const activeId = ship.active?.id ?? null;

  useEffect(() => {
    setSection(initialSection);
  }, [initialSection]);

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
    return () => {
      live = false;
    };
  }, [activeId, ship.available, section]);

  if (!ship.available || !ship.active) {
    return (
      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pb-4" data-ship-empty>
        {ship.error && (
          <div role="alert" className="card-stock flex items-center justify-between gap-2 border-pill-pro-bg/60 bg-pill-pro-bg/40 p-3 text-sm text-pill-pro-fg">
            <span>{ship.error}</span>
            <Button variant="outline" size="xs" onClick={() => void ship.refresh()}>
              <RefreshCw className="size-3.5" strokeWidth={1.5} aria-hidden />
              Retry
            </Button>
          </div>
        )}
        <MonetizationPackCard
          place={place ?? null}
          studioOk={studioOk}
          activeProjectId={activeId}
          onInstalled={onInstalledMonetization ?? (() => {})}
          onRunPlaytest={onRunPlaytest}
          onOpenCodeDiff={onOpenCodeDiff}
        />
        {ship.available && (
          <div className="card-stock flex flex-col items-start gap-2.5 p-4">
            <h3 className="font-heading text-ui font-medium text-ink">Link a Roblox project to publish</h3>
            <p className="text-sm text-ink-2">
              Open or create a project to run Ship &amp; Monetization Autopilot, sync GamePasses, configure Creator Hub metadata, and publish builds live.
            </p>
            <Button variant="outline" size="sm" onClick={onOpenProjects} className="max-md:min-h-11">
              <FolderOpen className="size-4" strokeWidth={1.5} aria-hidden />
              Open Projects
            </Button>
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden" data-ship>
      <div className="flex min-h-9 flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <h2 className="min-w-0 truncate font-heading text-ui font-medium">{ship.active.name}</h2>
          {study && (
            <div className="flex flex-wrap items-center gap-1.5" data-study-status-bar>
              <div
                className="inline-flex items-center gap-1.5 rounded-full border border-hairline bg-board/60 px-2.5 py-0.5 text-xs"
                aria-label="Project readiness stages"
              >
                {study.stages.map((st) => {
                  const active = study.stage === st.id;
                  return (
                    <span
                      key={st.id}
                      className={`inline-flex items-center gap-1 ${
                        st.done ? "font-medium text-pill-active-fg" : active ? "font-medium text-ink" : "text-ink-3"
                      }`}
                    >
                      {st.done && <Check className="size-3" strokeWidth={2} aria-hidden />}
                      <span>{st.label}</span>
                    </span>
                  );
                })}
              </div>
              <Pill
                className={
                  study.clean_room
                    ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg"
                    : "bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg"
                }
                data-clean-room-badge
              >
                <ShieldCheck className="size-3" strokeWidth={1.5} aria-hidden />
                <span>{study.clean_room ? "Clean Room" : "Deep Mode"}</span>
              </Pill>
            </div>
          )}
        </div>
        <SegmentedControl aria-label="Ship section" size="sm" className="max-w-full flex-wrap">
          {SECTIONS.map(({ id, label, Icon }) => (
            <SegmentedControlItem
              key={id}
              data-checked={section === id ? "" : undefined}
              aria-pressed={section === id}
              onClick={() => setSection(id)}
              className="gap-1 max-md:!h-11 max-md:px-2"
            >
              <Icon className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>{label}</span>
            </SegmentedControlItem>
          ))}
        </SegmentedControl>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto pb-4">
        {section === "publish" && (
          <div className="flex flex-col gap-4">
            <ReadinessAutopilot
              projectId={ship.active.id}
              projectName={ship.active.name}
              onCompleted={async () => {
                await ship.refresh();
                await onInstalledMonetization?.();
              }}
              onOpenCodeFile={onOpenCodeDiff}
              onOpenSettings={onOpenSettings}
            />
            <MonetizationPackCard
              place={place ?? null}
              studioOk={studioOk}
              activeProjectId={activeId}
              onInstalled={onInstalledMonetization ?? (() => {})}
              onRunPlaytest={onRunPlaytest}
              onOpenCodeDiff={onOpenCodeDiff}
            />
            <PublishPanel ship={ship} onOpenSettings={onOpenSettings} />
          </div>
        )}
        {section === "assets" && <AssetsPanel ship={ship} onOpenSettings={onOpenSettings} />}
        {section === "art" && <ArtStudioPanel ship={ship} />}
        {section === "playtests" && <PlaytestPanel ship={ship} studioOk={studioOk} />}
      </div>
    </div>
  );
});
