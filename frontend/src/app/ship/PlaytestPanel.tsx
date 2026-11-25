import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Check, ChevronDown, CircleCheck, CircleX, Download, ExternalLink, Play, RotateCw, ShieldCheck, Wrench } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Expandable, ExpandableTrigger } from "@/components/ui/expandable";
import { MorphingDialog, MorphingDialogClose, MorphingDialogContainer, MorphingDialogContent, MorphingDialogTitle, MorphingDialogTrigger } from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import { call, enc, projUrl, stamp, type AssertionDiff, type AssertionFixApplyResult, type AssertionFixPlan, type AssertionItem, type CoreLoopClaim, type FixAllAssertionsResult, type PlaytestReport, type PlaytestRun, type StudyStatus, type UnknownItem } from "./api";
import { DependencyGraphCard } from "./DependencyGraphCard";
import { EconomySimulatorCard } from "./EconomySimulatorCard";
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

function RunDetail({ id, run, onAppliedFix }: { id: string; run: PlaytestRun; onAppliedFix?: (nextRunId: string, diff?: AssertionDiff) => Promise<void> | void }) {
  const [report, setReport] = useState<PlaytestReport | null>(null);
  const [err, setErr] = useState("");
  const [view, setView] = useState<View>("summary");
  const [fixPlan, setFixPlan] = useState<AssertionFixPlan | null>(null);
  const [planBusy, setPlanBusy] = useState(false);
  const [applyBusy, setApplyBusy] = useState(false);
  useEffect(() => {
    let live = true;
    setReport(null); setErr(""); setView("summary"); setFixPlan(null);
    void call<PlaytestReport>(projUrl(id, `/playtests/${enc(run.run_id)}`)).then((r) => {
      if (!live) return;
      if (r.ok) setReport(r as PlaytestReport); else setErr(r.error || "Could not read that report.");
    });
    return () => { live = false; };
  }, [id, run.run_id]);
  const draftFix = async (assertionId: string) => {
    setPlanBusy(true);
    const r = await call<{ plan?: AssertionFixPlan }>(projUrl(id, "/playtests/fix-plan"), { assertion_id: assertionId });
    setPlanBusy(false);
    if (!r.ok || !r.plan) {
      toast.error(r.error || "Could not draft Luau fix plan.");
      return;
    }
    setFixPlan(r.plan);
  };
  const applyFixAndRerun = async () => {
    if (!fixPlan) return;
    setApplyBusy(true);
    const r = await call<AssertionFixApplyResult>(projUrl(id, "/playtests/fix-apply"), {
      assertion_id: fixPlan.assertion_id,
    });
    setApplyBusy(false);
    if (!r.ok || !r.rerun) {
      toast.error(r.error || "Could not apply assertion fix.");
      return;
    }
    setFixPlan(null);
    toast.success(`Applied ${r.applied_files?.length ?? 0} Luau file(s) & re-ran assertions`);
    await onAppliedFix?.(r.rerun.run_id, r.rerun.assertion_diff);
  };
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
          {shots.length > 0 && (
            <section aria-label="Screenshot filmstrip" className="flex flex-col gap-2" data-shot-filmstrip>
              <div className="flex items-center justify-between gap-2">
                <h4 className="text-sm font-medium text-ink">Screenshots <span className="font-mono text-ink-2">{shots.length}</span></h4>
                <span className="font-mono text-xs text-ink-3">
                  {report.mode ? `${report.mode.replace(/_/g, " ")} ` : ""}
                  {report.duration_ms ? `${(report.duration_ms / 1000).toFixed(1)}s ` : ""}
                  {stamp(run.at)}
                </span>
              </div>
              <ul className="flex gap-2 overflow-x-auto pb-1" data-gallery>
                {shots.map((s, i) => (
                  <li key={s.file} className="w-48 shrink-0">
                    <MorphingDialog transition={dialogSpring}>
                      <MorphingDialogTrigger className="block w-full overflow-hidden rounded-sm border border-hairline bg-board text-left outline-none active:opacity-80 focus-visible:ring-2 focus-visible:ring-focus" aria-label={`Open screenshot ${s.name}`}>
                        <img src={shotUrl(id, run.run_id, s.file)} alt={s.name} loading={i === 0 ? "eager" : "lazy"} fetchPriority={i === 0 ? "high" : undefined} className="aspect-video w-full object-cover" />
                        <span className="block truncate px-2 py-1 text-xs text-ink-2">{s.name}</span>
                      </MorphingDialogTrigger>
                      <MorphingDialogContainer>
                        <MorphingDialogContent className="relative mx-3 flex max-h-[88dvh] w-full max-w-[960px] flex-col gap-3 rounded-dialog border border-hairline bg-surface p-4 text-ink shadow-lift">
                          <MorphingDialogTitle className="font-heading text-ui font-semibold">{s.name}</MorphingDialogTitle>
                          <img src={shotUrl(id, run.run_id, s.file)} alt={s.name} className="min-h-0 w-full flex-1 rounded-sm object-contain" />
                          <MorphingDialogClose aria-label="Close screenshot" className="static inline-flex min-h-11 cursor-pointer items-center justify-center self-end rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none hover:bg-board focus-visible:ring-2 focus-visible:ring-focus">Close</MorphingDialogClose>
                        </MorphingDialogContent>
                      </MorphingDialogContainer>
                    </MorphingDialog>
                  </li>
                ))}
              </ul>
            </section>
          )}

          {(report.assertions?.length ?? 0) > 0 && (
            <section className="card-stock flex flex-col gap-2.5 p-3" data-assertion-suite aria-label="Deterministic assertion suite">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="flex flex-wrap items-center gap-2">
                  <h4 className="text-sm font-medium text-ink">Deterministic Assertions</h4>
                  {report.assertion_diff && (
                    <span className="font-mono text-xs tabular-nums text-ink-2">
                      {report.assertion_diff.passed}/{report.assertion_diff.total} pass
                      {report.assertion_diff.fixed > 0 ? ` · +${report.assertion_diff.fixed} fixed` : ""}
                      {report.assertion_diff.regressed > 0 ? ` · -${report.assertion_diff.regressed} regressed` : ""}
                    </span>
                  )}
                </div>
                <Button
                  variant="outline"
                  size="xs"
                  data-export-assertions
                  onClick={() => {
                    const payload = JSON.stringify(
                      {
                        project: id,
                        run_id: report.run_id,
                        passed: report.passed,
                        assertions: report.assertions ?? [],
                        assertion_diff: report.assertion_diff ?? null,
                      },
                      null,
                      2,
                    );
                    const blob = new Blob([payload], { type: "application/json" });
                    const url = URL.createObjectURL(blob);
                    const a = document.createElement("a");
                    a.href = url;
                    a.download = `${id}-playtest-assertions.json`;
                    a.click();
                    setTimeout(() => URL.revokeObjectURL(url), 1000);
                  }}
                >
                  <Download className="size-3.5" strokeWidth={1.5} aria-hidden />
                  Export JSON
                </Button>
              </div>
              {report.assertion_diff?.rooms && report.assertion_diff.rooms.length > 0 && (
                <div className="flex flex-wrap gap-1.5" data-assertion-rooms>
                  {report.assertion_diff.rooms.map((rm) => (
                    <Pill
                      key={rm.room}
                      className={`gap-1 px-2 py-0.5 text-xs ${
                        rm.regressed > 0
                          ? "bg-pill-pro-bg text-pill-pro-fg"
                          : rm.passed === rm.total
                            ? "bg-pill-active-bg text-pill-active-fg"
                            : "bg-pill-warn-bg text-pill-warn-fg"
                      }`}
                    >
                      <span>{rm.room}</span>
                      <span className="font-mono tabular-nums">{rm.passed}/{rm.total}</span>
                      {rm.fixed > 0 && <span className="font-mono">+{rm.fixed}</span>}
                      {rm.regressed > 0 && <span className="font-mono">-{rm.regressed}</span>}
                    </Pill>
                  ))}
                </div>
              )}
              <ul className="grid gap-1.5 sm:grid-cols-2">
                {(report.assertion_diff?.items ?? report.assertions ?? []).map((a: AssertionItem) => (
                  <li key={a.id} className="flex items-center justify-between gap-2 rounded-sm border border-hairline bg-board/50 px-2.5 py-1.5 text-xs">
                    <div className="min-w-0">
                      <div className="truncate font-medium text-ink">{a.label}</div>
                      <div className="truncate text-ink-2">{a.room} · {a.detail}</div>
                    </div>
                    <div className="flex shrink-0 items-center gap-1.5">
                      {!a.passed && (
                        <Button
                          variant="outline"
                          size="xs"
                          disabled={planBusy || applyBusy}
                          data-draft-fix={a.id}
                          onClick={() => void draftFix(a.id)}
                        >
                          Fix Plan
                        </Button>
                      )}
                      <Pill
                        className={`shrink-0 px-2 py-0.5 text-xs ${
                          a.delta === "regressed" || !a.passed
                            ? "bg-pill-pro-bg text-pill-pro-fg"
                            : a.delta === "fixed"
                              ? "bg-tag-sky-bg text-tag-sky-fg"
                              : "bg-pill-active-bg text-pill-active-fg"
                        }`}
                      >
                        {a.delta === "fixed" ? "Fixed" : a.delta === "regressed" ? "Regressed" : a.passed ? "Pass" : "Fail"}
                      </Pill>
                    </div>
                  </li>
                ))}
              </ul>
              {fixPlan && (
                <div className="mt-1 flex flex-col gap-2 rounded-sm border border-hairline bg-surface p-3 text-xs" data-assertion-fix-plan>
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="min-w-0">
                      <div className="font-medium text-ink">{fixPlan.title}</div>
                      <div className="font-mono text-ink-2">{fixPlan.target_file}</div>
                    </div>
                    <div className="flex items-center gap-1.5">
                      <Button variant="ghost" size="xs" onClick={() => setFixPlan(null)}>Dismiss</Button>
                      <Button variant="default" size="xs" loading={applyBusy} onClick={() => void applyFixAndRerun()} data-apply-assertion-fix>
                        Apply &amp; Re-Run Assertion
                      </Button>
                    </div>
                  </div>
                  <p className="text-ink-2">{fixPlan.summary}</p>
                  <pre className="max-h-40 overflow-auto rounded border border-hairline bg-board/60 p-2 font-mono text-xs text-ink">{fixPlan.diff_preview}</pre>
                </div>
              )}
            </section>
          )}

          {(report.checks?.length ?? 0) > 0 && (
            <div className="card-stock overflow-x-auto" data-verification-matrix>
              <table className="w-full border-collapse text-left text-sm" aria-label="Verification matrix">
                <thead>
                  <tr className="border-b border-hairline bg-board/50 text-xs font-medium text-ink-2">
                    <th className="px-3 py-2">Check</th>
                    <th className="px-3 py-2">Loop</th>
                    <th className="px-3 py-2">Console</th>
                    <th className="px-3 py-2 text-right">Result</th>
                  </tr>
                </thead>
                <tbody>
                  {report.checks!.map((c, i) => (
                    <tr key={c.name + i} className="border-b border-hairline/70 last:border-b-0">
                      <td className="px-3 py-2 font-medium text-ink">{c.name}</td>
                      <td className="px-3 py-2 font-mono text-xs text-ink-2">{c.datamodel || (report.mode ? report.mode.replace(/_/g, " ") : "studio")}</td>
                      <td className="px-3 py-2 font-mono text-xs text-ink-2">
                        {c.error ? String(c.error) : c.value === undefined ? `${report.console?.errors?.length ?? 0} errors` : JSON.stringify(c.value)}
                      </td>
                      <td className="px-3 py-2 text-right">
                        <Pill className={`gap-1 px-2 py-0.5 text-xs ${c.passed ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-pro-bg text-pill-pro-fg"}`}>
                          {c.passed ? <CircleCheck className="size-3" strokeWidth={1.5} aria-label="passed" /> : <CircleX className="size-3" strokeWidth={1.5} aria-label="failed" />}
                          {c.passed ? "Passed" : "Failed"}
                        </Pill>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          <Lines title="Console errors" lines={report.console?.errors ?? []} tone="text-pill-pro-fg" />
          <Lines title="Console warnings" lines={report.console?.warnings ?? []} tone="text-pill-warn-fg" />
        </>
      )}
    </div>
  );
}

function ClaimChip({ label, claim }: { label: string; claim?: CoreLoopClaim }) {
  const status = claim?.status || "not tested";
  const tone =
    claim?.passed === true
      ? "bg-pill-active-bg text-pill-active-fg"
      : claim?.tested === true && claim?.passed === false
        ? "bg-pill-pro-bg text-pill-pro-fg"
        : "bg-track text-ink-2";
  return (
    <Pill className={`px-2 py-0.5 text-xs ${tone}`}>
      <span>{label}: {status}</span>
    </Pill>
  );
}

export function PlaytestPanel({ ship, studioOk, onCount }: { ship: Ship; studioOk: boolean; onCount?: (n: number) => void }) {
  const id = ship.active?.id;
  const [runs, setRuns] = useState<PlaytestRun[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [rerunBusy, setRerunBusy] = useState(false);
  const [fixAllBusy, setFixAllBusy] = useState(false);
  const [fixAllResult, setFixAllResult] = useState<FixAllAssertionsResult | null>(null);
  const [latestAssertionDiff, setLatestAssertionDiff] = useState<AssertionDiff | null>(null);
  const [syncFirst, setSyncFirst] = useState(false);
  const [error, setError] = useState("");
  const [study, setStudy] = useState<StudyStatus | null>(null);
  const [openUnknowns, setOpenUnknowns] = useState<UnknownItem[]>([]);

  const loadStudy = useCallback(async () => {
    if (!id) {
      setStudy(null);
      setOpenUnknowns([]);
      return;
    }
    const [st, unks] = await Promise.all([
      call<StudyStatus>(projUrl(id, "/study/status")),
      call<{ unknowns?: UnknownItem[] }>(projUrl(id, "/investigation/unknowns?status=open")),
    ]);
    setStudy(st.ok && Array.isArray(st.stages) ? (st as StudyStatus) : null);
    setOpenUnknowns(unks.ok && Array.isArray(unks.unknowns) ? unks.unknowns : []);
  }, [id]);

  const load = useCallback(async (pick?: string) => {
    if (!id) return;
    const r = await call<{ runs: PlaytestRun[] }>(projUrl(id, "/playtests"));
    if (!r.ok) { setError(r.error || "Could not read the playtest reports."); setRuns((x) => x ?? []); return; }
    setError("");
    const list = r.runs ?? [];
    setRuns(list); onCount?.(list.length);
    setSelected((cur) => pick ?? (cur && list.some((x) => x.run_id === cur) ? cur : list[0]?.run_id ?? null));
  }, [id, onCount]);
  useEffect(() => { setRuns(null); setSelected(null); setFixAllResult(null); setLatestAssertionDiff(null); void load(); void loadStudy(); }, [load, loadStudy]);

  const run = async () => {
    if (!id) return;
    setRunning(true); setError("");
    const r = await call<{ run_id: string; passed: boolean; assertion_diff?: AssertionDiff }>(projUrl(id, "/playtest"), { sync_first: syncFirst });
    setRunning(false);
    if (!r.ok) { setError(r.error || "The playtest could not run."); return; }
    if (r.assertion_diff) setLatestAssertionDiff(r.assertion_diff);
    if (r.passed) toast.success("Playtest passed"); else toast.error("Playtest failed. The report shows why.");
    await Promise.all([load(r.run_id), loadStudy()]);
  };

  const rerunAssertions = async () => {
    if (!id) return;
    setRerunBusy(true); setError("");
    const r = await call<{ run_id: string; passed: boolean; assertion_diff?: AssertionDiff }>(
      projUrl(id, "/playtests/rerun"),
      { sync_first: syncFirst },
    );
    setRerunBusy(false);
    if (!r.ok) { setError(r.error || "Assertion suite re-run failed."); return; }
    if (r.assertion_diff) setLatestAssertionDiff(r.assertion_diff);
    if (r.passed) toast.success("All deterministic assertions passed");
    else toast.error("Assertion suite found failures — see per-room diff");
    await Promise.all([load(r.run_id), loadStudy()]);
  };

  const fixAllAssertions = async () => {
    if (!id) return;
    setFixAllBusy(true); setError("");
    const r = await call<FixAllAssertionsResult>(
      projUrl(id, "/playtests/fix-all"),
      { offline: true, sync_first: syncFirst },
    );
    setFixAllBusy(false);
    if (!r.ok) { setError(r.error || "Batch fix & export check failed."); return; }
    setFixAllResult(r as FixAllAssertionsResult);
    if (r.rerun?.assertion_diff) setLatestAssertionDiff(r.rerun.assertion_diff);
    if (r.export_ready) toast.success("All failing checks fixed — place & Ship Pack ready to export");
    else toast.error("Fixes applied, but some readiness checks still need attention");
    await Promise.all([load(r.rerun?.run_id), loadStudy()]);
  };

  if (!ship.active) return null;
  const current = runs?.find((x) => x.run_id === selected) ?? null;
  const latestMeta = study?.checks?.last_playtest;
  const likenessPct = latestMeta?.visual_likeness?.best_pct;
  const openCount = study?.unknowns?.open_count ?? openUnknowns.length;

  return (
    <div className="flex flex-col gap-4" data-panel="playtests">
      <section aria-label="Run a playtest" className="card-stock flex flex-wrap items-center gap-3 p-4">
        <div className="min-w-0 flex-1">
          <h3 className="font-heading text-ui font-semibold">Playtest in Studio</h3>
          <p className="text-sm text-ink-2">{studioOk ? "Runs Studio checks and captures gameplay screenshots." : "Connect Roblox Studio to playtest."}</p>
        </div>
        <Button variant={syncFirst ? "secondary" : "outline"} size="lg" aria-pressed={syncFirst} onClick={() => setSyncFirst((v) => !v)} className="active:opacity-80 max-md:min-h-11">Send to Studio first</Button>
        {!studioOk && (
          <Button
            variant="outline"
            size="lg"
            disabled={running}
            onClick={async () => {
              setRunning(true); setError("");
              const l = await call<{ studio_connected?: boolean }>("/api/studio/launch-and-continue", {});
              if (!l.ok || !l.studio_connected) { setRunning(false); setError(l.error || "Could not launch Studio."); return; }
              await run();
            }}
            className="active:opacity-80 max-md:min-h-11"
          >
            <Play className="size-4" strokeWidth={1.5} aria-hidden />Launch Studio &amp; playtest
          </Button>
        )}
        <Button variant="outline" size="lg" loading={rerunBusy} disabled={running || rerunBusy || fixAllBusy} onClick={() => void rerunAssertions()} data-rerun-assertions className="active:opacity-80 max-md:min-h-11">
          <RotateCw className="size-4" strokeWidth={1.5} aria-hidden />
          {rerunBusy ? "Re-running…" : "Re-Run All Assertions"}
        </Button>
        <Button variant="outline" size="lg" loading={fixAllBusy} disabled={running || rerunBusy || fixAllBusy} onClick={() => void fixAllAssertions()} data-fix-all-assertions className="active:opacity-80 max-md:min-h-11">
          <Wrench className="size-4" strokeWidth={1.5} aria-hidden />
          {fixAllBusy ? "Fixing & Verifying…" : "Fix All Failing Checks"}
        </Button>
        <Button variant="default" size="lg" disabled={!studioOk || running || rerunBusy || fixAllBusy} onClick={() => void run()} data-run-playtest className="active:opacity-80 max-md:min-h-11"><Play className="size-4" strokeWidth={1.5} aria-hidden />{running ? "Playing…" : "Run playtest"}</Button>
        {latestAssertionDiff && (
          <span className="font-mono text-xs tabular-nums text-ink-2" data-rerun-summary>
            {latestAssertionDiff.passed}/{latestAssertionDiff.total} assertions
            {latestAssertionDiff.fixed > 0 ? ` (+${latestAssertionDiff.fixed} fixed)` : ""}
            {latestAssertionDiff.regressed > 0 ? ` (-${latestAssertionDiff.regressed} regressed)` : ""}
          </span>
        )}
        {error && <p role="alert" className="w-full rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">{error}</p>}
      </section>

      {fixAllResult && (
        <section
          aria-label="Export readiness summary"
          data-export-ready-strip
          className={`card-stock flex flex-wrap items-center justify-between gap-3 px-4 py-3 ${fixAllResult.export_ready ? "border-l-2 border-l-pill-active-fg" : ""}`}
        >
          <div className="min-w-0 flex-1 space-y-0.5">
            <div className="flex flex-wrap items-center gap-2">
              <Pill className={fixAllResult.export_ready ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg" : "bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg"}>
                {fixAllResult.export_ready ? "Export Ready (.rbxlx / .zip)" : "Needs Manual Review"}
              </Pill>
              <span className="font-mono text-xs tabular-nums text-ink-2">
                {fixAllResult.fixed_ids.length} fixed · {fixAllResult.ship_pack_integrity.entry_count} ZIP entries · {fixAllResult.ship_pack_integrity.crc_verified ? "CRC-32 verified" : "ZIP warning"}
              </span>
            </div>
            <p className="text-xs text-ink-2">
              {fixAllResult.applied_files.length > 0
                ? `Updated ${fixAllResult.applied_files.join(", ")}`
                : "No file writes needed — build & Ship Pack integrity verified."}
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <a
              href={fixAllResult.export_urls?.rbxlx || "/api/export/rbxlx"}
              download="openro-place.rbxlx"
              data-export-ready-rbxlx
              className="inline-flex min-h-9 items-center gap-1.5 rounded-pill border border-hairline bg-surface px-3 py-1.5 text-xs font-medium text-ink outline-none hover:bg-track active:opacity-80 focus-visible:ring-2 focus-visible:ring-focus max-md:min-h-11"
            >
              <Download className="size-3.5" strokeWidth={1.5} aria-hidden />Export .rbxlx
            </a>
            <a
              href={fixAllResult.export_urls?.ship_pack || "/api/export/bundle"}
              download="openro-ship-pack.zip"
              data-export-ready-zip
              className="inline-flex min-h-9 items-center gap-1.5 rounded-pill bg-ink px-3 py-1.5 text-xs font-medium text-surface outline-none hover:opacity-90 active:opacity-80 focus-visible:ring-2 focus-visible:ring-focus max-md:min-h-11"
            >
              <Download className="size-3.5" strokeWidth={1.5} aria-hidden />Download Ship Pack .zip
            </a>
          </div>
        </section>
      )}

      {study && (
        <div className="card-stock flex flex-wrap items-center justify-between gap-2 px-4 py-2.5" data-playtest-stepper>
          <div className="flex flex-wrap items-center gap-1.5" aria-label="Study build check ready stepper">
            {study.stages.map((st, idx) => {
              const active = study.stage === st.id;
              return (
                <span key={st.id} className="inline-flex items-center gap-1.5">
                  <Pill
                    className={`gap-1 px-2 py-0.5 text-xs ${
                      st.done
                        ? "bg-pill-active-bg text-pill-active-fg"
                        : active
                          ? "bg-tag-sky-bg text-tag-sky-fg"
                          : "bg-board text-ink-2"
                    }`}
                  >
                    {st.done && <Check className="size-3" strokeWidth={2} aria-hidden />}
                    {st.label}
                  </Pill>
                </span>
              );
            })}
          </div>
          <Pill className={study.clean_room ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg" : "bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg"}>
            <ShieldCheck className="size-3" strokeWidth={1.5} aria-hidden />
            {study.clean_room ? "Clean Room" : "Deep Mode"}
          </Pill>
        </div>
      )}

      {study && (
        <section aria-label="Checks and unknowns summary" data-checks-unknowns className="card-stock px-4 py-2.5">
          <Expandable expandDirection="vertical" expandBehavior="push">
            {({ isExpanded }) => (
              <>
                <ExpandableTrigger
                  aria-label="Toggle Checks and Unknowns"
                  className="flex min-h-11 w-full cursor-pointer flex-wrap items-center justify-between gap-2 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-focus"
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <ShieldCheck className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                    <span className="font-medium text-ink">Checks &amp; Unknowns</span>
                    <ClaimChip label="Move" claim={latestMeta?.core_loop?.move} />
                    <ClaimChip label="Act" claim={latestMeta?.core_loop?.act} />
                    <ClaimChip label="HUD" claim={latestMeta?.core_loop?.hud} />
                    {typeof likenessPct === "number" && (
                      <Pill className="bg-tag-sky-bg px-2 py-0.5 text-xs text-tag-sky-fg">
                        Likeness <span className="font-mono tabular-nums">{likenessPct}%</span>
                      </Pill>
                    )}
                    <Pill className={openCount > 0 ? "bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg" : "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg"}>
                      <span className="font-mono tabular-nums">{openCount}</span> open unknowns
                    </Pill>
                  </div>
                  <ChevronDown className={`size-4 shrink-0 text-ink-2 motion-safe:transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`} strokeWidth={1.5} aria-hidden />
                </ExpandableTrigger>

                {isExpanded && (
                  <div className="mt-3 flex flex-col gap-2.5 border-t border-hairline pt-3 text-sm" data-checks-unknowns-body>
                    <div className="flex flex-wrap gap-2 text-xs text-ink-2">
                      <span>Evidence: <span className="font-mono tabular-nums text-ink">{study.evidence?.total ?? 0}</span></span>
                      {study.mechanics?.found && (
                        <span>
                          Mechanics: <span className="font-mono tabular-nums text-ink">{study.mechanics.counts?.sourced ?? 0}</span> sourced,{" "}
                          <span className="font-mono tabular-nums text-ink">{study.mechanics.counts?.unsourced ?? 0}</span> unsourced
                        </span>
                      )}
                      {latestMeta?.visual_likeness?.best_reference && (
                        <span>Best ref match: <span className="font-mono text-ink">{latestMeta.visual_likeness.best_reference}</span></span>
                      )}
                      {(latestMeta?.blank_screenshots ?? 0) > 0 && (
                        <span className="text-pill-pro-fg">
                          Blank screenshots rejected: <span className="font-mono tabular-nums">{latestMeta?.blank_screenshots}</span>
                        </span>
                      )}
                    </div>

                    {openUnknowns.length > 0 ? (
                      <ul className="flex flex-col gap-1.5" aria-label="Open unknowns">
                        {openUnknowns.map((u) => (
                          <li key={u.id} className="flex flex-wrap items-center justify-between gap-2 rounded-sm border border-hairline bg-board/50 px-3 py-1.5 text-xs">
                            <span className="min-w-0 flex-1 truncate text-ink">{u.title}</span>
                            <Pill className="bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg">{u.severity || u.status}</Pill>
                          </li>
                        ))}
                      </ul>
                    ) : (
                      <p className="text-xs text-ink-2">No open unknowns recorded for this project.</p>
                    )}
                  </div>
                )}
              </>
            )}
          </Expandable>
        </section>
      )}
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
                  <span className="flex w-full items-center justify-between gap-2"><Verdict passed={r.passed} /><span className="text-sm text-ink-2"><span className="font-mono tabular-nums">{r.checks}</span> checks</span></span>
                  <span className="flex flex-wrap gap-x-2 text-sm text-ink-2"><span>{stamp(r.at)}</span>{r.errors ? <span><span className="font-mono tabular-nums">{r.errors}</span> console error{r.errors === 1 ? "" : "s"}</span> : null}</span>
                </Button>
              </li>
            ))}
          </ul>
          {current && id && (
            <RunDetail
              id={id}
              run={current}
              onAppliedFix={async (nextRunId, diff) => {
                if (diff) setLatestAssertionDiff(diff);
                await Promise.all([load(nextRunId), loadStudy()]);
              }}
            />
          )}
        </div>
      )}
      <DependencyGraphCard projectId={id} studioOk={studioOk} />
      <EconomySimulatorCard projectId={id} studioOk={studioOk} />
    </div>
  );
}
