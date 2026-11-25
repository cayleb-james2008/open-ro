import { useState } from "react";
import { Download, History, Plus, RotateCcw, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pill } from "@/components/kibo-ui/pill";
import {
  MorphingDialog,
  MorphingDialogClose,
  MorphingDialogContainer,
  MorphingDialogContent,
  MorphingDialogDescription,
  MorphingDialogTitle,
} from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import type { PlaceSnapshot } from "./data";

function formatSnapTime(ts: number): string {
  try {
    const ms = ts > 1e12 ? ts : ts * 1000;
    return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  } catch {
    return "saved";
  }
}

export function SnapshotsDialog({
  open,
  onOpenChange,
  snapshots,
  busy,
  onCreateSnapshot,
  onRestoreSnapshot,
  onExportSnapshot,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  snapshots: PlaceSnapshot[];
  busy: boolean;
  onCreateSnapshot: (label: string) => void;
  onRestoreSnapshot: (snap: PlaceSnapshot) => void;
  onExportSnapshot: (snap: PlaceSnapshot) => void;
}) {
  const [label, setLabel] = useState("");

  const createNow = () => {
    const clean = label.trim() || `Checkpoint ${snapshots.length + 1}`;
    onCreateSnapshot(clean);
    setLabel("");
  };

  return (
    <MorphingDialog transition={dialogSpring} open={open} onOpenChange={onOpenChange}>
      <MorphingDialogContainer>
        <MorphingDialogContent className="relative mx-3 flex max-h-[84dvh] w-full max-w-[560px] flex-col rounded-dialog border border-hairline bg-surface text-ink shadow-lift">
          <div data-snapshots-dialog className="flex min-h-0 flex-1 flex-col">
            <div className="flex items-start justify-between gap-3 border-b border-hairline px-5 py-4">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <History className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                  <MorphingDialogTitle className="text-title font-medium text-ink">
                    Version Snapshots
                  </MorphingDialogTitle>
                </div>
                <MorphingDialogDescription className="mt-0.5 text-sm text-ink-2">
                  Restore any prior build state or export a historical snapshot as .rbxlx XML.
                </MorphingDialogDescription>
              </div>
              <MorphingDialogClose
                aria-label="Close snapshots"
                className="static inline-flex size-11 flex-none cursor-pointer items-center justify-center rounded-lg border border-hairline bg-surface text-ink-2 outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-board hover:text-ink md:size-8"
              >
                <X className="size-4" strokeWidth={1.5} aria-hidden />
              </MorphingDialogClose>
            </div>

            <div className="flex items-center gap-2 border-b border-hairline bg-board/40 px-5 py-3">
              <Input
                value={label}
                onChange={(e) => setLabel(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    createNow();
                  }
                }}
                aria-label="Snapshot label"
                placeholder="Snapshot label, e.g. Before boss arena…"
                className="flex-1 bg-surface text-sm"
              />
              <Button
                variant="default"
                size="sm"
                disabled={busy}
                onClick={createNow}
                className="shrink-0 max-md:min-h-11"
              >
                <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />
                Save snapshot
              </Button>
              {snapshots.length > 0 && (
                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy}
                  data-dialog-restore-verified
                  onClick={() => {
                    const target = snapshots.find((s) => s.verified !== false) ?? snapshots[0];
                    if (target) onRestoreSnapshot(target);
                  }}
                  className="shrink-0 max-md:min-h-11"
                >
                  <RotateCcw className="size-3.5" strokeWidth={1.5} aria-hidden />
                  Restore Verified
                </Button>
              )}
            </div>

            <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-5 py-4">
              {snapshots.length === 0 ? (
                <p className="rounded-card border border-hairline bg-board p-4 text-sm text-ink-2">
                  No snapshots saved yet. Build a blueprint, import a .rbxlx file, or click Save snapshot above to create your first restore point.
                </p>
              ) : (
                snapshots.map((snap) => {
                  const addN = snap.diff?.added.length ?? 0;
                  const modN = snap.diff?.modified.length ?? 0;
                  const remN = snap.diff?.removed.length ?? 0;
                  return (
                    <div
                      key={snap.id}
                      className="flex flex-col gap-2 rounded-card border border-hairline bg-board/45 p-3 sm:flex-row sm:items-center sm:justify-between"
                    >
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="truncate text-sm font-medium text-ink">{snap.label}</span>
                          <span className="font-mono text-xs tabular-nums text-ink-3">
                            {formatSnapTime(snap.created_at)}
                          </span>
                        </div>
                        <div className="mt-1 flex flex-wrap items-center gap-1.5">
                          <Pill className="bg-surface px-2 py-0 font-mono text-xs tabular-nums text-ink-2">
                            {snap.parts_count} parts, {snap.scripts_count} scripts
                          </Pill>
                          {addN > 0 && (
                            <Pill className="bg-pill-active-bg px-1.5 py-0 font-mono text-xs tabular-nums text-pill-active-fg">
                              +{addN}
                            </Pill>
                          )}
                          {modN > 0 && (
                            <Pill className="bg-pill-warn-bg px-1.5 py-0 font-mono text-xs tabular-nums text-pill-warn-fg">
                              ~{modN}
                            </Pill>
                          )}
                          {remN > 0 && (
                            <Pill className="bg-pill-pro-bg px-1.5 py-0 font-mono text-xs tabular-nums text-pill-pro-fg">
                              -{remN}
                            </Pill>
                          )}
                        </div>
                      </div>

                      <div className="flex shrink-0 items-center gap-1.5">
                        <Button
                          variant="outline"
                          size="xs"
                          disabled={busy}
                          onClick={() => onRestoreSnapshot(snap)}
                          aria-label={`Restore snapshot ${snap.label}`}
                          className="bg-surface max-md:min-h-11"
                        >
                          <RotateCcw className="size-3.5" strokeWidth={1.5} aria-hidden />
                          Restore
                        </Button>
                        <Button
                          variant="ghost"
                          size="xs"
                          onClick={() => onExportSnapshot(snap)}
                          aria-label={`Export snapshot ${snap.label} as .rbxlx`}
                          className="max-md:min-h-11"
                        >
                          <Download className="size-3.5" strokeWidth={1.5} aria-hidden />
                          .rbxlx
                        </Button>
                      </div>
                    </div>
                  );
                })
              )}
            </div>
          </div>
        </MorphingDialogContent>
      </MorphingDialogContainer>
    </MorphingDialog>
  );
}
