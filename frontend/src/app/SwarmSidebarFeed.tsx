import { useEffect, useState } from "react";
import { ChevronDown, Play, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import { Expandable, ExpandableTrigger } from "@/components/ui/expandable";
import { resumeRun, stopRun, teamUrl, type AgentState, type BoardPost, type QuotaState, type RunSummary, type Task, type TeamworkSnapshot } from "./teamwork/api";

const DOT: Record<string, string> = {
  working: "bg-dot-green",
  waiting: "bg-blue",
  stopped: "bg-ink-3",
  killed: "bg-pill-pro-fg",
};

function shortTitle(m: BoardPost): string {
  const first = (m.text || "").replace(/^@\S+\s*(\[done\]\s*)?/i, "").split("\n")[0].trim();
  return first.length > 68 ? `${first.slice(0, 67)}…` : first || m.kind;
}

/** Live Swarm status strip + compact expandable activity rows inside the right sidebar while Studio runs in center. */
export function SwarmSidebarFeed({
  runId,
  initial,
  onOpenFullSwarm,
}: {
  runId: string;
  initial: TeamworkSnapshot | null;
  onOpenFullSwarm: () => void;
}) {
  const [snap, setSnap] = useState<TeamworkSnapshot | null>(initial);
  const [busy, setBusy] = useState(false);

  useEffect(() => { setSnap(initial); }, [initial]);

  useEffect(() => {
    if (!runId) return;
    const es = new EventSource(teamUrl(runId, "/stream"));
    es.addEventListener("status", (ev) => {
      try {
        const d = JSON.parse((ev as MessageEvent).data) as { status?: RunSummary["status"]; roster?: AgentState[]; tasks?: Task[]; quota?: QuotaState; recent?: BoardPost[] };
        setSnap((prev) => prev ? {
          ...prev,
          run: { ...prev.run, status: d.status ?? prev.run.status, quota: d.quota ?? prev.run.quota },
          roster: d.roster ?? prev.roster,
          tasks: d.tasks ?? prev.tasks,
          board: d.recent && d.recent.length > prev.board.length ? d.recent : prev.board,
        } : prev);
      } catch { /* ignore */ }
    });
    es.addEventListener("board", (ev) => {
      try {
        const d = JSON.parse((ev as MessageEvent).data) as { message?: BoardPost };
        if (!d.message) return;
        setSnap((prev) => prev && !prev.board.some((b) => b.seq === d.message!.seq) ? { ...prev, board: [...prev.board, d.message!] } : prev);
      } catch { /* ignore */ }
    });
    es.addEventListener("roster", (ev) => {
      try {
        const d = JSON.parse((ev as MessageEvent).data) as { agent?: AgentState };
        if (!d.agent) return;
        setSnap((prev) => prev ? { ...prev, roster: prev.roster.map((a) => (a.name === d.agent!.name ? d.agent! : a)) } : prev);
      } catch { /* ignore */ }
    });
    es.addEventListener("run", (ev) => {
      try {
        const d = JSON.parse((ev as MessageEvent).data) as { status?: RunSummary["status"] };
        if (d.status) setSnap((prev) => prev ? { ...prev, run: { ...prev.run, status: d.status! } } : prev);
      } catch { /* ignore */ }
    });
    return () => es.close();
  }, [runId]);

  if (!snap) return null;
  const { run, roster, board } = snap;
  const toggle = async () => {
    setBusy(true);
    const r = run.status === "running" ? await stopRun(run.id) : await resumeRun(run.id);
    setBusy(false);
    if (r.ok && r.run) setSnap((p) => (p ? { ...p, run: { ...p.run, status: r.run!.status } } : p));
  };

  return (
    <div className="flex flex-col gap-2.5" data-swarm-sidebar>
      <div className="flex flex-col gap-2 overflow-hidden rounded-card border border-hairline bg-board/50 p-2.5">
        <div className="flex min-w-0 items-center justify-between gap-2">
          <Button variant="ghost" size="xs" onClick={onOpenFullSwarm} className="min-w-0 flex-1 justify-start overflow-hidden px-1 text-left text-xs font-medium text-ink">
            <span className="min-w-0 flex-1 truncate">{run.goal}</span>
          </Button>
          {run.status === "running" ? (
            <Button variant="outline" size="xs" disabled={busy} onClick={() => void toggle()} className="shrink-0 max-md:min-h-11">
              <Square className="size-3" strokeWidth={1.5} aria-hidden />Stop
            </Button>
          ) : run.status === "stopped" ? (
            <Button variant="default" size="xs" disabled={busy} onClick={() => void toggle()} className="shrink-0 max-md:min-h-11">
              <Play className="size-3" strokeWidth={1.5} aria-hidden />Resume
            </Button>
          ) : (
            <Pill className="shrink-0 bg-tag-sky-bg px-2 py-0.5 text-xs text-tag-sky-fg">{run.status}</Pill>
          )}
        </div>
        <div className="flex flex-wrap gap-1.5" aria-label="Teammates">
          {roster.map((a) => {
            const k = a.killed ? "killed" : a.state;
            return (
              <span key={a.name} className="inline-flex items-center gap-1.5 rounded-chip border border-hairline bg-surface px-2 py-0.5 text-xs text-ink">
                <span className={`size-1.5 rounded-full ${DOT[k] || DOT.stopped}`} />
                <span>{a.name}</span>
              </span>
            );
          })}
        </div>
      </div>

      <ul className="flex flex-col gap-1.5" aria-label="Swarm activity">
        {board.map((m) => (
          <li key={m.seq} className="rounded-sm border border-hairline bg-board/35 px-2.5 py-1.5">
            <Expandable expandDirection="vertical" expandBehavior="push">
              {({ isExpanded }) => (
                <>
                  <ExpandableTrigger aria-label={`Activity ${m.seq} from ${m.author}`} className="flex min-h-8 w-full cursor-pointer items-center justify-between gap-2 text-left text-xs outline-none focus-visible:ring-2 focus-visible:ring-focus max-md:min-h-11">
                    <span className="min-w-0 flex-1 truncate text-ink">
                      <span className="font-medium text-ink-2">{m.author}</span> {shortTitle(m)}
                    </span>
                    <ChevronDown className={`size-3.5 shrink-0 text-ink-3 motion-safe:transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`} strokeWidth={1.5} aria-hidden />
                  </ExpandableTrigger>
                  {isExpanded && (
                    <p className="mt-1.5 whitespace-pre-wrap break-words border-t border-hairline pt-1.5 text-xs text-ink-2">
                      {m.text}
                    </p>
                  )}
                </>
              )}
            </Expandable>
          </li>
        ))}
      </ul>
    </div>
  );
}
