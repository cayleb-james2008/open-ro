import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { CircleCheck, CircleX, ExternalLink, Play } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Expandable, ExpandableTrigger } from "@/components/ui/expandable";
import { MorphingDialog, MorphingDialogClose, MorphingDialogContainer, MorphingDialogContent, MorphingDialogTitle, MorphingDialogTrigger } from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import { call, enc, projUrl, stamp, type PlaytestReport, type PlaytestRun } from "./api";
import type { Ship } from "./useShip";

type View = "summary" | "html" | "json";
const shotUrl = (id: string, run: string, file: string) => projUrl(id, `/playtests/${enc(run)}/shots/${file.split("/").pop()}`);

function Verdict({ passed }: { passed: boolean }) {
  return (
    <Pill className={`gap-1.5 px-2.5 py-0.5 text-sm ${passed ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-pro-bg text-pill-pro-fg"}`}>
      {passed ? <CircleCheck className="size-3.5" strokeWidth={1.5} aria-hidden /> : <CircleX className="size-3.5" strokeWidth={1.5} aria-hidden />}{passed ? "Passed" : "Failed"}
    </Pill>
  );
}

function Lines({ title, lines, tone }: { title: string; lines: string[]; tone: string }) {
  if (!lines.length) return null;
  return (
    <Expandable expandDirection="vertical" expandBehavior="push">
      {({ isExpanded }) => (
        <div className="card-stock p-3">
          <ExpandableTrigger aria-label={title} className="flex min-h-11 w-full cursor-pointer items-center justify-between gap-2 text-left text-sm font-medium outline-none focus-visible:ring-2 focus-visible:ring-focus">
            <span>{title}</span><span className={`font-mono tabular-nums ${tone}`}>{lines.length}</span>
          </ExpandableTrigger>
          {isExpanded && <pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-words font-mono text-sm text-ink-2">{lines.join("\n")}</pre>}
        </div>
      )}
    </Expandable>
  );
}

function RunDetail({ id, run }: { id: string; run: PlaytestRun }) {
  const [report, setReport] = useState<PlaytestReport | null>(null);
  const [err, setErr] = useState("");
  const [view, setView] = useState<View>("summary");
  useEffect(() => {
    let live = true;
    setReport(null); setErr(""); setView("summary");
    void call<PlaytestReport>(projUrl(id, `/playtests/${enc(run.run_id)}`)).then((r) => {
      if (!live) return;
      if (r.ok) setReport(r as PlaytestReport); else setErr(r.error || "Could not read that report.");
    });
    return () => { live = false; };
  }, [id, run.run_id]);
  const htmlUrl = projUrl(id, `/playtests/${enc(run.run_id)}/report.html`);
  const shots = report?.screenshots ?? [];
  return (
    <div className="flex min-w-0 flex-col gap-3" data-run={run.run_id}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2"><Verdict passed={run.passed} /><span className="font-mono text-sm text-ink-2">{run.run_id}</span></div>
        <div className="flex flex-wrap items-center gap-2">
          <SegmentedControl aria-label="Report format" size="sm">
            {(["summary", "html", "json"] as const).map((v) => (
              <SegmentedControlItem key={v} data-checked={view === v ? "" : undefined} aria-pressed={view === v} onClick={() => setView(v)} className="max-md:!h-11 max-md:px-3">{v === "summary" ? "Summary" : v.toUpperCase()}</SegmentedControlItem>
            ))}
          </SegmentedControl>
          <a href={htmlUrl} target="_blank" rel="noreferrer" className="inline-flex min-h-11 items-center gap-1.5 rounded-lg px-2 text-sm text-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-focus"><ExternalLink className="size-4" strokeWidth={1.5} aria-hidden />Open report</a>
        </div>
      </div>
      {err && <p role="alert" className="rounded-card bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">{err}</p>}
      {!report && !err && <div className="grid gap-2" aria-hidden><span className="h-14 rounded-card bg-track" /><span className="h-14 rounded-card bg-track" /></div>}
      {report && view === "html" && <iframe title="Playtest report" src={htmlUrl} sandbox="" className="h-[60dvh] w-full rounded-card border border-hairline bg-surface" />}
      {report && view === "json" && <pre className="max-h-[60dvh] overflow-auto rounded-card border border-hairline bg-board p-3 font-mono text-sm text-ink-2" data-json>{JSON.stringify(report, null, 2)}</pre>}
      {report && view === "summary" && (
        <>
          <p className="text-sm text-ink-2">
            {report.mode ? `${report.mode.replace(/_/g, " ")} · ` : ""}{report.duration_ms ? `${(report.duration_ms / 1000).toFixed(1)} s · ` : ""}build <span className="font-mono">{report.build_id ?? run.build_id}</span> · {stamp(run.at)}
          </p>
          {(report.checks?.length ?? 0) > 0 && (
            <ul className="flex flex-col gap-2" aria-label="Checks">
              {report.checks!.map((c, i) => (
                <li key={c.name + i} className="card-stock flex flex-wrap items-center justify-between gap-2 px-3 py-2.5">
                  <span className="inline-flex min-w-0 items-center gap-2 text-sm"><span className={c.passed ? "text-dot-green" : "text-dot-red"}>{c.passed ? <CircleCheck className="size-4" strokeWidth={1.5} aria-label="passed" /> : <CircleX className="size-4" strokeWidth={1.5} aria-label="failed" />}</span><span className="min-w-0 truncate">{c.name}</span></span>
                  <span className="max-w-full truncate font-mono text-sm text-ink-2">{c.error ? String(c.error) : c.value === undefined ? "" : JSON.stringify(c.value)}</span>
                </li>
              ))}
            </ul>
          )}
          <Lines title="Console errors" lines={report.console?.errors ?? []} tone="text-pill-pro-fg" />
          <Lines title="Console warnings" lines={report.console?.warnings ?? []} tone="text-pill-warn-fg" />
          {shots.length > 0 && (
            <section aria-label="Screenshots" className="flex flex-col gap-2">
              <h4 className="text-sm font-medium text-ink-2">Screenshots <span className="font-mono">{shots.length}</span></h4>
              <ul className="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-2" data-gallery>
                {shots.map((s) => (
                  <li key={s.file}>
                    <MorphingDialog transition={dialogSpring}>
                      <MorphingDialogTrigger className="block w-full overflow-hidden rounded-card border border-hairline bg-board text-left outline-none focus-visible:ring-2 focus-visible:ring-focus" aria-label={`Open screenshot ${s.name}`}>
                        <img src={shotUrl(id, run.run_id, s.file)} alt={s.name} loading="lazy" className="aspect-video w-full object-cover" />
                        <span className="block truncate px-2 py-1.5 text-sm text-ink-2">{s.name}</span>
                      </MorphingDialogTrigger>
                      <MorphingDialogContainer>
                        <MorphingDialogContent className="relative mx-3 flex max-h-[88dvh] w-full max-w-[960px] flex-col gap-3 rounded-dialog border border-hairline bg-surface p-4 text-ink shadow-lift">
                          <MorphingDialogTitle className="font-heading text-ui font-semibold">{s.name}</MorphingDialogTitle>
                          <img src={shotUrl(id, run.run_id, s.file)} alt={s.name} className="min-h-0 w-full flex-1 rounded-card object-contain" />
                          <MorphingDialogClose aria-label="Close screenshot" className="static inline-flex min-h-11 cursor-pointer items-center justify-center self-end rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none hover:bg-board focus-visible:ring-2 focus-visible:ring-focus">Close</MorphingDialogClose>
                        </MorphingDialogContent>
                      </MorphingDialogContainer>
                    </MorphingDialog>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </>
      )}
    </div>
  );
}

export function PlaytestPanel({ ship, studioOk, onCount }: { ship: Ship; studioOk: boolean; onCount?: (n: number) => void }) {
  const id = ship.active?.id;
  const [runs, setRuns] = useState<PlaytestRun[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [syncFirst, setSyncFirst] = useState(false);
  const [error, setError] = useState("");

  const load = useCallback(async (pick?: string) => {
    if (!id) return;
    const r = await call<{ runs: PlaytestRun[] }>(projUrl(id, "/playtests"));
    if (!r.ok) { setError(r.error || "Could not read the playtest reports."); setRuns((x) => x ?? []); return; }
    setError("");
    const list = r.runs ?? [];
    setRuns(list); onCount?.(list.length);
    setSelected((cur) => pick ?? (cur && list.some((x) => x.run_id === cur) ? cur : list[0]?.run_id ?? null));
  }, [id, onCount]);
  useEffect(() => { setRuns(null); setSelected(null); void load(); }, [load]);

  const run = async () => {
    if (!id) return;
    setRunning(true); setError("");
    const r = await call<{ run_id: string; passed: boolean }>(projUrl(id, "/playtest"), { sync_first: syncFirst });
    setRunning(false);
    if (!r.ok) { setError(r.error || "The playtest could not run."); return; }
    if (r.passed) toast.success("Playtest passed"); else toast.error("Playtest failed. The report shows why.");
    await load(r.run_id);
  };

  if (!ship.active) return null;
  const current = runs?.find((x) => x.run_id === selected) ?? null;
  return (
    <div className="flex flex-col gap-4" data-panel="playtests">
      <section aria-label="Run a playtest" className="card-stock flex flex-wrap items-center gap-3 p-4">
        <div className="min-w-0 flex-1">
          <h3 className="font-heading text-ui font-semibold">Playtest in Studio</h3>
          <p className="text-sm text-ink-2">{studioOk ? "Starts the game, runs the project's checks, takes screenshots and saves a report." : "Connect Roblox Studio to run a playtest."}</p>
        </div>
        <Button variant={syncFirst ? "secondary" : "outline"} size="lg" aria-pressed={syncFirst} onClick={() => setSyncFirst((v) => !v)} className="max-md:min-h-11">Send to Studio first</Button>
        <Button variant="default" size="lg" disabled={!studioOk || running} onClick={() => void run()} data-run-playtest className="max-md:min-h-11"><Play className="size-4" strokeWidth={1.5} aria-hidden />{running ? "Playing…" : "Run playtest"}</Button>
        {error && <p role="alert" className="w-full rounded-card bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">{error}</p>}
      </section>
      {runs === null ? (
        <div className="grid gap-2" aria-hidden><span className="h-14 rounded-card bg-track" /><span className="h-14 rounded-card bg-track" /></div>
      ) : runs.length === 0 ? (
        <p className="card-stock p-4 text-sm text-ink-2">No playtests yet. Run one and the report shows up here.</p>
      ) : (
        <div className="grid min-w-0 gap-4 lg:grid-cols-[240px_minmax(0,1fr)]">
          <ul className="flex flex-col gap-2" aria-label="Playtest runs">
            {runs.map((r) => (
              <li key={r.run_id}>
                <Button variant="ghost" onClick={() => setSelected(r.run_id)} aria-current={r.run_id === selected ? "true" : undefined} className={`h-auto min-h-14 w-full flex-col items-start gap-1 whitespace-normal rounded-card border px-3 py-2 text-left sm:h-auto ${r.run_id === selected ? "border-hairline bg-blue-soft" : "border-transparent hover:bg-track"}`}>
                  <span className="flex w-full items-center justify-between gap-2"><Verdict passed={r.passed} /><span className="font-mono text-sm tabular-nums text-ink-2">{r.checks} checks</span></span>
                  <span className="text-sm text-ink-2">{stamp(r.at)}{r.errors ? ` · ${r.errors} console error${r.errors === 1 ? "" : "s"}` : ""}</span>
                </Button>
              </li>
            ))}
          </ul>
          {current && id && <RunDetail id={id} run={current} />}
        </div>
      )}
    </div>
  );
}
