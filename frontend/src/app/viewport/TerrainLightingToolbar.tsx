import { useState } from "react";
import {
  Eraser,
  Lightbulb,
  Mountain,
  Paintbrush,
  Plus,
  Sun,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill, PillIndicator } from "@/components/kibo-ui/pill";
import type {
  RendererMode,
  TerrainGeneratorKind,
  WorkspaceLight,
  WorkspaceLightKind,
  WorkspaceTerrain,
} from "./types";

const TERRAIN_PRESETS: Array<{ kind: TerrainGeneratorKind; label: string; amplitude: number; waterLevel: number; material: string }> = [
  { kind: "flat", label: "Flat", amplitude: 0, waterLevel: -4, material: "Grass" },
  { kind: "hills", label: "Hills", amplitude: 16, waterLevel: -4, material: "Grass" },
  { kind: "island", label: "Island", amplitude: 22, waterLevel: -1, material: "Grass" },
  { kind: "canyon", label: "Canyon", amplitude: 24, waterLevel: -2, material: "Rock" },
];

const PAINT_MATERIALS: Array<{ id: string; label: string; dotClass: string }> = [
  { id: "Grass", label: "Grass", dotClass: "bg-emerald-600" },
  { id: "Sand", label: "Sand", dotClass: "bg-amber-300" },
  { id: "Rock", label: "Rock", dotClass: "bg-slate-500" },
  { id: "Snow", label: "Snow", dotClass: "bg-sky-100" },
  { id: "Water", label: "Water", dotClass: "bg-sky-500" },
  { id: "CrackedLava", label: "Lava", dotClass: "bg-orange-600" },
];

export function RendererStatusBadge({
  mode,
  contextLost,
}: {
  mode: RendererMode;
  contextLost: boolean;
}) {
  const label = contextLost
    ? "Restoring 3D"
    : mode === "webgl2"
      ? "WebGL2"
      : "2D Fallback";
  return (
    <Pill
      data-testid="viewport-renderer-badge"
      className={`px-2 py-0.5 text-xs ${
        contextLost
          ? "bg-pill-warn-bg text-pill-warn-fg"
          : mode === "webgl2"
            ? "bg-pill-active-bg text-pill-active-fg"
            : "bg-board text-ink-2"
      }`}
    >
      <PillIndicator
        variant={contextLost ? "warning" : mode === "webgl2" ? "success" : "info"}
        pulse={contextLost}
      />
      <span>{label}</span>
    </Pill>
  );
}

export function TerrainLightingToolbar({
  panel,
  terrain,
  lights,
  selectedPath,
  clockTime,
  busy,
  onGenerateTerrain,
  onPaintBox,
  onClearTerrain,
  onInsertLight,
  onSelectLight,
  onToggleLight,
  onDeleteLight,
  onSetLightingPreset,
}: {
  panel: "terrain" | "lights" | null;
  terrain: WorkspaceTerrain | null;
  lights: WorkspaceLight[];
  selectedPath?: string | null;
  clockTime?: number;
  busy?: boolean;
  onGenerateTerrain: (cfg: { kind: TerrainGeneratorKind; seed: number; amplitude: number; water_level: number; material: string }) => Promise<void> | void;
  onPaintBox: (material: string) => Promise<void> | void;
  onClearTerrain: () => Promise<void> | void;
  onInsertLight: (kind: WorkspaceLightKind) => Promise<void> | void;
  onSelectLight: (path: string) => void;
  onToggleLight: (light: WorkspaceLight) => Promise<void> | void;
  onDeleteLight: (path: string) => Promise<void> | void;
  onSetLightingPreset: (preset: "day" | "golden" | "night") => Promise<void> | void;
}) {
  const activeGen = terrain?.regions?.find((r) => r.generator)?.generator;
  const [paintMaterial, setPaintMaterial] = useState("Sand");
  const [seed, setSeed] = useState(() => String(activeGen?.seed ?? 42));

  if (!panel) return null;

  if (panel === "terrain") {
    const parsedSeed = Number.parseInt(seed, 10);
    const safeSeed = Number.isFinite(parsedSeed) ? parsedSeed : 42;
    const regionCount = terrain?.enabled ? terrain.regions.length : 0;

    return (
      <div
        data-terrain-toolbar
        className="flex flex-col gap-2 border-t border-hairline pt-2"
      >
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="inline-flex items-center gap-1 text-xs font-medium text-ink-2">
              <Mountain className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
              <span>Terrain Generator:</span>
            </span>
            {TERRAIN_PRESETS.map((p) => {
              const isActive = Boolean(terrain?.enabled && activeGen?.kind === p.kind);
              return (
                <Button
                  key={p.kind}
                  data-terrain-preset={p.kind}
                  variant={isActive ? "default" : "outline"}
                  size="xs"
                  disabled={busy}
                  aria-pressed={isActive}
                  onClick={() =>
                    void onGenerateTerrain({
                      kind: p.kind,
                      seed: safeSeed,
                      amplitude: p.amplitude,
                      water_level: p.waterLevel,
                      material: p.material,
                    })
                  }
                  className={isActive ? "max-md:min-h-11" : "bg-surface max-md:min-h-11"}
                >
                  <span>{p.label}</span>
                </Button>
              );
            })}
            <label className="inline-flex items-center gap-1 text-xs text-ink-2">
              <span>Seed</span>
              <input
                type="number"
                aria-label="Terrain seed"
                value={seed}
                onChange={(e) => setSeed(e.target.value)}
                className="h-6 w-16 rounded-sm border border-hairline bg-surface px-1.5 font-mono text-xs tabular-nums text-ink outline-none focus-visible:ring-1 focus-visible:ring-accent"
              />
            </label>
          </div>

          <div className="flex flex-wrap items-center gap-1.5">
            <span className="text-xs text-ink-2">
              {regionCount > 0 ? `${regionCount} region(s)` : "Baseplate mode"}
            </span>
            {terrain?.enabled && (
              <Button
                data-terrain-clear
                variant="outline"
                size="xs"
                disabled={busy}
                onClick={() => void onClearTerrain()}
                className="bg-surface max-md:min-h-11"
              >
                <Eraser className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>Clear</span>
              </Button>
            )}
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          <span className="inline-flex items-center gap-1 text-xs font-medium text-ink-2">
            <Paintbrush className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
            <span>Paint Box:</span>
          </span>
          {PAINT_MATERIALS.map((m) => (
            <Button
              key={m.id}
              variant={paintMaterial === m.id ? "secondary" : "outline"}
              size="xs"
              aria-pressed={paintMaterial === m.id}
              onClick={() => setPaintMaterial(m.id)}
              className={`gap-1.5 max-md:min-h-11 ${paintMaterial === m.id ? "" : "bg-surface"}`}
            >
              <span className={`size-2.5 flex-none rounded-full border border-hairline ${m.dotClass}`} aria-hidden />
              <span>{m.label}</span>
            </Button>
          ))}
          <Button
            data-terrain-paint
            variant="outline"
            size="xs"
            disabled={busy}
            onClick={() => void onPaintBox(paintMaterial)}
            className="bg-surface max-md:min-h-11"
          >
            <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>Apply Paint Box</span>
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div
      data-lights-toolbar
      className="flex flex-col gap-2 border-t border-hairline pt-2"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="inline-flex items-center gap-1 text-xs font-medium text-ink-2">
            <Lightbulb className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
            <span>Insert Light:</span>
          </span>
          {(["PointLight", "SpotLight", "SurfaceLight"] as const).map((kind) => (
            <Button
              key={kind}
              data-add-light={kind}
              variant="outline"
              size="xs"
              disabled={busy}
              onClick={() => void onInsertLight(kind)}
              className="bg-surface max-md:min-h-11"
            >
              <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>{kind}</span>
            </Button>
          ))}
        </div>

        <div className="flex flex-wrap items-center gap-1">
          <span className="inline-flex items-center gap-1 text-xs text-ink-2">
            <Sun className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
            <span>Sky:</span>
          </span>
          {(
            [
              { id: "day", label: "Day", active: clockTime === undefined || (clockTime >= 7.5 && clockTime < 17) },
              { id: "golden", label: "Golden", active: clockTime !== undefined && ((clockTime >= 5.5 && clockTime < 7.5) || (clockTime >= 17 && clockTime < 19.5)) },
              { id: "night", label: "Night", active: clockTime !== undefined && (clockTime < 5.5 || clockTime >= 19.5) },
            ] as const
          ).map((p) => (
            <Button
              key={p.id}
              data-lighting-preset={p.id}
              variant={p.active ? "default" : "outline"}
              size="xs"
              disabled={busy}
              onClick={() => void onSetLightingPreset(p.id)}
              className={p.active ? "max-md:min-h-11" : "bg-surface max-md:min-h-11"}
            >
              <span>{p.label}</span>
            </Button>
          ))}
        </div>
      </div>

      {lights.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5">
          {lights.map((l) => {
            const isSel = selectedPath === l.path;
            const shortName = l.path.split(".").slice(-2).join(".");
            return (
              <div
                key={l.path}
                data-light-item={l.path}
                className={`inline-flex items-center gap-1 rounded-sm border px-1.5 py-0.5 text-xs ${
                  isSel ? "border-accent bg-surface text-ink" : "border-hairline bg-surface/80 text-ink-2"
                }`}
              >
                <button
                  type="button"
                  onClick={() => onSelectLight(l.path)}
                  className="max-w-[11rem] truncate text-left hover:text-ink"
                >
                  {shortName}
                </button>
                <button
                  type="button"
                  aria-label={`Toggle ${shortName}`}
                  onClick={() => void onToggleLight(l)}
                  className={`rounded-sm px-1 py-0.5 text-xs ${
                    l.enabled === false ? "bg-board text-ink-3" : "bg-pill-active-bg text-pill-active-fg"
                  }`}
                >
                  {l.enabled === false ? "Off" : "On"}
                </button>
                <button
                  type="button"
                  aria-label={`Delete ${shortName}`}
                  onClick={() => void onDeleteLight(l.path)}
                  className="text-ink-3 hover:text-ink"
                >
                  <Trash2 className="size-3" strokeWidth={1.5} aria-hidden />
                </button>
              </div>
            );
          })}
        </div>
      ) : (
        <p className="text-xs text-ink-2">
          No scene lights yet. Insert a PointLight, SpotLight, or SurfaceLight to illuminate terrain and parts.
        </p>
      )}
    </div>
  );
}
