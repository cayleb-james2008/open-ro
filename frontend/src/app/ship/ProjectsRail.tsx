import { useState, type ReactNode } from "react";
import { FolderOpen, FolderPlus, Hammer, RefreshCw, ScanSearch, UploadCloud } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pill } from "@/components/kibo-ui/pill";
import { SlidingNumber } from "@/components/ui/sliding-number";
import { Expandable, ExpandableTrigger } from "@/components/ui/expandable";
import { MorphingDialog, MorphingDialogClose, MorphingDialogContainer, MorphingDialogContent, MorphingDialogDescription, MorphingDialogTitle, MorphingDialogTrigger } from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import { prettyBytes, stamp } from "./api";
import type { Ship } from "./useShip";

export const triggerCls = "inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-lg border border-hairline bg-surface px-3 text-sm font-medium text-ink outline-none transition-[color,background-color] duration-(--dur-press) ease-(--ease-house) hover:bg-board focus-visible:ring-2 focus-visible:ring-focus";
const closeCls = "static inline-flex min-h-11 cursor-pointer items-center justify-center rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none transition-[color,background-color] duration-(--dur-press) ease-(--ease-house) hover:bg-board focus-visible:ring-2 focus-visible:ring-focus";
const dialogCls = "relative mx-3 flex w-full max-w-[460px] flex-col gap-4 rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift";

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1 text-sm font-medium text-ink-2">
      <span>{label}</span>
      {children}
      {hint && <span className="text-sm font-normal text-ink-3">{hint}</span>}
    </label>
  );
}

const asId = (v: string) => (/^\d+$/.test(v.trim()) ? Number(v.trim()) : undefined);

function CreateDialog({ ship }: { ship: Ship }) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [parent, setParent] = useState("");
  const [uni, setUni] = useState("");
  const [place, setPlace] = useState("");
  const submit = async () => {
    const ok = await ship.createProject({ name: name.trim(), parent: parent.trim() || undefined, universeId: asId(uni), placeId: asId(place) });
    if (ok) { setOpen(false); setName(""); setUni(""); setPlace(""); }
  };
  return (
    <MorphingDialog transition={dialogSpring} open={open} onOpenChange={setOpen}>
      <MorphingDialogTrigger className={`${triggerCls} flex-1`} thing="new-project">
        <FolderPlus className="size-4" strokeWidth={1.5} aria-hidden />New project
      </MorphingDialogTrigger>
      <MorphingDialogContainer>
        <MorphingDialogContent className={dialogCls}>
          <div>
            <MorphingDialogTitle className="font-heading text-title font-semibold">New project</MorphingDialogTitle>
            <MorphingDialogDescription className="mt-1 text-sm text-ink-2">Makes a folder with the Rojo layout and a project file. You can add the game and place numbers later.</MorphingDialogDescription>
          </div>
          <Field label="Name"><Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Untitled Anime Card Game" /></Field>
          <Field label="Inside folder (optional)" hint="Leave empty to use your OpenRO workspace."><Input value={parent} onChange={(e) => setParent(e.target.value)} className="font-mono" placeholder="/home/you/Work/projects" /></Field>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Game number"><Input inputMode="numeric" value={uni} onChange={(e) => setUni(e.target.value)} className="font-mono" placeholder="optional" /></Field>
            <Field label="Place number"><Input inputMode="numeric" value={place} onChange={(e) => setPlace(e.target.value)} className="font-mono" placeholder="optional" /></Field>
          </div>
          <div className="flex flex-wrap justify-end gap-2">
            <MorphingDialogClose aria-label="Cancel" className={closeCls}>Cancel</MorphingDialogClose>
            <Button variant="default" size="lg" disabled={!name.trim() || ship.busy === "create"} onClick={() => void submit()} className="max-md:min-h-11">{ship.busy === "create" ? "Creating…" : "Create project"}</Button>
          </div>
        </MorphingDialogContent>
      </MorphingDialogContainer>
    </MorphingDialog>
  );
}

function OpenDialog({ ship }: { ship: Ship }) {
  const [open, setOpen] = useState(false);
  const [path, setPath] = useState("");
  const submit = async () => { if (await ship.openProject(path.trim())) { setOpen(false); setPath(""); } };
  return (
    <MorphingDialog transition={dialogSpring} open={open} onOpenChange={setOpen}>
      <MorphingDialogTrigger className={`${triggerCls} flex-1`} thing="open-project">
        <FolderOpen className="size-4" strokeWidth={1.5} aria-hidden />Open folder
      </MorphingDialogTrigger>
      <MorphingDialogContainer>
        <MorphingDialogContent className={dialogCls}>
          <div>
            <MorphingDialogTitle className="font-heading text-title font-semibold">Open a project folder</MorphingDialogTitle>
            <MorphingDialogDescription className="mt-1 text-sm text-ink-2">The folder needs a default.project.json or an openro.project.json file.</MorphingDialogDescription>
          </div>
          <Field label="Folder path"><Input value={path} onChange={(e) => setPath(e.target.value)} className="font-mono" placeholder="/home/you/Work/projects/my-game" /></Field>
          <div className="flex flex-wrap justify-end gap-2">
            <MorphingDialogClose aria-label="Cancel" className={closeCls}>Cancel</MorphingDialogClose>
            <Button variant="default" size="lg" disabled={!path.trim() || ship.busy === "open"} onClick={() => void submit()} className="max-md:min-h-11">{ship.busy === "open" ? "Opening…" : "Open project"}</Button>
          </div>
        </MorphingDialogContent>
      </MorphingDialogContainer>
    </MorphingDialog>
  );
}

function Count({ label, value, tone }: { label: string; value: number; tone: string }) {
  return (
    <div className={`flex min-w-0 flex-1 flex-col items-start gap-0.5 rounded-card px-2.5 py-2 ${tone}`}>
      <span className="font-mono text-ui font-medium tabular-nums"><SlidingNumber value={value} /></span>
      <span className="text-sm">{label}</span>
    </div>
  );
}

/** The left dock's Projects section: list, create/open, and the open project's build and Studio sync. */
export function ProjectsRail({ ship, studioOk }: { ship: Ship; studioOk: boolean }) {
  const { projects, active, busy, build, diff, sync, detail } = ship;
  const lastBuild = build ?? active?.last_build ?? null;
  const syncBlocked = !studioOk || !active;
  return (
    <section aria-label="Projects" className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto">
      {!ship.available ? (
        <p className="rounded-card bg-board p-3 text-sm text-ink-2">This OpenRO build does not have projects yet.</p>
      ) : (
        <>
          <div className="flex gap-2">
            <CreateDialog ship={ship} />
            <OpenDialog ship={ship} />
          </div>
          {ship.error && <p role="alert" className="rounded-card bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">{ship.error}</p>}
          {!ship.loaded ? (
            <div className="grid gap-2" aria-hidden><span className="h-14 rounded-card bg-track" /><span className="h-14 rounded-card bg-track" /></div>
          ) : projects.length === 0 ? (
            <p className="rounded-card bg-board p-3 text-sm text-ink-2">No projects yet. Create one, or open a folder that already has a Rojo project.</p>
          ) : (
            <ul className="flex flex-col gap-2" aria-label="Your projects">
              {projects.map((p) => {
                const on = p.id === active?.id;
                return (
                  <li key={p.id}>
                    <Button
                      variant="ghost"
                      onClick={() => { if (!on) void ship.openProject(p.path); }}
                      aria-current={on ? "true" : undefined}
                      data-project={p.id}
                      className={`h-auto min-h-14 w-full flex-col items-start gap-1 whitespace-normal rounded-card border px-3 py-2 text-left sm:h-auto ${on ? "border-hairline bg-blue-soft" : "border-transparent hover:bg-track"}`}
                    >
                      <span className="flex w-full min-w-0 items-center justify-between gap-2">
                        <span className="truncate text-ui font-medium text-ink">{p.name}</span>
                        <Pill className={`flex-none px-2 py-0.5 text-sm ${p.universeId ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-inactive-bg text-pill-inactive-fg"}`}>{p.universeId ? "linked" : "not linked"}</Pill>
                      </span>
                      <span className="w-full truncate font-mono text-sm text-ink-2">{p.path}</span>
                    </Button>
                  </li>
                );
              })}
            </ul>
          )}

          {active && (
            <div className="card-stock flex flex-col gap-3 p-3" data-active-project>
              <div className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <h3 className="truncate font-heading text-ui font-semibold">{active.name}</h3>
                  <p className="text-sm text-ink-2">
                    {detail?.tree ? `${detail.tree.instances} parts · ${detail.tree.scripts} scripts` : "Reading the project…"}
                  </p>
                </div>
                <Button variant="ghost" size="icon" aria-label="Reload the project" onClick={() => void ship.refresh()} className="max-md:size-11"><RefreshCw className="size-4" strokeWidth={1.5} aria-hidden /></Button>
              </div>

              <div className="grid grid-cols-2 gap-2">
                <Button variant="default" size="lg" disabled={!!busy} onClick={() => void ship.buildNow()} className="max-md:min-h-11"><Hammer className="size-4" strokeWidth={1.5} aria-hidden />{busy === "build" ? "Building…" : "Build"}</Button>
                <Button variant="outline" size="lg" disabled={!!busy || syncBlocked} onClick={() => void ship.checkChanges()} className="max-md:min-h-11"><ScanSearch className="size-4" strokeWidth={1.5} aria-hidden />{busy === "diff" ? "Comparing…" : "Compare"}</Button>
              </div>
              <Button variant="outline" size="lg" disabled={!!busy || syncBlocked} onClick={() => void ship.syncNow(false)} className="w-full max-md:min-h-11"><UploadCloud className="size-4" strokeWidth={1.5} aria-hidden />{busy === "sync" ? "Sending to Studio…" : "Send to Studio"}</Button>
              {!studioOk && <p className="text-sm text-ink-2">Connect Roblox Studio to compare or send.</p>}

              {lastBuild && (
                <div className="flex flex-col gap-1 border-t border-hairline pt-2.5 text-sm" data-build-result>
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <span className="font-medium text-ink">Last build</span>
                    <Pill className="bg-tag-sky-bg px-2 py-0.5 font-mono text-sm text-tag-sky-fg">{lastBuild.engine}</Pill>
                  </div>
                  <div className="flex flex-wrap gap-x-3 gap-y-0.5 text-ink-2">
                    <span><span className="font-mono tabular-nums">{lastBuild.instances}</span> parts</span>
                    <span><span className="font-mono tabular-nums">{lastBuild.scripts}</span> scripts</span>
                    <span className="font-mono tabular-nums">{prettyBytes(lastBuild.bytes)}</span>
                    <span>{stamp(lastBuild.at)}</span>
                  </div>
                  <code className="truncate font-mono text-sm text-ink-2">{lastBuild.path}</code>
                  {lastBuild.warnings?.length > 0 && (
                    <Expandable expandDirection="vertical" expandBehavior="push">
                      {({ isExpanded }) => (
                        <>
                          <ExpandableTrigger aria-label="Build warnings" className="inline-flex min-h-11 cursor-pointer items-center text-left text-sm text-pill-warn-fg outline-none hover:underline focus-visible:ring-2 focus-visible:ring-focus">
                            {lastBuild.warnings.length} warning{lastBuild.warnings.length === 1 ? "" : "s"}, {isExpanded ? "hide" : "show"}
                          </ExpandableTrigger>
                          {isExpanded && <ul className="flex flex-col gap-1 text-sm text-ink-2">{lastBuild.warnings.map((w, i) => <li key={i}>{w}</li>)}</ul>}
                        </>
                      )}
                    </Expandable>
                  )}
                </div>
              )}

              {diff && (
                <div className="flex flex-col gap-2" data-diff-result>
                  <div className="grid grid-cols-2 gap-1.5">
                    <Count label="new" value={diff.summary.create} tone="bg-pill-active-bg text-pill-active-fg" />
                    <Count label="changed" value={diff.summary.update} tone="bg-pill-warn-bg text-pill-warn-fg" />
                    <Count label="same" value={diff.summary.unchanged} tone="bg-pill-inactive-bg text-pill-inactive-fg" />
                    <Count label="extra" value={diff.summary.extra} tone="bg-pill-pro-bg text-pill-pro-fg" />
                  </div>
                  {diff.changes.length > 0 && (
                    <ul className="flex max-h-40 flex-col gap-1 overflow-y-auto text-sm" aria-label="What would change">
                      {diff.changes.slice(0, 80).map((c) => (
                        <li key={c.op + c.path} className="flex items-baseline gap-2"><span className="w-14 flex-none text-ink-2">{c.op === "create" ? "new" : c.op === "update" ? "changed" : "extra"}</span><code className="min-w-0 break-all font-mono text-ink">{c.path}</code></li>
                      ))}
                    </ul>
                  )}
                  {diff.summary.create + diff.summary.update + diff.summary.extra === 0 && <p className="text-sm text-ink-2">Studio already matches the project.</p>}
                  {diff.summary.extra > 0 && (
                    <Button variant="outline" size="sm" disabled={!!busy} onClick={() => void ship.syncNow(true)} className="self-start max-md:min-h-11">Send and remove the extras</Button>
                  )}
                </div>
              )}

              {sync && (
                <div className="flex flex-col gap-1 border-t border-hairline pt-2.5 text-sm" data-sync-result>
                  <span className="font-medium text-ink">Sent to Studio</span>
                  <div className="flex flex-wrap gap-x-3 gap-y-0.5 text-ink-2">
                    <span><span className="font-mono tabular-nums">{sync.applied?.created ?? 0}</span> added</span>
                    <span><span className="font-mono tabular-nums">{sync.applied?.updated ?? 0}</span> updated</span>
                    <span><span className="font-mono tabular-nums">{sync.applied?.deleted ?? 0}</span> removed</span>
                  </div>
                  <span className={sync.verify && sync.verify.remaining > 0 ? "text-pill-warn-fg" : "text-ink-2"}>{sync.verify ? (sync.verify.remaining === 0 ? "Checked after sending: nothing left to send." : `${sync.verify.remaining} still differ in Studio.`) : ""}</span>
                  {sync.errors?.map((e, i) => <span key={i} className="text-pill-pro-fg">{e}</span>)}
                </div>
              )}
            </div>
          )}
        </>
      )}
    </section>
  );
}
