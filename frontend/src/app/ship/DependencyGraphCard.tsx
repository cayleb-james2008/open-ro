import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { AlertTriangle, Check, GitFork, RotateCw, Scissors } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import { call, projUrl, type DependencyGraphReport, type DependencyPruneResult } from "./api";

export function DependencyGraphCard({
  projectId,
  studioOk = true,
}: {
  projectId?: string | null;
  studioOk?: boolean;
}) {
  const [graph, setGraph] = useState<DependencyGraphReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [pruning, setPruning] = useState(false);

  const load = useCallback(async () => {
    if (!projectId && !studioOk) return;
    setBusy(true);
    const endpoint = projectId ? projUrl(projectId, "/dependencies") : "/api/studio/dependencies";
    const res = await call<DependencyGraphReport>(endpoint);
    setBusy(false);
    if (res.ok && res.summary) {
      setGraph(res as DependencyGraphReport);
    }
  }, [projectId, studioOk]);

  useEffect(() => {
    void load();
  }, [load]);

  const pruneOrFix = async () => {
    if (!projectId && !studioOk) return;
    setPruning(true);
    const endpoint = projectId ? projUrl(projectId, "/dependencies/prune") : "/api/studio/dependencies/prune";
    const res = await call<DependencyPruneResult>(endpoint, {});
    setPruning(false);
    if (!res.ok) {
      toast.error(res.error || "Could not prune or fix dependencies.");
      return;
    }
    if (res.after) setGraph(res.after);
    toast.success(`Cleaned: ${res.fixed_requires ?? 0} require(s), ${res.pruned_modules ?? 0} module(s), ${res.pruned_remotes ?? 0} remote(s)`);
  };

  const s = graph?.summary;
  const issues = (s?.broken_count ?? 0) + (s?.unused_count ?? 0) + (s?.orphan_count ?? 0);

  return (
    <section
      data-dependency-graph
      aria-label="Luau Module Dependency Graph and Remote Audit"
      className="card-stock flex flex-col gap-3 p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <GitFork className="size-4 text-blue" strokeWidth={1.5} aria-hidden />
            <h3 className="font-heading text-ui font-medium text-ink">Luau Module Dependency Graph &amp; Remote Audit</h3>
            {s && (
              <Pill className={s.healthy ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg" : "bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg"}>
                {s.healthy ? (
                  <>
                    <Check className="size-3" strokeWidth={2} aria-hidden />
                    <span>0 broken, 0 unused, 0 orphan</span>
                  </>
                ) : (
                  <span>{issues} issue(s)</span>
                )}
              </Pill>
            )}
          </div>
          <p className="mt-0.5 text-xs text-ink-2">
            Audits <span className="font-mono">require()</span> edges, unused ModuleScripts, and ReplicatedStorage remotes across your place.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" size="sm" loading={busy} onClick={() => void load()} data-refresh-dependencies className="max-md:min-h-11">
            <RotateCw className="size-3.5" strokeWidth={1.5} aria-hidden />
            Refresh
          </Button>
          <Button
            variant={issues > 0 ? "default" : "outline"}
            size="sm"
            loading={pruning}
            disabled={!s || issues === 0 || pruning}
            onClick={() => void pruneOrFix()}
            data-prune-dependencies
            className="max-md:min-h-11"
          >
            <Scissors className="size-3.5" strokeWidth={1.5} aria-hidden />
            {pruning ? "Pruning…" : "Prune / Fix Links"}
          </Button>
        </div>
      </div>

      {s && (
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 font-mono text-xs tabular-nums">
          <div className="rounded-sm border border-hairline bg-board/50 px-3 py-1.5">
            <div className="font-sans text-[11px] text-ink-2">Scripts</div>
            <div className="font-semibold text-ink">{s.scripts}</div>
          </div>
          <div className="rounded-sm border border-hairline bg-board/50 px-3 py-1.5">
            <div className="font-sans text-[11px] text-ink-2">Modules</div>
            <div className="font-semibold text-ink">{s.modules}</div>
          </div>
          <div className="rounded-sm border border-hairline bg-board/50 px-3 py-1.5">
            <div className="font-sans text-[11px] text-ink-2">Require Edges</div>
            <div className="font-semibold text-ink">{s.edges}</div>
          </div>
          <div className="rounded-sm border border-hairline bg-board/50 px-3 py-1.5">
            <div className="font-sans text-[11px] text-ink-2">Remotes</div>
            <div className="font-semibold text-ink">{s.remotes}</div>
          </div>
        </div>
      )}

      {graph && graph.broken_requires.length > 0 && (
        <ul className="flex flex-col gap-1.5 text-xs" data-dep-broken aria-label="Broken require links">
          {graph.broken_requires.map((b, i) => (
            <li key={`${b.from}:${b.line}:${i}`} className="flex items-start gap-2 rounded-sm border border-hairline bg-pill-pro-bg/30 px-3 py-1.5">
              <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-pill-pro-fg" strokeWidth={1.5} aria-hidden />
              <div className="min-w-0 flex-1">
                <div className="font-mono text-ink">{b.from}:{b.line} → require({b.raw_expr})</div>
                <div className="text-ink-2">
                  Target <span className="font-mono">{b.resolved_target || "unknown"}</span> missing
                  {b.suggested_target ? ` · Suggested: ${b.suggested_target}` : ""}
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}

      {graph && graph.unused_modules.length > 0 && (
        <ul className="flex flex-col gap-1.5 text-xs" data-dep-unused aria-label="Unused ModuleScripts">
          {graph.unused_modules.map((m) => (
            <li key={m.path} className="flex items-center justify-between gap-2 rounded-sm border border-hairline bg-board/40 px-3 py-1.5">
              <span className="font-mono text-ink">{m.rel_file || m.path}</span>
              <Pill className="bg-pill-warn-bg px-2 py-0.5 text-[11px] text-pill-warn-fg">Unused ModuleScript</Pill>
            </li>
          ))}
        </ul>
      )}

      {graph && graph.orphan_remotes.length > 0 && (
        <ul className="flex flex-col gap-1.5 text-xs" data-dep-orphans aria-label="Orphan ReplicatedStorage remotes">
          {graph.orphan_remotes.map((o, i) => (
            <li key={`${o.name}:${i}`} className="flex items-center justify-between gap-2 rounded-sm border border-hairline bg-board/40 px-3 py-1.5">
              <div className="min-w-0 flex-1">
                <span className="font-mono text-ink">{o.name}</span>{" "}
                <span className="text-ink-2">({o.script}:{o.line})</span>
              </div>
              <Pill className="bg-pill-warn-bg px-2 py-0.5 text-[11px] text-pill-warn-fg">{o.kind}</Pill>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
