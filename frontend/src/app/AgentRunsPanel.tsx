import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { ChevronDown, ListTodo, Play, RotateCcw, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pill } from "@/components/kibo-ui/pill";
import { SlidingNumber } from "@/components/ui/sliding-number";
import { Expandable, ExpandableTrigger } from "@/components/ui/expandable";
import { call, enc } from "./ship/api";

export type PlanPhase = {
  id: string;
  title: string;
  goal?: string;
  files?: string[];
  status: string;
  summary?: string;
};

export type AgentRun = {
  run_id: string;
  chat_id?: string | null;
  project_id?: string | null;
  ask: string;
  mode: string;
  status: "queued" | "planning" | "running" | "paused" | "completed" | "failed" | "cancelled";
  phases_total?: number;
  phases_done?: number;
  current_phase?: string | null;
  step?: number;
  max_steps?: number;
  checkpoint_seq?: number;
  summary?: string | null;
  error?: string | null;
  quota_reset_in?: string | null;
  plan?: PlanPhase[];
};

const TONE: Record<string, string> = {
  completed: "bg-pill-active-bg text-pill-active-fg",
  done: "bg-pill-active-bg text-pill-active-fg",
  running: "bg-tag-sky-bg text-tag-sky-fg",
  planning: "bg-tag-sky-bg text-tag-sky-fg",
  queued: "bg-pill-inactive-bg text-pill-inactive-fg",
  paused: "bg-pill-warn-bg text-pill-warn-fg",
  failed: "bg-pill-pro-bg text-pill-pro-fg",
  cancelled: "bg-pill-inactive-bg text-pill-inactive-fg",
};

/** Compact Copilot run queue, plan-phase progress, and checkpoint resume strip (backed by /api/runs). */
export function AgentRunsPanel({
  chatId,
  projectId,
  composerText,
  onClearComposer,
  onRunFinished,
}: {
  chatId: string;
  projectId: string | null;
  composerText: string;
  onClearComposer: () => void;
  onRunFinished?: () => void;
}) {
  const [available, setAvailable] = useState(true);
  const [runs, setRuns] = useState<AgentRun[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<AgentRun | null>(null);
  const [hint, setHint] = useState("");
  const [busy, setBusy] = useState("");
  const prevActiveRef = useState<{ id: string | null }>({ id: null })[0];

  const refresh = useCallback(async () => {
    const r = await call<{ active_run_id?: string | null; runs?: AgentRun[] }>("/api/runs");
    if (r.httpStatus === 404) { setAvailable(false); return; }
    if (!r.ok) return;
    setAvailable(true);
    const list = r.runs ?? [];
    setRuns(list);
    if (prevActiveRef.id && !r.active_run_id) onRunFinished?.();
    prevActiveRef.id = r.active_run_id ?? null;
    setSelectedId((cur) => (cur && list.some((x) => x.run_id === cur) ? cur : r.active_run_id ?? list[0]?.run_id ?? null));
  }, [onRunFinished, prevActiveRef]);

  const loadDetail = useCallback(async (id: string | null) => {
    if (!id) { setDetail(null); return; }
    const r = await call<{ run?: AgentRun }>(`/api/runs/${enc(id)}`);
    if (r.ok && r.run) setDetail(r.run);
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => { void loadDetail(selectedId); }, [selectedId, loadDetail]);

  const hasActive = runs.some((r) => r.status === "queued" || r.status === "planning" || r.status === "running");
  useEffect(() => {
    if (!hasActive) return;
    const t = window.setInterval(() => {
      void refresh();
      if (selectedId) void loadDetail(selectedId);
    }, 2500);
    return () => window.clearInterval(t);
  }, [hasActive, refresh, loadDetail, selectedId]);

  if (!available) return null;

  const queueRun = async () => {
    const message = composerText.trim();
    if (!message) return;
    setBusy("queue");
    const r = await call<{ run_id?: string; run?: AgentRun }>("/api/runs", {
      message,
      chat_id: chatId || undefined,
      mode: "plan_execute",
      project_id: projectId || undefined,
    });
    setBusy("");
    if (!r.ok) { toast.error(r.error || "Could not queue run."); return; }
    onClearComposer();
    toast.success("Queued background plan run");
    if (r.run_id) setSelectedId(r.run_id);
    await refresh();
  };

  const resume = async (id: string) => {
    setBusy(`resume:${id}`);
    const r = await call(`/api/runs/${enc(id)}/resume`, hint.trim() ? { hint: hint.trim() } : {});
    setBusy("");
    if (!r.ok) { toast.error(r.error || "Could not resume run."); return; }
    setHint("");
    toast.success("Run resumed");
    await refresh();
    await loadDetail(id);
  };

  const cancel = async (id: string) => {
    setBusy(`cancel:${id}`);
    const r = await call(`/api/runs/${enc(id)}/cancel`, {});
    setBusy("");
    if (!r.ok) { toast.error(r.error || "Could not cancel run."); return; }
    toast("Run cancelled");
    await refresh();
    await loadDetail(id);
  };

  const active = detail ?? runs.find((x) => x.run_id === selectedId) ?? runs[0] ?? null;

  return (
    <section aria-label="Agent run queue" className="border-t border-hairline bg-board/40 px-3 py-2" data-agent-runs>
      <Expandable expandDirection="vertical" expandBehavior="push">
        {({ isExpanded }) => (
          <>
            <div className="flex flex-wrap items-center justify-between gap-2">
              <ExpandableTrigger aria-label="Agent run queue and plan" className="flex min-h-11 flex-1 cursor-pointer items-center gap-2 text-left text-xs font-medium text-ink outline-none focus-visible:ring-2 focus-visible:ring-focus md:min-h-8">
                <ListTodo className="size-3.5 shrink-0 text-icon" strokeWidth={1.5} aria-hidden />
                <span>Runs</span>
                <Pill className="bg-surface px-1.5 py-0 text-xs text-ink-2">
                  <span className="font-mono tabular-nums"><SlidingNumber value={runs.length} /></span>
                </Pill>
                {active && (
                  <Pill className={`px-1.5 py-0 text-xs ${TONE[active.status] || TONE.queued}`}>
                    {active.status}
                  </Pill>
                )}
                {active && active.phases_total ? (
                  <span className="text-xs text-ink-2">
                    <span className="font-mono tabular-nums">{active.phases_done ?? 0}/{active.phases_total}</span> phases
                  </span>
                ) : null}
                <ChevronDown className={`ml-auto size-3.5 text-ink-2 motion-safe:transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`} strokeWidth={1.5} aria-hidden />
              </ExpandableTrigger>
              {composerText.trim() && (
                <Button variant="outline" size="xs" disabled={busy === "queue"} onClick={() => void queueRun()} className="max-md:min-h-11">
                  <Play className="size-3" strokeWidth={1.5} aria-hidden />Queue plan
                </Button>
              )}
            </div>

            {isExpanded && (
              <div className="mt-2 flex flex-col gap-2 border-t border-hairline pt-2" data-runs-expanded>
                {runs.length === 0 ? (
                  <p className="text-xs text-ink-2">Type a goal in the box below and click Queue plan to run multi-phase tasks in the background.</p>
                ) : (
                  <>
                    <div className="flex flex-wrap gap-1">
                      {runs.slice(0, 6).map((r) => (
                        <Button key={r.run_id} variant={r.run_id === active?.run_id ? "secondary" : "ghost"} size="xs" onClick={() => setSelectedId(r.run_id)} className="max-w-[160px] truncate max-md:min-h-11">
                          <span className="truncate">{r.ask || r.run_id}</span>
                        </Button>
                      ))}
                    </div>

                    {active && (
                      <div className="flex flex-col gap-2 rounded-sm border border-hairline bg-surface p-2.5 text-xs">
                        <div className="flex flex-wrap items-center justify-between gap-2">
                          <span className="font-medium text-ink">{active.ask}</span>
                          <div className="flex items-center gap-1.5">
                            {(active.status === "paused" || active.status === "failed") && (
                              <Button variant="default" size="xs" disabled={!!busy} onClick={() => void resume(active.run_id)} className="max-md:min-h-11">
                                <RotateCcw className="size-3" strokeWidth={1.5} aria-hidden />Resume
                              </Button>
                            )}
                            {(active.status === "queued" || active.status === "planning" || active.status === "running") && (
                              <Button variant="outline" size="xs" disabled={!!busy} onClick={() => void cancel(active.run_id)} className="max-md:min-h-11">
                                <Square className="size-3" strokeWidth={1.5} aria-hidden />Cancel
                              </Button>
                            )}
                          </div>
                        </div>

                        <div className="flex flex-wrap gap-x-3 gap-y-1 text-ink-2">
                          <span>step <span className="font-mono tabular-nums">{active.step ?? 0}/{active.max_steps ?? 0}</span></span>
                          {active.current_phase && <span>phase <span className="font-mono">{active.current_phase}</span></span>}
                          {active.quota_reset_in && <span className="text-pill-warn-fg">resets in <span className="font-mono">{active.quota_reset_in}</span></span>}
                        </div>

                        {active.error && <p role="alert" className="rounded-xs bg-pill-pro-bg px-2 py-1 text-pill-pro-fg">{active.error}</p>}

                        {active.plan && active.plan.length > 0 && (
                          <ul className="flex flex-col gap-1 border-t border-hairline pt-1.5" aria-label="Plan phases">
                            {active.plan.map((p) => (
                              <li key={p.id} className="flex flex-wrap items-center justify-between gap-2">
                                <span className="min-w-0 flex-1 truncate text-ink">{p.title}</span>
                                <Pill className={`px-1.5 py-0 text-xs ${TONE[p.status] || TONE.queued}`}>{p.status}</Pill>
                              </li>
                            ))}
                          </ul>
                        )}

                        {(active.status === "paused" || active.status === "failed") && (
                          <Input value={hint} onChange={(e) => setHint(e.target.value)} placeholder="Optional hint before resuming…" aria-label="Resume hint" className="text-xs" />
                        )}
                      </div>
                    )}
                  </>
                )}
              </div>
            )}
          </>
        )}
      </Expandable>
    </section>
  );
}
