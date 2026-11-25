import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { toast } from "sonner";
import { AlertTriangle, CheckCircle2, ChevronDown, FileLock2, GitCommitHorizontal, Play, Plus, RefreshCw, RotateCcw, Send, Square, Terminal, Users, X, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { Pill } from "@/components/kibo-ui/pill";
import { ProgressCircle } from "@/components/ProgressCircle/ProgressCircle";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { SlidingNumber } from "@/components/ui/sliding-number";
import { Expandable, ExpandableTrigger } from "@/components/ui/expandable";
import { MorphingDialog, MorphingDialogClose, MorphingDialogContainer, MorphingDialogContent, MorphingDialogDescription, MorphingDialogTitle, MorphingDialogTrigger } from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import { stamp } from "../ship/api";
import { AdaptiveAgentCards } from "../AdaptiveAgentCards";
import {
  createRun, getAgentTrace, getRun, killAgent, listRuns, postToBoard, restartAgent, resumeRun, stopRun, teamUrl,
  type AgentState, type BoardPost, type ChatMessage, type FileClaim, type QuotaState, type RunSummary, type Task, type TeamworkSnapshot, type TraceEntry,
} from "./api";

const DEFAULT_ROLES = ["researcher", "systems", "ui", "world", "art", "verifier"];
const RUN_TONE: Record<string, string> = {
  running: "bg-pill-active-bg text-pill-active-fg",
  completed: "bg-tag-sky-bg text-tag-sky-fg",
  stopped: "bg-pill-inactive-bg text-pill-inactive-fg",
  failed: "bg-pill-pro-bg text-pill-pro-fg",
};
const AGENT_TONE: Record<string, string> = {
  working: "bg-pill-active-bg text-pill-active-fg",
  waiting: "bg-tag-sky-bg text-tag-sky-fg",
  stopped: "bg-pill-inactive-bg text-pill-inactive-fg",
  killed: "bg-pill-pro-bg text-pill-pro-fg",
};
const TASK_TONE: Record<string, string> = {
  done: "bg-pill-active-bg text-pill-active-fg",
  in_progress: "bg-tag-sky-bg text-tag-sky-fg",
  awaiting_verification: "bg-pill-warn-bg text-pill-warn-fg",
  changes_requested: "bg-pill-warn-bg text-pill-warn-fg",
  failed: "bg-pill-pro-bg text-pill-pro-fg",
  cancelled: "bg-pill-inactive-bg text-pill-inactive-fg",
  pending: "bg-pill-inactive-bg text-pill-inactive-fg",
};

function NewRunDialog({ projectId, onCreated }: { projectId?: string | null; onCreated: (run: RunSummary) => void }) {
  const [open, setOpen] = useState(false);
  const [goal, setGoal] = useState("");
  const [roles, setRoles] = useState<string[]>(DEFAULT_ROLES);
  const [conc, setConc] = useState("3");
  const [busy, setBusy] = useState(false);

  const toggleRole = (r: string) => setRoles((prev) => (prev.includes(r) ? prev.filter((x) => x !== r) : [...prev, r]));
  const submit = async () => {
    if (!goal.trim()) return;
    setBusy(true);
    const maxC = Math.min(16, Math.max(1, Number(conc) || 3));
    const r = await createRun({ goal: goal.trim(), project_id: projectId || undefined, roles, max_concurrency: maxC });
    setBusy(false);
    if (!r.ok || !r.run) { toast.error(r.error || "Could not start the team run."); return; }
    toast.success("Team run started");
    setOpen(false); setGoal("");
    onCreated(r.run);
  };

  return (
    <MorphingDialog transition={dialogSpring} open={open} onOpenChange={setOpen}>
      <MorphingDialogTrigger thing="new-team-run" className="inline-flex min-h-11 cursor-pointer items-center justify-center gap-1.5 rounded-lg border border-primary bg-primary px-3 text-sm font-medium text-primary-foreground outline-none hover:bg-primary/90 focus-visible:ring-2 focus-visible:ring-focus">
        <Plus className="size-4" strokeWidth={1.5} aria-hidden />New team run
      </MorphingDialogTrigger>
      <MorphingDialogContainer>
        <MorphingDialogContent className="relative mx-3 flex w-full max-w-[480px] flex-col gap-4 rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift">
          <div>
            <MorphingDialogTitle className="font-heading text-title font-semibold">Start a team run</MorphingDialogTitle>
            <MorphingDialogDescription className="mt-1 text-sm text-ink-2">Coordinator assigns tasks across roles, guards file ownership, and verifies changes.</MorphingDialogDescription>
          </div>
          <label className="flex flex-col gap-1 text-sm font-medium text-ink-2">
            <span>Goal</span>
            <Input value={goal} onChange={(e) => setGoal(e.target.value)} placeholder="Build an anime card RNG game with roll, combat, and shop" />
          </label>
          <div className="flex flex-col gap-1.5">
            <span className="text-sm font-medium text-ink-2">Team roles (coordinator is always included)</span>
            <div className="flex flex-wrap gap-1.5">
              {DEFAULT_ROLES.map((r) => {
                const on = roles.includes(r);
                return (
                  <Button key={r} variant={on ? "default" : "outline"} size="sm" aria-pressed={on} onClick={() => toggleRole(r)} className="max-md:min-h-11">
                    {r}
                  </Button>
                );
              })}
            </div>
          </div>
          <label className="flex flex-col gap-1 text-sm font-medium text-ink-2">
            <span>Max parallel model calls (1-16)</span>
            <Input inputMode="numeric" value={conc} onChange={(e) => setConc(e.target.value)} className="w-28 font-mono" />
          </label>
          <div className="flex flex-wrap justify-end gap-2">
            <MorphingDialogClose aria-label="Cancel" className="static inline-flex min-h-11 cursor-pointer items-center rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none hover:bg-board focus-visible:ring-2 focus-visible:ring-focus">Cancel</MorphingDialogClose>
            <Button variant="default" size="lg" disabled={!goal.trim() || busy} onClick={() => void submit()} className="max-md:min-h-11">{busy ? "Starting…" : "Start team"}</Button>
          </div>
        </MorphingDialogContent>
      </MorphingDialogContainer>
    </MorphingDialog>
  );
}

function TraceDrawer({ runId, agent, liveTraces, onClose }: { runId: string; agent: AgentState; liveTraces: TraceEntry[]; onClose: () => void }) {
  const [trace, setTrace] = useState<TraceEntry[]>([]);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [loading, setLoading] = useState(true);
  const [err, setErr] = useState("");
  useEffect(() => {
    let live = true;
    setLoading(true); setErr("");
    void getAgentTrace(runId, agent.name).then((r) => {
      if (!live) return;
      setLoading(false);
      if (!r.ok) { setErr(r.error || "Could not load trace."); return; }
      setTrace(r.trace ?? []); setMessages(r.messages ?? []);
    });
    return () => { live = false; };
  }, [runId, agent.name]);

  const combined = useMemo(() => {
    const map = new Map<number, TraceEntry>();
    for (const t of trace) map.set(t.seq, t);
    for (const t of liveTraces) map.set(t.seq, t);
    return [...map.values()].sort((a, b) => a.seq - b.seq);
  }, [trace, liveTraces]);

  return (
    <section aria-label={`Trace for ${agent.name}`} className="card-stock flex flex-col gap-3 p-3.5" data-agent-trace={agent.name}>
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <Terminal className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
          <h4 className="truncate font-heading text-ui font-semibold">{agent.name} trace</h4>
          <Pill className="bg-tag-neutral-bg px-2 py-0.5 text-xs text-tag-neutral-fg"><span className="font-mono tabular-nums">{combined.length}</span> steps</Pill>
        </div>
        <Button variant="ghost" size="icon" aria-label="Close trace" onClick={onClose} className="max-md:size-11"><X className="size-4" strokeWidth={1.5} aria-hidden /></Button>
      </div>
      {err && <p role="alert" className="rounded-card bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">{err}</p>}
      {loading ? <p className="text-sm text-ink-2">Loading trace…</p> : combined.length === 0 ? (
        <p className="text-sm text-ink-2">No trace steps recorded for {agent.name} yet.</p>
      ) : (
        <ul className="flex max-h-60 flex-col gap-1.5 overflow-y-auto text-sm">
          {combined.map((t) => (
            <li key={t.seq} className="rounded-card border border-hairline bg-board/60 px-2.5 py-2">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="font-mono text-xs font-medium text-ink">#{t.seq} {t.kind}</span>
                {t.ts_ms && <span className="font-mono text-xs text-ink-2">{stamp(t.ts_ms)}</span>}
              </div>
              <p className="mt-1 break-words text-sm text-ink-2">{t.summary}</p>
            </li>
          ))}
        </ul>
      )}
      {messages.length > 0 && (
        <Expandable expandDirection="vertical" expandBehavior="push">
          {({ isExpanded }) => (
            <div className="border-t border-hairline pt-2">
              <ExpandableTrigger aria-label="Conversation history" className="flex min-h-11 w-full cursor-pointer items-center justify-between text-left text-sm font-medium text-ink-2 outline-none focus-visible:ring-2 focus-visible:ring-focus">
                <span>Chat messages ({messages.length})</span>
                <span>{isExpanded ? "Hide" : "Show"}</span>
              </ExpandableTrigger>
              {isExpanded && (
                <ul className="mt-2 flex max-h-52 flex-col gap-1.5 overflow-y-auto text-sm">
                  {messages.map((m, i) => (
                    <li key={i} className="rounded-card bg-board p-2"><span className="font-mono text-xs font-medium text-ink">{m.role}: </span><span className="break-words text-ink-2">{m.content}</span></li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </Expandable>
      )}
    </section>
  );
}

/** Center Teamwork mode workspace: runs, roster, live board + @mentions, tasks/claims, per-agent traces, and quota status. */
export function TeamworkPanel({ projectId }: { projectId?: string | null }) {
  const [available, setAvailable] = useState(true);
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [snap, setSnap] = useState<TeamworkSnapshot | null>(null);
  const [tab, setTab] = useState<"board" | "tasks" | "roster">("board");
  const [selectedAgent, setSelectedAgent] = useState<string | null>(null);
  const [traceLive, setTraceLive] = useState<Record<string, TraceEntry[]>>({});
  const [postText, setPostText] = useState("");
  const [busy, setBusy] = useState("");
  const [err, setErr] = useState("");
  const feedRef = useRef<HTMLDivElement>(null);

  const loadRuns = useCallback(async () => {
    const r = await listRuns(projectId);
    if (r.httpStatus === 404) { setAvailable(false); return; }
    if (!r.ok) { setErr(r.error || "Could not load team runs."); return; }
    setAvailable(true); setErr("");
    const list = r.runs ?? [];
    setRuns(list);
    setActiveId((cur) => (cur && list.some((x) => x.id === cur) ? cur : list[0]?.id ?? null));
  }, [projectId]);

  const loadRun = useCallback(async (id: string) => {
    const r = await getRun(id);
    if (!r.ok || !r.run) { setErr(r.error || "Could not read team run."); return; }
    setErr("");
    setSnap({ run: r.run, roster: r.roster ?? [], tasks: r.tasks ?? [], claims: r.claims ?? [], board: r.board ?? [] });
  }, []);

  useEffect(() => { void loadRuns(); }, [loadRuns]);
  useEffect(() => {
    if (!activeId) { setSnap(null); return; }
    setSelectedAgent(null); setTraceLive({});
    void loadRun(activeId);
    const es = new EventSource(teamUrl(activeId, "/stream"));
    const onStatus = (ev: MessageEvent) => {
      try {
        const d = JSON.parse(ev.data) as { status?: string; roster?: AgentState[]; tasks?: Task[]; quota?: QuotaState; studio_lease?: RunSummary["studio_lease"]; recent?: BoardPost[] };
        setSnap((prev) => prev ? {
          ...prev,
          run: { ...prev.run, status: (d.status as RunSummary["status"]) ?? prev.run.status, quota: d.quota ?? prev.run.quota, studio_lease: d.studio_lease ?? prev.run.studio_lease },
          roster: d.roster ?? prev.roster,
          tasks: d.tasks ?? prev.tasks,
          board: d.recent && d.recent.length > prev.board.length ? d.recent : prev.board,
        } : prev);
      } catch { /* ignore */ }
    };
    const onBoard = (ev: MessageEvent) => {
      try {
        const d = JSON.parse(ev.data) as { message?: BoardPost };
        if (!d.message) return;
        setSnap((prev) => prev && !prev.board.some((b) => b.seq === d.message!.seq) ? { ...prev, board: [...prev.board, d.message!] } : prev);
      } catch { /* ignore */ }
    };
    const onTask = (ev: MessageEvent) => {
      try {
        const d = JSON.parse(ev.data) as { task?: Task; claims?: FileClaim[] };
        if (!d.task) return;
        setSnap((prev) => {
          if (!prev) return prev;
          const tasks = prev.tasks.some((t) => t.id === d.task!.id) ? prev.tasks.map((t) => (t.id === d.task!.id ? d.task! : t)) : [...prev.tasks, d.task!];
          return { ...prev, tasks, claims: d.claims ?? prev.claims };
        });
      } catch { /* ignore */ }
    };
    const onRoster = (ev: MessageEvent) => {
      try {
        const d = JSON.parse(ev.data) as { agent?: AgentState };
        if (!d.agent) return;
        setSnap((prev) => prev ? { ...prev, roster: prev.roster.some((a) => a.name === d.agent!.name) ? prev.roster.map((a) => (a.name === d.agent!.name ? d.agent! : a)) : [...prev.roster, d.agent!] } : prev);
      } catch { /* ignore */ }
    };
    const onQuota = (ev: MessageEvent) => {
      try {
        const d = JSON.parse(ev.data) as { quota?: QuotaState };
        if (!d.quota) return;
        setSnap((prev) => prev ? { ...prev, run: { ...prev.run, quota: d.quota } } : prev);
      } catch { /* ignore */ }
    };
    const onRun = (ev: MessageEvent) => {
      try {
        const d = JSON.parse(ev.data) as { status?: RunSummary["status"]; studio_lease?: RunSummary["studio_lease"]; active_model_calls?: number };
        setSnap((prev) => prev ? { ...prev, run: { ...prev.run, status: d.status ?? prev.run.status, studio_lease: d.studio_lease !== undefined ? d.studio_lease : prev.run.studio_lease, active_model_calls: d.active_model_calls ?? prev.run.active_model_calls } } : prev);
      } catch { /* ignore */ }
    };
    const onTrace = (ev: MessageEvent) => {
      try {
        const d = JSON.parse(ev.data) as { agent?: string; entry?: TraceEntry };
        if (!d.agent || !d.entry) return;
        setTraceLive((prev) => ({ ...prev, [d.agent!]: [...(prev[d.agent!] ?? []), d.entry!] }));
      } catch { /* ignore */ }
    };
    es.addEventListener("status", onStatus);
    es.addEventListener("board", onBoard);
    es.addEventListener("task", onTask);
    es.addEventListener("roster", onRoster);
    es.addEventListener("quota", onQuota);
    es.addEventListener("run", onRun);
    es.addEventListener("trace", onTrace);
    return () => es.close();
  }, [activeId, loadRun]);

  useEffect(() => {
    const el = feedRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [snap?.board.length]);

  const sendPost = async (e: FormEvent) => {
    e.preventDefault();
    if (!activeId || !postText.trim()) return;
    setBusy("post");
    const r = await postToBoard(activeId, postText.trim());
    setBusy("");
    if (!r.ok) { toast.error(r.error || "Could not post message."); return; }
    setPostText("");
    if (r.message) setSnap((prev) => prev && !prev.board.some((b) => b.seq === r.message!.seq) ? { ...prev, board: [...prev.board, r.message!] } : prev);
  };

  const toggleRun = async () => {
    if (!snap) return;
    const stop = snap.run.status === "running";
    setBusy(stop ? "stop" : "resume");
    const r = stop ? await stopRun(snap.run.id) : await resumeRun(snap.run.id);
    setBusy("");
    if (!r.ok) { toast.error(r.error || "Could not update run."); return; }
    toast.success(stop ? "Team run stopped" : "Team run resumed");
    await loadRun(snap.run.id); await loadRuns();
  };

  const actAgent = async (name: string, kind: "kill" | "restart" | "reset") => {
    if (!snap) return;
    setBusy(`${kind}:${name}`);
    const r = kind === "kill" ? await killAgent(snap.run.id, name) : await restartAgent(snap.run.id, name, kind === "reset");
    setBusy("");
    if (!r.ok) { toast.error(r.error || `Could not ${kind} ${name}.`); return; }
    toast.success(`${name} ${kind === "kill" ? "stopped" : "restarted"}`);
    await loadRun(snap.run.id);
  };

  if (!available) {
    return <div className="card-stock m-1 p-6 text-sm text-ink-2">This OpenRO build does not have Teamwork mode yet.</div>;
  }

  const run = snap?.run;
  const roster = snap?.roster ?? [];
  const tasks = snap?.tasks ?? [];
  const claims = snap?.claims ?? [];
  const board = snap?.board ?? [];
  const traced = roster.find((a) => a.name === selectedAgent) ?? null;

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto md:overflow-hidden" data-panel="teamwork">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <Users className="size-4 flex-none text-icon" strokeWidth={1.5} aria-hidden />
          <h2 className="truncate font-heading text-title font-semibold">Teamwork</h2>
          {runs.length > 0 && (
            <div className="flex flex-wrap gap-1">
              {runs.slice(0, 4).map((r) => (
                <Button key={r.id} variant={r.id === activeId ? "secondary" : "ghost"} size="xs" onClick={() => setActiveId(r.id)} className="max-w-[180px] truncate font-mono max-md:min-h-11">
                  {r.id}
                </Button>
              ))}
            </div>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="ghost" size="icon" aria-label="Refresh team runs" onClick={() => void loadRuns()} className="max-md:size-11"><RefreshCw className="size-4" strokeWidth={1.5} aria-hidden /></Button>
          <NewRunDialog projectId={projectId} onCreated={(r) => { setActiveId(r.id); void loadRuns(); }} />
        </div>
      </div>

      {err && <p role="alert" className="rounded-card bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">{err}</p>}

      {!run ? (
        <div className="card-stock flex flex-col items-start gap-3 p-6" data-teamwork-empty>
          <h3 className="font-heading text-ui font-semibold">Build with a team of agents</h3>
          <p className="max-w-[58ch] text-sm text-ink-2">Coordinator splits goals across specialized roles with file locks and Studio verification.</p>
        </div>
      ) : (
        <>
          <section aria-label="Run status" className="card-stock flex flex-col gap-3 p-3.5">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <Pill className={`px-2 py-0.5 text-sm ${RUN_TONE[run.status] || RUN_TONE.stopped}`}>{run.status}</Pill>
                  <span className="font-mono text-sm text-ink-2">{run.model_id}</span>
                  <span className="inline-flex items-baseline gap-1 text-sm text-ink-2">
                    <span className="inline-flex items-baseline font-mono tabular-nums"><SlidingNumber value={run.active_model_calls ?? 0} />/{run.max_concurrency}</span> active
                  </span>
                  {run.studio_lease && (
                    <Pill className="bg-tag-research-bg px-2 py-0.5 font-mono text-xs text-tag-research-fg">
                      Studio: {run.studio_lease.holder_agent} ({run.studio_lease.operation})
                    </Pill>
                  )}
                </div>
                <p className="mt-1 truncate text-sm text-ink">{run.goal}</p>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                {run.status === "running" ? (
                  <Button variant="outline" size="sm" disabled={!!busy} onClick={() => void toggleRun()} className="active:opacity-80 max-md:min-h-11"><Square className="size-3.5" strokeWidth={1.5} aria-hidden />Stop run</Button>
                ) : run.status === "stopped" ? (
                  <Button variant="default" size="sm" disabled={!!busy} onClick={() => void toggleRun()} className="active:opacity-80 max-md:min-h-11"><Play className="size-3.5" strokeWidth={1.5} aria-hidden />Resume run</Button>
                ) : null}
              </div>
            </div>

            {/* Visual Live Agent Workflow Diagram */}
            {(() => {
              const coord = roster.find((a) => a.name === "coordinator");
              const verifier = roster.find((a) => a.name === "verifier" || a.role.toLowerCase().includes("verif"));
              const workers = roster.filter((a) => a !== coord && a !== verifier);
              const doneCount = tasks.filter((t) => t.status === "done").length;
              const pct = tasks.length > 0 ? Math.round((doneCount / tasks.length) * 100) : 0;
              const nodeDot = (a?: AgentState) => {
                if (!a) return "bg-ink-3";
                if (a.killed) return "bg-pill-pro-fg";
                if (a.state === "working") return "bg-dot-green";
                if (a.state === "waiting") return "bg-blue";
                return "bg-ink-3";
              };
              const nodeRing = (a?: AgentState) =>
                a && !a.killed && a.state === "working" ? "border-dot-green/60 bg-pill-active-bg/35" : "border-hairline bg-surface";
              return (
                <div className="flex flex-col gap-2 rounded-sm border border-hairline bg-board/40 p-2.5" data-swarm-workflow>
                  <div className="flex flex-wrap items-center gap-2">
                    <Button
                      variant="outline"
                      size="xs"
                      onClick={() => coord && setSelectedAgent((c) => (c === coord.name ? null : coord.name))}
                      className={`min-h-8 gap-1.5 px-2 text-xs active:opacity-80 max-md:min-h-11 ${nodeRing(coord)}`}
                    >
                      <span className={`size-2 rounded-full ${nodeDot(coord)}`} aria-hidden />
                      <span>Coordinator</span>
                    </Button>
                    <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1.5" aria-label="Active role nodes">
                      {workers.length === 0 ? (
                        <span className="rounded-sm border border-hairline bg-surface px-2 py-1 text-xs text-ink-2">Workers</span>
                      ) : (
                        workers.map((w) => (
                          <Button
                            key={w.name}
                            variant="outline"
                            size="xs"
                            onClick={() => setSelectedAgent((c) => (c === w.name ? null : w.name))}
                            className={`min-h-8 gap-1.5 px-2 text-xs active:opacity-80 max-md:min-h-11 ${nodeRing(w)}`}
                          >
                            <span className={`size-2 rounded-full ${nodeDot(w)}`} aria-hidden />
                            <span>@{w.name}</span>
                          </Button>
                        ))
                      )}
                    </div>
                    <Button
                      variant="outline"
                      size="xs"
                      onClick={() => verifier && setSelectedAgent((c) => (c === verifier.name ? null : verifier.name))}
                      className={`min-h-8 gap-1.5 px-2 text-xs active:opacity-80 max-md:min-h-11 ${nodeRing(verifier)}`}
                    >
                      <span className={`size-2 rounded-full ${nodeDot(verifier)}`} aria-hidden />
                      <span>{verifier ? `@${verifier.name}` : "Verifier"}</span>
                    </Button>
                  </div>

                  {tasks.length > 0 && (
                    <div className="flex items-center justify-between gap-2.5 border-t border-hairline pt-2 text-xs text-ink-2">
                      <span className="inline-flex items-center gap-2">
                        <ProgressCircle value={pct} radius={11} strokeWidth={2.5} className="size-6 flex-none">
                          <span className="font-mono text-xs tabular-nums text-ink">{doneCount}</span>
                        </ProgressCircle>
                        <span>
                          <span className="font-mono tabular-nums text-ink">{doneCount}/{tasks.length}</span> tasks done (<span className="font-mono tabular-nums text-ink">{pct}%</span>)
                        </span>
                      </span>
                      <span className="text-ink-3">Click any role node to inspect trace</span>
                    </div>
                  )}
                </div>
              );
            })()}

            {run.quota?.active && (
              <div role="alert" data-teamwork-quota className="flex w-full flex-wrap items-center justify-between gap-2 rounded-sm bg-pill-warn-bg px-3 py-2 text-sm text-pill-warn-fg">
                <span className="inline-flex items-center gap-1.5"><AlertTriangle className="size-4 flex-none" strokeWidth={1.5} aria-hidden />{run.quota.message || "Model quota reached. Scheduler is backing off."}</span>
                {run.quota.reset_in && <span className="text-xs font-medium">resets in <span className="font-mono">{run.quota.reset_in}</span></span>}
              </div>
            )}
          </section>

          <SegmentedControl aria-label="Teamwork view" size="sm" className="self-start">
            <SegmentedControlItem data-checked={tab === "board" ? "" : undefined} aria-pressed={tab === "board"} onClick={() => setTab("board")} className="max-md:!h-11">
              Board <span className="ml-1 font-mono text-xs">{board.length}</span>
            </SegmentedControlItem>
            <SegmentedControlItem data-checked={tab === "tasks" ? "" : undefined} aria-pressed={tab === "tasks"} onClick={() => setTab("tasks")} className="max-md:!h-11">
              Tasks &amp; Claims <span className="ml-1 font-mono text-xs">{tasks.length}</span>
            </SegmentedControlItem>
            <SegmentedControlItem data-checked={tab === "roster" ? "" : undefined} aria-pressed={tab === "roster"} onClick={() => setTab("roster")} className="max-md:!h-11">
              Roster <span className="ml-1 font-mono text-xs">{roster.length}</span>
            </SegmentedControlItem>
          </SegmentedControl>

          {traced && (
            <TraceDrawer runId={run.id} agent={traced} liveTraces={traceLive[traced.name] ?? []} onClose={() => setSelectedAgent(null)} />
          )}

          {tab === "board" && (
            <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-hidden">
              <div ref={feedRef} className="min-h-0 flex-1 overflow-y-auto pr-1" data-teamwork-board>
                {board.length === 0 ? (
                  <p className="card-stock p-4 text-sm text-ink-2">No board messages yet.</p>
                ) : (
                  <ul className="flex flex-col gap-2">
                    {board.map((m) => {
                      const headline = (m.text || "").replace(/^@\S+\s*(\[done\]\s*)?/i, "").split("\n")[0].trim();
                      return (
                        <li key={m.seq} className="card-stock flex flex-col gap-1.5 px-3 py-2.5">
                          <Expandable expandDirection="vertical" expandBehavior="push">
                            {({ isExpanded }) => (
                              <>
                                <ExpandableTrigger
                                  aria-label={`Post ${m.seq} from ${m.author}`}
                                  className="flex min-h-8 w-full cursor-pointer items-center justify-between gap-2 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-focus max-md:min-h-11"
                                >
                                  <div className="flex min-w-0 flex-1 items-center gap-1.5">
                                    <span className="shrink-0 font-mono text-xs text-ink-2">#{m.seq}</span>
                                    <span className="shrink-0 font-medium text-ink">@{m.author}</span>
                                    <Pill className="shrink-0 bg-tag-neutral-bg px-1.5 py-0 text-xs text-tag-neutral-fg">{m.kind}</Pill>
                                    {m.is_done && <Pill className="shrink-0 bg-pill-active-bg px-1.5 py-0 text-xs text-pill-active-fg">done</Pill>}
                                    <span className="min-w-0 flex-1 truncate text-ink-2">{headline || m.kind}</span>
                                  </div>
                                  <div className="flex shrink-0 items-center gap-1.5">
                                    {m.ts_ms && <span className="font-mono text-xs text-ink-3">{stamp(m.ts_ms)}</span>}
                                    <ChevronDown className={`size-3.5 text-ink-3 duration-(--dur-swap) ease-(--ease-house) motion-safe:transition-transform ${isExpanded ? "rotate-180" : ""}`} strokeWidth={1.5} aria-hidden />
                                  </div>
                                </ExpandableTrigger>
                                {isExpanded && (
                                  <p className="mt-1.5 whitespace-pre-wrap break-words border-t border-hairline pt-2 text-sm text-ink-2">
                                    {m.text}
                                  </p>
                                )}
                              </>
                            )}
                          </Expandable>
                          <AdaptiveAgentCards text={m.text} projectId={run.project_id} />
                        </li>
                      );
                    })}
                  </ul>
                )}
              </div>
              <div className="flex flex-col gap-1.5 border-t border-hairline pt-2">
                {roster.length > 0 && (
                  <div className="flex flex-wrap gap-1">
                    {roster.map((a) => (
                      <Button key={a.name} variant="ghost" size="xs" onClick={() => setPostText((t) => (t ? `${t} @${a.name} ` : `@${a.name} `))} className="font-mono active:opacity-80 max-md:min-h-11">
                        @{a.name}
                      </Button>
                    ))}
                  </div>
                )}
                <form onSubmit={(e) => void sendPost(e)} className="flex gap-2">
                  <InputGroup className="min-w-0 flex-1">
                    <InputGroupAddon align="inline-start"><span className="text-xs text-ink-2">human</span></InputGroupAddon>
                    <InputGroupInput aria-label="Post to team board" value={postText} onChange={(e) => setPostText(e.target.value)} placeholder="Message the team, e.g. @systems use 1000 pity" />
                  </InputGroup>
                  <Button type="submit" variant="default" size="lg" disabled={!postText.trim() || busy === "post"} className="active:opacity-80 max-md:min-h-11"><Send className="size-4" strokeWidth={1.5} aria-hidden />Post</Button>
                </form>
              </div>
            </div>
          )}

          {tab === "tasks" && (
            <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pb-2" data-teamwork-tasks>
              {tasks.length === 0 ? (
                <p className="card-stock p-4 text-sm text-ink-2">No tasks assigned yet.</p>
              ) : (
                <div className="card-stock overflow-x-auto" data-teamwork-tasks-table>
                  <table className="w-full border-collapse text-left text-sm">
                    <thead>
                      <tr className="border-b border-hairline bg-board/50 text-xs font-medium text-ink-2">
                        <th className="px-3 py-2">Status</th>
                        <th className="px-3 py-2">Task</th>
                        <th className="px-3 py-2">Owner</th>
                        <th className="px-3 py-2">Files</th>
                        <th className="px-3 py-2">Gate</th>
                      </tr>
                    </thead>
                    <tbody>
                      {tasks.map((t) => (
                        <tr key={t.id} className="border-b border-hairline/70 last:border-b-0">
                          <td className="px-3 py-2 align-top">
                            <Pill className={`px-2 py-0.5 text-xs ${TASK_TONE[t.status] || TASK_TONE.pending}`}>
                              {t.status.replace(/_/g, " ")}
                            </Pill>
                          </td>
                          <td className="max-w-[260px] px-3 py-2 align-top">
                            <div className="font-medium text-ink">{t.title}</div>
                            <span className="font-mono text-xs text-ink-3">{t.id}</span>
                          </td>
                          <td className="px-3 py-2 align-top">
                            <Pill className="bg-board px-2 py-0.5 text-xs text-ink">@{t.owner}</Pill>
                          </td>
                          <td className="px-3 py-2 align-top">
                            {t.claimed_files && t.claimed_files.length > 0 ? (
                              <div className="flex flex-wrap gap-1">
                                {t.claimed_files.map((f) => (
                                  <code key={f} className="rounded-sm bg-board px-1.5 py-0.5 font-mono text-xs text-ink-2">{f}</code>
                                ))}
                              </div>
                            ) : (
                              <span className="text-xs text-ink-3">-</span>
                            )}
                          </td>
                          <td className="px-3 py-2 align-top">
                            <div className="flex flex-wrap items-center gap-1.5">
                              {t.verify_report && (
                                <Pill className={`gap-1 px-2 py-0.5 text-xs ${t.verify_report.passed ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-pro-bg text-pill-pro-fg"}`}>
                                  {t.verify_report.passed ? <CheckCircle2 className="size-3" strokeWidth={1.5} aria-hidden /> : <XCircle className="size-3" strokeWidth={1.5} aria-hidden />}
                                  {t.verify_report.passed ? "Verified" : "Rejected"}
                                  {t.verify_report.console_errors ? ` (${t.verify_report.console_errors})` : ""}
                                </Pill>
                              )}
                              {t.commit_sha && (
                                <Pill className="bg-tag-mint-bg px-1.5 py-0.5 font-mono text-xs text-tag-mint-fg">
                                  <GitCommitHorizontal className="size-3" strokeWidth={1.5} aria-hidden />
                                  {t.commit_sha.slice(0, 7)}
                                </Pill>
                              )}
                              {!t.verify_report && !t.commit_sha && <span className="text-xs text-ink-3">Pending</span>}
                            </div>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
              {claims.length > 0 && (
                <section aria-label="Active file claims" className="card-stock flex flex-col gap-1.5 p-3">
                  <h4 className="inline-flex items-center gap-1.5 text-sm font-medium text-ink"><FileLock2 className="size-4 text-icon" strokeWidth={1.5} aria-hidden />Active file claims</h4>
                  <ul className="flex flex-col gap-1 text-sm">
                    {claims.map((c) => <li key={c.path} className="flex flex-wrap justify-between gap-2"><code className="font-mono text-xs text-ink">{c.path}</code><span className="font-mono text-xs text-ink-2">@{c.owner} ({c.task_id})</span></li>)}
                  </ul>
                </section>
              )}
            </div>
          )}

          {tab === "roster" && (
            <ul className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto pb-2" data-teamwork-roster>
              {roster.map((a) => {
                const key = a.killed ? "killed" : a.state;
                return (
                  <li key={a.name} className="card-stock flex flex-wrap items-center justify-between gap-3 p-3">
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="text-ui font-medium text-ink">@{a.name}</span>
                        <Pill className={`px-2 py-0.5 text-xs ${AGENT_TONE[key] || AGENT_TONE.stopped}`}>{key}</Pill>
                        <span className="inline-flex items-baseline gap-1 text-xs text-ink-2"><span className="font-mono tabular-nums"><SlidingNumber value={a.turns} /></span> turns</span>
                        {a.current_task_id && <span className="text-xs text-ink-2">task <span className="font-mono">{a.current_task_id}</span></span>}
                      </div>
                      {a.last_error && <p className="mt-1 truncate text-sm text-pill-pro-fg">{a.last_error}</p>}
                    </div>
                    <div className="flex flex-wrap gap-1.5">
                      <Button variant="outline" size="sm" onClick={() => setSelectedAgent((cur) => (cur === a.name ? null : a.name))} className="max-md:min-h-11"><Terminal className="size-3.5" strokeWidth={1.5} aria-hidden />Trace</Button>
                      {!a.killed && a.state !== "stopped" && (
                        <Button variant="outline" size="sm" disabled={!!busy} onClick={() => void actAgent(a.name, "kill")} className="max-md:min-h-11">Kill</Button>
                      )}
                      {(a.killed || a.state === "stopped") && (
                        <Button variant="outline" size="sm" disabled={!!busy} onClick={() => void actAgent(a.name, "restart")} className="max-md:min-h-11"><RotateCcw className="size-3.5" strokeWidth={1.5} aria-hidden />Restart</Button>
                      )}
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </>
      )}
    </div>
  );
}
