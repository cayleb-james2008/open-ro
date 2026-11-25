import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import {
  AlertTriangle,
  AtSign,
  Check,
  CheckSquare,
  Copy,
  FileCode,
  FileDiff,
  Play,
  Plus,
  Save,
  ShieldCheck,
  Square,
  X,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Textarea } from "@/components/ui/textarea";
import { Pill } from "@/components/kibo-ui/pill";
import {
  type PendingProposal,
  type Place,
  type ScriptFileDiff,
} from "./data";
import {
  analyzePlaceLuau,
  tokenizeLuauLine,
  type LuauDiagnostic,
  type LuauTokenKind,
} from "./luauAudit";

const TOKEN_CLS: Record<LuauTokenKind, string> = {
  comment: "text-ink-3",
  string: "text-tag-mint-fg",
  keyword: "font-medium text-tag-research-fg",
  global: "text-tag-ops-fg",
  number: "text-tag-plan-fg",
  plain: "text-ink",
};

function shortName(path: string): string {
  return path.split(".").pop() || path;
}

export function CodeDiffWorkbench({
  place,
  selectedScriptPath,
  onSelectScriptPath,
  proposals,
  activeProposalId,
  onSelectProposalId,
  latestFileDiffs,
  serverDiagnostics,
  busy,
  onSaveScript,
  onRunPlaytest,
  onApplyProposal,
  onRejectProposal,
  onAttachContext,
}: {
  place: Place;
  selectedScriptPath: string | null;
  onSelectScriptPath: (path: string) => void;
  proposals: PendingProposal[];
  activeProposalId: string | null;
  onSelectProposalId: (id: string | null) => void;
  latestFileDiffs: ScriptFileDiff[];
  serverDiagnostics: LuauDiagnostic[];
  busy: boolean;
  onSaveScript: (path: string, source: string) => Promise<void> | void;
  onRunPlaytest: () => Promise<void> | void;
  onApplyProposal: (proposalId: string, selectedPaths: string[]) => Promise<void> | void;
  onRejectProposal: (proposalId: string) => Promise<void> | void;
  onAttachContext: (path: string) => void;
}) {
  const pendingProposals = useMemo(
    () => proposals.filter((p) => p.status === "pending"),
    [proposals],
  );
  const activeProposal = useMemo(
    () =>
      (activeProposalId ? proposals.find((p) => p.id === activeProposalId) : null) ??
      pendingProposals[0] ??
      null,
    [activeProposalId, pendingProposals, proposals],
  );

  const activeDiffs = useMemo<ScriptFileDiff[]>(
    () => (activeProposal && activeProposal.file_diffs.length > 0 ? activeProposal.file_diffs : latestFileDiffs),
    [activeProposal, latestFileDiffs],
  );

  const allPaths = useMemo(() => {
    const set = new Set<string>();
    for (const d of activeDiffs) set.add(d.path);
    for (const k of Object.keys(place.scripts || {})) set.add(k);
    return [...set];
  }, [activeDiffs, place.scripts]);

  const currentPath = useMemo(() => {
    if (selectedScriptPath && allPaths.includes(selectedScriptPath)) return selectedScriptPath;
    if (activeDiffs[0]?.path) return activeDiffs[0].path;
    return allPaths[0] ?? null;
  }, [activeDiffs, allPaths, selectedScriptPath]);

  const currentDiff = useMemo(
    () => (currentPath ? activeDiffs.find((d) => d.path === currentPath) ?? null : null),
    [activeDiffs, currentPath],
  );

  const [viewMode, setViewMode] = useState<"diff" | "edit">(() => (currentDiff ? "diff" : "edit"));
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [applying, setApplying] = useState(false);
  const [newScriptOpen, setNewScriptOpen] = useState(false);
  const [newScriptPath, setNewScriptPath] = useState("ServerScriptService.NewScript");
  const [checkedPaths, setCheckedPaths] = useState<Set<string>>(() => new Set());

  useEffect(() => {
    if (activeProposal) {
      setCheckedPaths(new Set(activeProposal.file_diffs.map((d) => d.path)));
    }
  }, [activeProposal]);

  useEffect(() => {
    if (currentDiff && activeProposal?.status === "pending") {
      setViewMode("diff");
    } else if (!currentDiff) {
      setViewMode("edit");
    }
  }, [activeProposal?.id, activeProposal?.status, currentDiff, currentPath]);

  const liveSource = currentPath ? place.scripts?.[currentPath] ?? currentDiff?.after_source ?? "" : "";

  useEffect(() => {
    setDraft(liveSource);
  }, [currentPath, liveSource]);

  const allDiagnostics = useMemo(() => {
    const virtualPlace: Place = currentPath
      ? { instances: place.instances || {}, scripts: { ...(place.scripts || {}), [currentPath]: draft } }
      : place;
    const local = analyzePlaceLuau(virtualPlace);
    const merged = [...local];
    for (const sd of serverDiagnostics) {
      if (!merged.some((m) => m.path === sd.path && m.line === sd.line && m.message === sd.message)) {
        merged.push(sd);
      }
    }
    if (activeProposal) {
      for (const pd of activeProposal.diagnostics) {
        if (!merged.some((m) => m.path === pd.path && m.line === pd.line && m.message === pd.message)) {
          merged.push(pd);
        }
      }
    }
    return merged;
  }, [activeProposal, currentPath, draft, place, serverDiagnostics]);

  const fileDiagnostics = useMemo(
    () => (currentPath ? allDiagnostics.filter((d) => d.path === currentPath) : []),
    [allDiagnostics, currentPath],
  );

  const dirty = Boolean(currentPath) && draft !== liveSource;
  const errCount = fileDiagnostics.filter((d) => d.severity === "error").length;
  const warnCount = fileDiagnostics.filter((d) => d.severity === "warning").length;

  const toggleCheckedPath = (p: string) => {
    setCheckedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(p)) next.delete(p);
      else next.add(p);
      return next;
    });
  };

  const handleSave = async () => {
    if (!currentPath || saving) return;
    setSaving(true);
    try {
      await onSaveScript(currentPath, draft);
    } finally {
      setSaving(false);
    }
  };

  const handleCreateNewScript = async () => {
    const clean = newScriptPath.trim();
    if (!clean || !clean.includes(".")) {
      toast.error("Use a valid Roblox path like ServerScriptService.Sprint");
      return;
    }
    const starter = `-- ${shortName(clean)}\nlocal Players = game:GetService("Players")\n\nPlayers.PlayerAdded:Connect(function(player)\n\tprint("[OpenRO] Ready for", player.Name)\nend)\n`;
    setSaving(true);
    try {
      await onSaveScript(clean, starter);
      onSelectScriptPath(clean);
      setNewScriptOpen(false);
      setViewMode("edit");
    } finally {
      setSaving(false);
    }
  };

  const handleCopy = async () => {
    if (!draft) return;
    try {
      await navigator.clipboard.writeText(draft);
      toast.success(`Copied ${currentPath ? shortName(currentPath) : "Luau"} to clipboard`);
    } catch {
      toast.error("Could not copy Luau source.");
    }
  };

  const handleAcceptProposal = async () => {
    if (!activeProposal || applying) return;
    const selected = [...checkedPaths];
    if (selected.length === 0 && activeProposal.file_diffs.length > 0) {
      toast.error("Select at least one file to apply.");
      return;
    }
    setApplying(true);
    try {
      await onApplyProposal(activeProposal.id, selected);
    } finally {
      setApplying(false);
    }
  };

  const handleRejectProposalClick = async () => {
    if (!activeProposal || applying) return;
    setApplying(true);
    try {
      await onRejectProposal(activeProposal.id);
    } finally {
      setApplying(false);
    }
  };

  const previewLines = useMemo(() => draft.replace(/\r\n/g, "\n").split("\n"), [draft]);

  return (
    <div data-code-diff-workbench className="flex h-full min-h-0 flex-col bg-surface">
      {/* Pending Proposal Review Banner */}
      {activeProposal && activeProposal.status === "pending" && (
        <div
          data-proposal-review-bar
          className="flex flex-wrap items-center justify-between gap-2 border-b border-blue/40 bg-blue-soft/35 px-3 py-2"
        >
          <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
            <Badge className="border-transparent bg-tag-plan-bg px-2 py-0.5 text-xs font-medium text-tag-plan-fg">
              Pending Proposal
            </Badge>
            <span className="truncate text-xs font-medium text-ink">{activeProposal.summary}</span>
            <Pill className="bg-surface px-2 py-0.5 font-mono text-xs tabular-nums text-ink-2">
              {checkedPaths.size}/{activeProposal.file_diffs.length} files selected
            </Pill>
            {pendingProposals.length > 1 && (
              <div className="flex items-center gap-1">
                {pendingProposals.slice(0, 3).map((p, idx) => (
                  <Button
                    key={p.id}
                    variant={p.id === activeProposal.id ? "default" : "outline"}
                    size="xs"
                    onClick={() => onSelectProposalId(p.id)}
                    className="max-md:min-h-11"
                  >
                    #{idx + 1}
                  </Button>
                ))}
              </div>
            )}
          </div>
          <div className="flex shrink-0 flex-wrap items-center gap-1.5">
            <Button
              variant="outline"
              size="xs"
              disabled={applying || busy}
              aria-label="Reject staged proposal"
              onClick={() => void handleRejectProposalClick()}
              className="bg-surface max-md:min-h-11"
            >
              <X className="size-3.5" strokeWidth={1.5} aria-hidden />
              Reject
            </Button>
            <Button
              variant="default"
              size="xs"
              disabled={applying || busy}
              aria-label="Accept and apply staged proposal to Studio"
              onClick={() => void handleAcceptProposal()}
              className="max-md:min-h-11"
            >
              <Check className="size-3.5" strokeWidth={1.75} aria-hidden />
              {applying ? "Applying…" : "Accept & Apply to Studio"}
            </Button>
          </div>
        </div>
      )}

      {/* File Tabs Bar */}
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-hairline bg-board/60 px-3 py-1.5">
        <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto">
          {allPaths.map((p) => {
            const diff = activeDiffs.find((d) => d.path === p);
            const active = p === currentPath;
            const inProposal = Boolean(activeProposal?.status === "pending" && activeProposal.file_diffs.some((d) => d.path === p));
            const checked = checkedPaths.has(p);
            return (
              <div key={p} className="inline-flex shrink-0 items-center gap-0.5">
                {inProposal && (
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    aria-label={`${checked ? "Exclude" : "Include"} ${p} in proposal apply`}
                    aria-pressed={checked}
                    onClick={() => toggleCheckedPath(p)}
                    className="text-ink-2 hover:text-ink max-md:size-11"
                  >
                    {checked ? (
                      <CheckSquare className="size-3.5 text-blue" strokeWidth={1.75} aria-hidden />
                    ) : (
                      <Square className="size-3.5" strokeWidth={1.5} aria-hidden />
                    )}
                  </Button>
                )}
                <Button
                  variant={active ? "secondary" : "ghost"}
                  size="xs"
                  aria-pressed={active}
                  onClick={() => onSelectScriptPath(p)}
                  className={`gap-1.5 font-mono text-xs max-md:min-h-11 ${active ? "bg-surface text-ink shadow-xs" : "text-ink-2"}`}
                >
                  <FileCode className="size-3 shrink-0" strokeWidth={1.5} aria-hidden />
                  <span>{shortName(p)}</span>
                  {diff && (
                    <span className="inline-flex items-center gap-1 tabular-nums">
                      <span className="text-pill-active-fg">+{diff.additions}</span>
                      {diff.deletions > 0 && <span className="text-pill-pro-fg">-{diff.deletions}</span>}
                    </span>
                  )}
                </Button>
              </div>
            );
          })}
          <Button
            variant="ghost"
            size="xs"
            aria-label="Create new Luau script"
            onClick={() => setNewScriptOpen((v) => !v)}
            className="shrink-0 text-ink-2 hover:text-ink max-md:min-h-11"
          >
            <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />
            New Script
          </Button>
        </div>

        <div className="flex shrink-0 flex-wrap items-center gap-1.5">
          {currentPath && (
            <>
              <SegmentedControl aria-label="Code workbench view mode" size="sm">
                <SegmentedControlItem
                  data-checked={viewMode === "diff" ? "" : undefined}
                  aria-pressed={viewMode === "diff"}
                  disabled={!currentDiff}
                  onClick={() => setViewMode("diff")}
                  className="gap-1 max-md:!h-11"
                >
                  <FileDiff className="size-3" strokeWidth={1.5} aria-hidden />
                  Diff
                </SegmentedControlItem>
                <SegmentedControlItem
                  data-checked={viewMode === "edit" ? "" : undefined}
                  aria-pressed={viewMode === "edit"}
                  onClick={() => setViewMode("edit")}
                  className="gap-1 max-md:!h-11"
                >
                  <FileCode className="size-3" strokeWidth={1.5} aria-hidden />
                  Edit Luau
                </SegmentedControlItem>
              </SegmentedControl>

              <Button
                variant="outline"
                size="xs"
                aria-label={`Attach ${currentPath} to Copilot context`}
                onClick={() => onAttachContext(currentPath)}
                className="bg-surface max-md:min-h-11"
              >
                <AtSign className="size-3" strokeWidth={1.5} aria-hidden />
                Context
              </Button>
              <Button
                variant="outline"
                size="xs"
                aria-label="Copy Luau source"
                onClick={() => void handleCopy()}
                className="bg-surface max-md:min-h-11"
              >
                <Copy className="size-3" strokeWidth={1.5} aria-hidden />
                Copy
              </Button>
              <Button
                variant={dirty ? "default" : "outline"}
                size="xs"
                disabled={!dirty || saving}
                aria-label="Save and check Luau script"
                onClick={() => void handleSave()}
                className={dirty ? "max-md:min-h-11" : "bg-surface max-md:min-h-11"}
              >
                <Save className="size-3" strokeWidth={1.5} aria-hidden />
                {saving ? "Saving…" : "Save & Check"}
              </Button>
              <Button
                variant="outline"
                size="xs"
                disabled={busy}
                aria-label="Run Studio playtest from Code Workbench"
                onClick={() => void onRunPlaytest()}
                className="bg-surface max-md:min-h-11"
              >
                <Play className="size-3" strokeWidth={1.5} aria-hidden />
                Playtest
              </Button>
            </>
          )}
        </div>
      </div>

      {/* Inline New Script Creator */}
      {newScriptOpen && (
        <div className="flex flex-wrap items-center gap-2 border-b border-hairline bg-board/40 px-3 py-2">
          <Input
            value={newScriptPath}
            onChange={(e) => setNewScriptPath(e.target.value)}
            aria-label="New Luau script DataModel path"
            placeholder="ServerScriptService.SprintController"
            className="h-8 max-w-sm bg-surface font-mono text-xs max-md:min-h-11"
          />
          <Button variant="default" size="xs" disabled={saving} onClick={() => void handleCreateNewScript()} className="max-md:min-h-11">
            Create Script
          </Button>
          <Button variant="ghost" size="xs" onClick={() => setNewScriptOpen(false)} className="max-md:min-h-11">
            Cancel
          </Button>
        </div>
      )}

      {/* Active Script Path & Diagnostics Header */}
      {currentPath && (
        <div className="flex flex-wrap items-center justify-between gap-2 border-b border-hairline px-3 py-1.5 text-xs">
          <div className="flex min-w-0 items-center gap-2">
            <code className="truncate font-mono text-ink">{currentPath}</code>
            {currentDiff && (
              <Badge className="border-transparent bg-board px-1.5 py-0.5 font-mono text-xs text-ink-2">
                {currentDiff.status}
              </Badge>
            )}
            {dirty && (
              <Badge className="border-transparent bg-tag-dev-bg px-1.5 py-0.5 text-xs text-tag-dev-fg">
                Unsaved
              </Badge>
            )}
          </div>
          <div className="flex items-center gap-1.5">
            {errCount === 0 && warnCount === 0 ? (
              <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg">
                <ShieldCheck className="size-3" strokeWidth={1.5} aria-hidden />
                Luau Clean
              </Pill>
            ) : (
              <Pill className="bg-pill-pro-bg px-2 py-0.5 font-mono text-xs text-pill-pro-fg">
                <AlertTriangle className="size-3" strokeWidth={1.5} aria-hidden />
                {errCount} err / {warnCount} warn
              </Pill>
            )}
          </div>
        </div>
      )}

      {/* Main Content Area: Unified Diff OR Luau Code Editor */}
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        {!currentPath ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 p-8 text-center">
            <FileCode className="size-7 text-icon" strokeWidth={1.5} aria-hidden />
            <h3 className="text-sm font-medium text-ink">No Luau scripts open yet</h3>
            <p className="max-w-md text-xs leading-relaxed text-ink-2">
              Select a script in the Left Explorer, click New Script above, or ask the OpenRO Copilot on the right to generate or refactor Luau with reviewable unified diffs.
            </p>
            <Button variant="outline" size="sm" onClick={() => setNewScriptOpen(true)} className="mt-1 max-md:min-h-11">
              <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />
              Create Luau Script
            </Button>
          </div>
        ) : viewMode === "diff" && currentDiff ? (
          <div data-unified-diff-viewer className="flex flex-col font-mono text-xs">
            {currentDiff.hunks.length === 0 ? (
              <div className="p-6 text-center text-ink-2">No line differences in {currentDiff.path}.</div>
            ) : (
              currentDiff.hunks.map((hunk, hi) => (
                <div key={`${currentDiff.path}-h-${hi}`} className="border-b border-hairline">
                  <div className="bg-board/70 px-3 py-1 text-ink-2">
                    @@ -{hunk.old_start} +{hunk.new_start} @@
                  </div>
                  <div className="divide-y divide-hairline/40">
                    {hunk.lines.map((line, li) => {
                      const rowCls =
                        line.kind === "add"
                          ? "bg-pill-active-bg/35 text-ink"
                          : line.kind === "remove"
                            ? "bg-pill-pro-bg/35 text-ink"
                            : "bg-surface text-ink-2";
                      const sign = line.kind === "add" ? "+" : line.kind === "remove" ? "-" : " ";
                      const signCls =
                        line.kind === "add"
                          ? "text-pill-active-fg font-semibold"
                          : line.kind === "remove"
                            ? "text-pill-pro-fg font-semibold"
                            : "text-ink-3";
                      return (
                        <div key={li} data-diff-line={line.kind} className={`flex items-start px-2 py-0.5 ${rowCls}`}>
                          <span className="w-9 shrink-0 select-none pr-2 text-right tabular-nums text-ink-3">
                            {line.old_line ?? ""}
                          </span>
                          <span className="w-9 shrink-0 select-none pr-2 text-right tabular-nums text-ink-3">
                            {line.new_line ?? ""}
                          </span>
                          <span className={`w-4 shrink-0 select-none ${signCls}`}>{sign}</span>
                          <span className="min-w-0 flex-1 whitespace-pre-wrap break-all">{line.text || " "}</span>
                        </div>
                      );
                    })}
                  </div>
                </div>
              ))
            )}
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col p-2.5">
            <span className="mb-1.5 text-xs font-medium text-ink-2">Luau Source Editor</span>
            <Textarea
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              aria-label={`Edit Luau source for ${currentPath}`}
              spellCheck={false}
              className="min-h-[280px] flex-1 resize-none bg-board/35 font-mono text-xs leading-relaxed"
            />
          </div>
        )}

        {/* Inline Luau Diagnostics Footer */}
        {fileDiagnostics.length > 0 && (
          <div className="border-t border-hairline bg-board/50 px-3 py-2">
            <ul className="flex flex-col gap-1 text-xs">
              {fileDiagnostics.map((d, idx) => (
                <li
                  key={`${d.path}-${d.line}-${idx}`}
                  className={d.severity === "error" ? "font-mono text-pill-pro-fg" : "font-mono text-pill-warn-fg"}
                >
                  L{d.line} [{d.code}] {d.message}
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </div>
  );
}
