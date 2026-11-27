import { memo, useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { toast } from "sonner";
import {
  AlertTriangle,
  AtSign,
  Check,
  CheckSquare,
  Compass,
  Copy,
  FileCode,
  FileDiff,
  ListTree,
  Play,
  Plus,
  RefreshCw,
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
  buildWorkspaceSymbolIndex,
  resolveLuauDefinitionClient,
  tokenizeLuauLine,
  type LuauDiagnostic,
  type LuauTokenKind,
} from "./luauAudit";
import {
  fetchLuauDefinition,
  fetchLuauSymbols,
  runLuauTypecheck,
  type LuauContractMismatch,
  type LuauSymbol,
  type WorkspaceScriptEntry,
} from "./ship/api";

const TOKEN_CLS: Record<LuauTokenKind, string> = {
  comment: "text-ink-3",
  string: "text-tag-mint-fg",
  keyword: "font-medium text-tag-research-fg",
  global: "text-tag-ops-fg",
  number: "text-tag-plan-fg",
  plain: "text-ink",
};

const SYMBOL_KIND_META: Record<string, { label: string; cls: string }> = {
  export: { label: "exp", cls: "bg-tag-ops-bg text-tag-ops-fg" },
  function: { label: "fn", cls: "bg-tag-mint-bg text-tag-mint-fg" },
  type: { label: "type", cls: "bg-tag-research-bg text-tag-research-fg" },
  remote_event: { label: "RE", cls: "bg-tag-plan-bg text-tag-plan-fg" },
  remote_function: { label: "RF", cls: "bg-tag-plan-bg text-tag-plan-fg" },
};

const LINE_HEIGHT_PX = 20;

function shortName(path: string): string {
  if (path.includes("/")) return path.split("/").pop() || path;
  return path.split(".").pop() || path;
}

function getCursorLineCol(text: string, offset: number): { line: number; col: number } {
  const clamped = Math.max(0, Math.min(offset, text.length));
  const before = text.slice(0, clamped);
  const lines = before.split("\n");
  return { line: lines.length, col: (lines[lines.length - 1]?.length ?? 0) + 1 };
}

function getOffsetForLineCol(text: string, line: number, col = 1): number {
  const lines = text.split("\n");
  const row = Math.max(1, Math.min(line, lines.length));
  let offset = 0;
  for (let i = 0; i < row - 1; i++) {
    offset += lines[i].length + 1;
  }
  const lineLen = lines[row - 1]?.length ?? 0;
  const clampedCol = Math.max(1, Math.min(col, lineLen + 1));
  return offset + (clampedCol - 1);
}

function normalizePathMatch(a: string, b: string): boolean {
  return a === b || a.replace(/\\/g, "/") === b.replace(/\\/g, "/");
}

export const CodeDiffWorkbench = memo(function CodeDiffWorkbench({
  place,
  selectedScriptPath,
  onSelectScriptPath,
  proposals,
  activeProposalId,
  onSelectProposalId,
  latestFileDiffs,
  serverDiagnostics,
  busy,
  projectMode = false,
  projectId = null,
  scriptMeta,
  onSaveScript,
  onRunPlaytest,
  onApplyProposal,
  onRejectProposal,
  onAttachContext,
  onTypecheckDiagnostics,
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
  projectMode?: boolean;
  projectId?: string | null;
  scriptMeta?: Record<string, WorkspaceScriptEntry>;
  onSaveScript: (path: string, source: string) => Promise<void> | void;
  onRunPlaytest: () => Promise<void> | void;
  onApplyProposal: (proposalId: string, selectedPaths: string[]) => Promise<void> | void;
  onRejectProposal: (proposalId: string) => Promise<void> | void;
  onAttachContext: (path: string) => void;
  onTypecheckDiagnostics?: (diags: LuauDiagnostic[]) => void;
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
    if (selectedScriptPath && selectedScriptPath.trim()) set.add(selectedScriptPath);
    return [...set];
  }, [activeDiffs, place.scripts, selectedScriptPath]);

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
  const [checkingTypes, setCheckingTypes] = useState(false);
  const [newScriptOpen, setNewScriptOpen] = useState(false);
  const [newScriptPath, setNewScriptPath] = useState(() =>
    projectMode ? "src/server/NewScript.server.luau" : "ServerScriptService.NewScript",
  );
  const [checkedPaths, setCheckedPaths] = useState<Set<string>>(() => new Set());
  const [symbolsOpen, setSymbolsOpen] = useState(() =>
    typeof window !== "undefined" ? window.innerWidth >= 768 : true,
  );
  const [symbolFilter, setSymbolFilter] = useState("");
  const [cursorLine, setCursorLine] = useState(1);
  const [cursorCol, setCursorCol] = useState(1);
  const [inspectedLine, setInspectedLine] = useState<number | null>(null);
  const [pendingJump, setPendingJump] = useState<{ path: string; line: number; col: number } | null>(null);

  // Backend workspace symbol index & typecheck state
  const [backendSymbols, setBackendSymbols] = useState<LuauSymbol[] | null>(null);
  const [backendMismatches, setBackendMismatches] = useState<LuauContractMismatch[] | null>(null);
  const [backendDiagnostics, setBackendDiagnostics] = useState<LuauDiagnostic[] | null>(null);
  const [diagnosticsSource, setDiagnosticsSource] = useState<"luau-analyze+builtin" | "builtin">("builtin");
  const [luauAnalyzeAvailable, setLuauAnalyzeAvailable] = useState(false);

  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const overlayRef = useRef<HTMLPreElement>(null);
  const gutterRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setNewScriptPath((prev) => {
      if (projectMode && prev === "ServerScriptService.NewScript") return "src/server/NewScript.server.luau";
      if (!projectMode && prev === "src/server/NewScript.server.luau") return "ServerScriptService.NewScript";
      return prev;
    });
  }, [projectMode]);

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

  const liveSource = useMemo(
    () => (currentPath ? (place.scripts?.[currentPath] ?? currentDiff?.after_source ?? "").replace(/\r\n/g, "\n") : ""),
    [currentDiff?.after_source, currentPath, place.scripts],
  );

  useEffect(() => {
    setDraft(liveSource);
    setInspectedLine(null);
    setCursorLine(1);
    setCursorCol(1);
  }, [currentPath, liveSource]);

  const virtualScripts = useMemo(() => {
    const base = { ...(place.scripts || {}) };
    if (currentPath) base[currentPath] = draft;
    return base;
  }, [currentPath, draft, place.scripts]);

  const clientSymbolIndex = useMemo(
    () => buildWorkspaceSymbolIndex(virtualScripts, place.instances || {}),
    [place.instances, virtualScripts],
  );

  const scriptsFingerprint = useMemo(() => {
    const entries = Object.entries(place.scripts || {}).sort(([a], [b]) => a.localeCompare(b));
    return entries.map(([k, v]) => `${k}:${v.length}`).join("|");
  }, [place.scripts]);

  const applySymbolResponse = useCallback(
    (res: Awaited<ReturnType<typeof fetchLuauSymbols>>) => {
      if (!res.ok) return false;
      setBackendSymbols(Array.isArray(res.symbols) ? res.symbols : []);
      setBackendMismatches(Array.isArray(res.mismatches) ? res.mismatches : []);
      const mappedDiags: LuauDiagnostic[] = Array.isArray(res.diagnostics)
        ? res.diagnostics.map((d) => ({
            path: String(d.path || ""),
            line: typeof d.line === "number" && Number.isFinite(d.line) ? d.line : 1,
            severity: d.severity === "error" ? "error" : "warning",
            code: String(d.code || "LUAU_CHECK"),
            message: String(d.message || ""),
          }))
        : [];
      setBackendDiagnostics(mappedDiags);
      setLuauAnalyzeAvailable(Boolean(res.luau_analyze_available));
      setDiagnosticsSource(res.diagnostics_source === "luau-analyze+builtin" ? "luau-analyze+builtin" : "builtin");
      onTypecheckDiagnostics?.(mappedDiags);
      return true;
    },
    [onTypecheckDiagnostics],
  );

  useEffect(() => {
    if (!projectMode || !projectId) {
      setBackendSymbols(null);
      setBackendMismatches(null);
      setBackendDiagnostics(null);
      setDiagnosticsSource("builtin");
      setLuauAnalyzeAvailable(false);
      return;
    }
    let cancelled = false;
    void fetchLuauSymbols(projectId).then((res) => {
      if (cancelled) return;
      if (!applySymbolResponse(res)) {
        setBackendSymbols(null);
        setBackendMismatches(null);
        setBackendDiagnostics(null);
        setDiagnosticsSource("builtin");
        setLuauAnalyzeAvailable(false);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [applySymbolResponse, projectId, projectMode, scriptsFingerprint]);

  const dirty = Boolean(currentPath) && (!(currentPath in (place.scripts || {})) || draft !== liveSource);

  const allDiagnostics = useMemo(() => {
    const virtualPlace: Place = currentPath
      ? { instances: place.instances || {}, scripts: virtualScripts }
      : place;
    const local = analyzePlaceLuau(virtualPlace);
    const merged = [...local];
    const extra = [...serverDiagnostics, ...(backendDiagnostics || [])];
    for (const sd of extra) {
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
  }, [activeProposal, backendDiagnostics, currentPath, place, serverDiagnostics, virtualScripts]);

  const workspaceSymbols = useMemo(() => {
    if (backendSymbols && !dirty) return backendSymbols;
    if (backendSymbols && dirty && currentPath) {
      const others = backendSymbols.filter((s) => !normalizePathMatch(s.path, currentPath));
      const localEdited = clientSymbolIndex.symbols.filter((s) => normalizePathMatch(s.path, currentPath));
      return [...localEdited, ...others];
    }
    return clientSymbolIndex.symbols;
  }, [backendSymbols, clientSymbolIndex.symbols, currentPath, dirty]);

  const workspaceMismatches = useMemo(() => {
    if (backendMismatches && !dirty) return backendMismatches;
    if (backendMismatches && dirty && currentPath) {
      const others = backendMismatches.filter((m) => !normalizePathMatch(m.caller_path, currentPath));
      const localEdited = clientSymbolIndex.mismatches.filter((m) => normalizePathMatch(m.caller_path, currentPath));
      return [...localEdited, ...others];
    }
    return clientSymbolIndex.mismatches;
  }, [backendMismatches, clientSymbolIndex.mismatches, currentPath, dirty]);

  const matchesCurrentFile = useCallback(
    (p: string) => {
      if (!currentPath) return false;
      const svcPath = scriptMeta?.[currentPath]?.service_path;
      return normalizePathMatch(p, currentPath) || Boolean(svcPath && normalizePathMatch(p, svcPath));
    },
    [currentPath, scriptMeta],
  );

  const fileDiagnostics = useMemo(
    () => allDiagnostics.filter((d) => matchesCurrentFile(d.path)),
    [allDiagnostics, matchesCurrentFile],
  );

  const fileMismatches = useMemo(
    () => workspaceMismatches.filter((m) => matchesCurrentFile(m.caller_path)),
    [matchesCurrentFile, workspaceMismatches],
  );

  const diagnosticsByLine = useMemo(() => {
    const map = new Map<number, { diagnostics: LuauDiagnostic[]; mismatches: LuauContractMismatch[] }>();
    for (const d of fileDiagnostics) {
      const ln = Math.max(1, d.line || 1);
      const entry = map.get(ln) || { diagnostics: [], mismatches: [] };
      entry.diagnostics.push(d);
      map.set(ln, entry);
    }
    for (const m of fileMismatches) {
      const ln = Math.max(1, m.caller_line || 1);
      const entry = map.get(ln) || { diagnostics: [], mismatches: [] };
      entry.mismatches.push(m);
      map.set(ln, entry);
    }
    return map;
  }, [fileDiagnostics, fileMismatches]);

  const errCount = fileDiagnostics.filter((d) => d.severity === "error").length + fileMismatches.length;
  const warnCount = fileDiagnostics.filter((d) => d.severity === "warning").length;

  const resolveTargetPath = useCallback(
    (target: string): string => {
      if (allPaths.includes(target)) return target;
      const clean = target.replace(/\\/g, "/").replace(/^\.\//, "");
      const exact = allPaths.find((p) => p.replace(/\\/g, "/") === clean);
      if (exact) return exact;
      if (scriptMeta) {
        for (const [rel, meta] of Object.entries(scriptMeta)) {
          if (meta.service_path === target || meta.service_path === clean) return rel;
        }
      }
      return target;
    },
    [allPaths, scriptMeta],
  );

  const focusCursorAt = useCallback(
    (line: number, col = 1) => {
      const ta = textareaRef.current;
      if (!ta) return;
      const offset = getOffsetForLineCol(draft, line, col);
      ta.focus();
      ta.setSelectionRange(offset, offset);
      const targetTop = Math.max(0, (line - 4) * LINE_HEIGHT_PX);
      ta.scrollTop = targetTop;
      if (overlayRef.current) overlayRef.current.scrollTop = targetTop;
      if (gutterRef.current) gutterRef.current.scrollTop = targetTop;
      setCursorLine(line);
      setCursorCol(col);
    },
    [draft],
  );

  const jumpToLocation = useCallback(
    (targetPath: string, line: number, col = 1) => {
      const resolved = resolveTargetPath(targetPath);
      setViewMode("edit");
      if (currentPath && normalizePathMatch(resolved, currentPath)) {
        requestAnimationFrame(() => focusCursorAt(line, col));
      } else {
        setPendingJump({ path: resolved, line, col });
        onSelectScriptPath(resolved);
      }
    },
    [currentPath, focusCursorAt, onSelectScriptPath, resolveTargetPath],
  );

  useEffect(() => {
    if (!pendingJump || !currentPath) return;
    if (!normalizePathMatch(currentPath, pendingJump.path)) return;
    if (viewMode !== "edit") {
      setViewMode("edit");
      return;
    }
    const t = requestAnimationFrame(() => {
      focusCursorAt(pendingJump.line, pendingJump.col);
      setPendingJump(null);
    });
    return () => cancelAnimationFrame(t);
  }, [currentPath, draft, focusCursorAt, pendingJump, viewMode]);

  const syncScroll = useCallback(() => {
    const ta = textareaRef.current;
    if (!ta) return;
    if (overlayRef.current) {
      overlayRef.current.scrollTop = ta.scrollTop;
      overlayRef.current.scrollLeft = ta.scrollLeft;
    }
    if (gutterRef.current) {
      gutterRef.current.scrollTop = ta.scrollTop;
    }
  }, []);

  const syncCursor = useCallback(() => {
    const ta = textareaRef.current;
    if (!ta) return;
    const pos = getCursorLineCol(ta.value, ta.selectionStart ?? 0);
    setCursorLine(pos.line);
    setCursorCol(pos.col);
  }, []);

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

  const handleCheckTypes = async () => {
    if (checkingTypes) return;
    setCheckingTypes(true);
    try {
      if (projectMode && projectId) {
        if (dirty && currentPath) {
          await onSaveScript(currentPath, draft);
        }
        const res = await runLuauTypecheck(projectId);
        if (applySymbolResponse(res)) {
          const totalErrs =
            (res.diagnostics?.filter((d) => d.severity === "error").length ?? 0) +
            (res.mismatches?.length ?? 0);
          if (totalErrs > 0) {
            toast.error(`Typecheck found ${totalErrs} issue${totalErrs === 1 ? "" : "s"}`);
          } else {
            toast.success(`Typecheck passed (${res.symbols?.length ?? 0} symbols indexed)`);
          }
          return;
        }
      }
      const local = buildWorkspaceSymbolIndex(virtualScripts, place.instances || {});
      setBackendSymbols(local.symbols);
      setBackendMismatches(local.mismatches);
      const mapped: LuauDiagnostic[] = local.diagnostics.map((d) => ({
        path: d.path,
        line: d.line,
        severity: d.severity === "error" ? "error" : "warning",
        code: d.code,
        message: d.message,
      }));
      setBackendDiagnostics(mapped);
      setDiagnosticsSource("builtin");
      setLuauAnalyzeAvailable(false);
      onTypecheckDiagnostics?.(mapped);
      const total = mapped.filter((d) => d.severity === "error").length + local.mismatches.length;
      if (total > 0) toast.error(`Static prover found ${total} issue${total === 1 ? "" : "s"}`);
      else toast.success(`Static prover clean (${local.symbols.length} symbols)`);
    } finally {
      setCheckingTypes(false);
    }
  };

  const handleGoToDefinition = useCallback(async () => {
    if (!currentPath) return;
    const ta = textareaRef.current;
    const pos = ta ? getCursorLineCol(ta.value, ta.selectionStart ?? 0) : { line: cursorLine, col: cursorCol };
    if (projectMode && projectId && !dirty) {
      const res = await fetchLuauDefinition(projectId, currentPath, pos.line, pos.col);
      if (res.ok && res.target_path) {
        jumpToLocation(res.target_path, res.line || 1, res.col || 1);
        return;
      }
    }
    const localDef = resolveLuauDefinitionClient(virtualScripts, currentPath, pos.line, pos.col, place.instances || {});
    if (localDef.ok && localDef.target_path) {
      jumpToLocation(localDef.target_path, localDef.line || 1, localDef.col || 1);
    } else {
      toast.info("Place the cursor on a require(), module call, remote, or symbol to jump.");
    }
  }, [cursorCol, cursorLine, currentPath, dirty, jumpToLocation, place.instances, projectId, projectMode, virtualScripts]);

  const handleEditorKeyDown = (e: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "F12") {
      e.preventDefault();
      void handleGoToDefinition();
    }
  };

  const handleCreateNewScript = async () => {
    const clean = newScriptPath.trim().replace(/\\/g, "/").replace(/^\/+/, "");
    const valid = projectMode
      ? Boolean(clean && (clean.endsWith(".luau") || clean.endsWith(".lua")) && !clean.includes(".."))
      : Boolean(clean && clean.includes("."));
    if (!valid) {
      toast.error(
        projectMode
          ? "Use a project-relative Luau path like src/server/Sprint.server.luau"
          : "Use a valid Roblox path like ServerScriptService.Sprint",
      );
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

  const filteredSymbols = useMemo(() => {
    const q = symbolFilter.trim().toLowerCase();
    const list = q
      ? workspaceSymbols.filter(
          (s) => s.name.toLowerCase().includes(q) || s.path.toLowerCase().includes(q) || s.kind.toLowerCase().includes(q),
        )
      : workspaceSymbols;
    const inFile = list.filter((s) => matchesCurrentFile(s.path));
    const others = list.filter((s) => !matchesCurrentFile(s.path));
    return { inFile, others };
  }, [matchesCurrentFile, symbolFilter, workspaceSymbols]);

  const inspectedEntry = inspectedLine !== null ? diagnosticsByLine.get(inspectedLine) ?? null : null;

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
            <Pill className="bg-surface px-2 py-0.5 text-xs tabular-nums text-ink-2">
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
          <div className="flex w-full min-w-0 flex-wrap items-center gap-1.5">
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

      {/* File Tabs Bar + Action Controls */}
      <div className="flex flex-col gap-1.5 border-b border-hairline bg-board/60 px-3 py-1.5">
        <div className="flex w-full min-w-0 items-center gap-1 overflow-x-auto pb-0.5">
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

        <div className="flex w-full min-w-0 flex-wrap items-center gap-1.5">
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
                data-check-types-btn
                disabled={checkingTypes}
                aria-label="Check Luau types and cross-file contracts"
                onClick={() => void handleCheckTypes()}
                className="bg-surface max-md:min-h-11"
              >
                <RefreshCw className={`size-3 ${checkingTypes ? "animate-spin" : ""}`} strokeWidth={1.5} aria-hidden />
                {checkingTypes ? "Checking…" : "Check Types"}
              </Button>

              <Button
                variant="outline"
                size="xs"
                data-goto-definition-btn
                aria-label="Go to definition (F12)"
                onClick={() => void handleGoToDefinition()}
                className="bg-surface max-md:min-h-11"
              >
                <Compass className="size-3" strokeWidth={1.5} aria-hidden />
                Go to Definition
              </Button>

              <Button
                variant={symbolsOpen ? "secondary" : "outline"}
                size="xs"
                data-symbols-toggle
                aria-pressed={symbolsOpen}
                aria-label="Toggle Symbols & Exports outline"
                onClick={() => setSymbolsOpen((v) => !v)}
                className="bg-surface max-md:min-h-11"
              >
                <ListTree className="size-3" strokeWidth={1.5} aria-hidden />
                Symbols
              </Button>

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
            aria-label={projectMode ? "New project Luau file path" : "New Luau script DataModel path"}
            placeholder={projectMode ? "src/server/SprintController.server.luau" : "ServerScriptService.SprintController"}
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
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <code className="truncate font-mono text-ink">{currentPath}</code>
            {scriptMeta?.[currentPath]?.service_path && (
              <Badge className="hidden border-transparent bg-board px-1.5 py-0.5 font-mono text-xs text-ink-2 sm:inline-flex">
                {scriptMeta[currentPath].service_path}
              </Badge>
            )}
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
            <span className="tabular-nums text-ink-3">
              Ln {cursorLine}, Col {cursorCol}
            </span>
          </div>
          <div className="flex flex-wrap items-center gap-1.5">
            <Pill
              data-diagnostics-source={diagnosticsSource}
              className={
                luauAnalyzeAvailable || diagnosticsSource === "luau-analyze+builtin"
                  ? "bg-tag-ops-bg px-2 py-0.5 text-xs text-tag-ops-fg"
                  : "bg-board px-2 py-0.5 text-xs text-ink-2"
              }
            >
              {diagnosticsSource === "luau-analyze+builtin" ? "luau-analyze+builtin" : "Static Prover (builtin)"}
            </Pill>
            {errCount === 0 && warnCount === 0 ? (
              <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg">
                <ShieldCheck className="size-3" strokeWidth={1.5} aria-hidden />
                Luau Clean
              </Pill>
            ) : (
              <Pill className="bg-pill-pro-bg px-2 py-0.5 text-xs tabular-nums text-pill-pro-fg">
                <AlertTriangle className="size-3" strokeWidth={1.5} aria-hidden />
                {errCount} err / {warnCount} warn
              </Pill>
            )}
          </div>
        </div>
      )}

      {/* Main Content Area: Unified Diff OR Synchronized Code Editor + Collapsible Symbols & Exports Rail */}
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
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
          <div data-unified-diff-viewer className="flex min-h-0 flex-1 flex-col overflow-y-auto text-xs">
            {currentDiff.hunks.length === 0 ? (
              <div className="p-6 text-center text-ink-2">
                No line differences in <code className="font-mono">{currentDiff.path}</code>.
              </div>
            ) : (
              currentDiff.hunks.map((hunk, hi) => (
                <div key={`${currentDiff.path}-h-${hi}`} className="border-b border-hairline font-mono">
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
          <div className="flex min-h-0 flex-1 flex-col md:flex-row">
            {/* Synchronized Line-Numbered Code Editor + Highlight Overlay */}
            <div className="flex min-h-0 min-w-0 flex-1 flex-col p-2.5">
              <div className="mb-1.5 flex items-center justify-between gap-2 text-xs text-ink-2">
                <span className="font-medium">Luau Source Editor</span>
                <span className="hidden text-ink-3 sm:inline">F12 Go to Definition</span>
              </div>

              {/* Clickable Gutter Diagnostic / Contract Mismatch Tooltip Banner */}
              {inspectedEntry && inspectedLine !== null && (
                <div
                  data-gutter-tooltip
                  role="status"
                  className="mb-2 flex flex-wrap items-start justify-between gap-2 rounded-lg border border-hairline bg-board px-3 py-2 text-xs"
                >
                  <div className="flex min-w-0 flex-1 flex-col gap-1">
                    {inspectedEntry.mismatches.map((m, idx) => (
                      <div key={`mm-${idx}`} className="flex flex-wrap items-center gap-2 text-pill-pro-fg">
                        <Badge className="border-transparent bg-pill-pro-bg px-1.5 py-0.5 tabular-nums text-xs text-pill-pro-fg">
                          L{inspectedLine} {m.kind}
                        </Badge>
                        <span className="min-w-0 flex-1 text-ink">{m.message}</span>
                        <Button
                          variant="outline"
                          size="xs"
                          data-mismatch-jump-target
                          onClick={() => jumpToLocation(m.target_path, 1, 1)}
                          className="bg-surface font-mono max-md:min-h-11"
                        >
                          Open {shortName(m.target_path)}
                        </Button>
                      </div>
                    ))}
                    {inspectedEntry.diagnostics.map((d, idx) => (
                      <div key={`dg-${idx}`} className="flex flex-wrap items-center gap-2">
                        <Badge
                          className={`border-transparent px-1.5 py-0.5 tabular-nums text-xs ${
                            d.severity === "error"
                              ? "bg-pill-pro-bg text-pill-pro-fg"
                              : "bg-pill-warn-bg text-pill-warn-fg"
                          }`}
                        >
                          L{inspectedLine} {d.code}
                        </Badge>
                        <span className="min-w-0 flex-1 text-ink">{d.message}</span>
                      </div>
                    ))}
                  </div>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    aria-label="Close line diagnostic tooltip"
                    onClick={() => setInspectedLine(null)}
                    className="text-ink-2 hover:text-ink max-md:size-11"
                  >
                    <X className="size-3.5" strokeWidth={1.5} aria-hidden />
                  </Button>
                </div>
              )}

              <div className="relative flex min-h-[260px] min-w-0 flex-1 overflow-hidden rounded-lg border border-hairline bg-board/35">
                {/* Synchronized Line-Number & Diagnostic Gutter */}
                <div
                  ref={gutterRef}
                  aria-hidden="false"
                  className="w-16 shrink-0 select-none overflow-hidden border-r border-hairline bg-board/70 py-2.5 font-mono text-xs leading-5"
                >
                  {previewLines.map((_, idx) => {
                    const lineNo = idx + 1;
                    const entry = diagnosticsByLine.get(lineNo);
                    const hasMismatch = Boolean(entry && entry.mismatches.length > 0);
                    const hasError = hasMismatch || Boolean(entry?.diagnostics.some((d) => d.severity === "error"));
                    const hasWarn = !hasError && Boolean(entry && entry.diagnostics.length > 0);
                    const isCursor = lineNo === cursorLine;
                    const tooltipSummary = entry
                      ? [
                          ...entry.mismatches.map((m) => `[${m.kind}] ${m.message}`),
                          ...entry.diagnostics.map((d) => `[${d.code}] ${d.message}`),
                        ].join(" | ")
                      : "";
                    return (
                      <div
                        key={lineNo}
                        className={`flex h-5 items-center justify-between px-1.5 ${
                          hasError
                            ? "bg-pill-pro-bg/55 text-pill-pro-fg"
                            : hasWarn
                              ? "bg-pill-warn-bg/55 text-pill-warn-fg"
                              : isCursor
                                ? "bg-blue-soft/60 text-ink"
                                : "text-ink-3"
                        }`}
                      >
                        {entry ? (
                          <button
                            type="button"
                            data-gutter-marker={lineNo}
                            data-gutter-severity={hasError ? "error" : "warning"}
                            aria-label={`Line ${lineNo}: ${tooltipSummary}`}
                            title={tooltipSummary}
                            onClick={() => {
                              setInspectedLine((prev) => (prev === lineNo ? null : lineNo));
                              focusCursorAt(lineNo, 1);
                            }}
                            className={`inline-flex size-4 shrink-0 cursor-pointer items-center justify-center rounded-sm ${
                              hasError ? "bg-pill-pro-bg text-pill-pro-fg" : "bg-pill-warn-bg text-pill-warn-fg"
                            }`}
                          >
                            <AlertTriangle className="size-3" strokeWidth={1.75} aria-hidden />
                          </button>
                        ) : (
                          <span className="inline-block size-4 shrink-0" />
                        )}
                        <span className="tabular-nums">{lineNo}</span>
                      </div>
                    );
                  })}
                </div>

                {/* Layered Syntax-Highlight Overlay + Editable Textarea */}
                <div className="relative min-w-0 flex-1 overflow-hidden">
                  <pre
                    ref={overlayRef}
                    aria-hidden="true"
                    style={{ tabSize: 2 }}
                    className="pointer-events-none absolute inset-0 m-0 overflow-hidden whitespace-pre px-3 py-2.5 font-mono text-xs leading-5"
                  >
                    <div className="min-w-full w-max">
                      {previewLines.map((line, idx) => {
                        const lineNo = idx + 1;
                        const entry = diagnosticsByLine.get(lineNo);
                        const hasErr = Boolean(
                          entry && (entry.mismatches.length > 0 || entry.diagnostics.some((d) => d.severity === "error")),
                        );
                        const hasWarn = !hasErr && Boolean(entry && entry.diagnostics.length > 0);
                        const isCursor = lineNo === cursorLine;
                        const tokens = tokenizeLuauLine(line);
                        return (
                          <div
                            key={lineNo}
                            className={`h-5 ${
                              hasErr
                                ? "bg-pill-pro-bg/25"
                                : hasWarn
                                  ? "bg-pill-warn-bg/25"
                                  : isCursor
                                    ? "bg-blue-soft/30"
                                    : ""
                            }`}
                          >
                            {tokens.length === 0 ? (
                              <span>{"\u00A0"}</span>
                            ) : (
                              tokens.map((tok, ti) => (
                                <span key={ti} className={TOKEN_CLS[tok.kind]}>
                                  {tok.text}
                                </span>
                              ))
                            )}
                          </div>
                        );
                      })}
                    </div>
                  </pre>

                  <Textarea
                    ref={textareaRef}
                    value={draft}
                    onChange={(e) => {
                      setDraft(e.target.value.replace(/\r\n/g, "\n"));
                      syncCursor();
                    }}
                    onScroll={syncScroll}
                    onClick={syncCursor}
                    onKeyUp={syncCursor}
                    onKeyDown={handleEditorKeyDown}
                    aria-label={`Edit Luau source for ${currentPath}`}
                    spellCheck={false}
                    style={{ tabSize: 2, WebkitTextFillColor: "transparent" }}
                    className="relative z-10 h-full min-h-[260px] w-full resize-none overflow-auto whitespace-pre rounded-none border-0 bg-transparent px-3 py-2.5 font-mono text-xs leading-5 text-transparent caret-blue focus-visible:outline-none"
                  />
                </div>
              </div>
            </div>

            {/* Collapsible Symbols & Exports Outline Rail */}
            {symbolsOpen && (
              <aside
                data-symbols-rail
                aria-label="Symbols & Exports"
                className="flex max-h-52 w-full shrink-0 flex-col border-t border-hairline bg-board/50 md:max-h-none md:w-64 md:border-t-0 md:border-l"
              >
                <div className="flex items-center justify-between gap-2 border-b border-hairline px-2.5 py-2">
                  <div className="flex items-center gap-1.5">
                    <span className="text-xs font-medium text-ink">Symbols &amp; Exports</span>
                    <Pill className="bg-surface px-1.5 py-0.5 text-xs tabular-nums text-ink-2">
                      {workspaceSymbols.length}
                    </Pill>
                  </div>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    aria-label="Collapse Symbols & Exports"
                    onClick={() => setSymbolsOpen(false)}
                    className="text-ink-2 hover:text-ink max-md:size-11"
                  >
                    <X className="size-3.5" strokeWidth={1.5} aria-hidden />
                  </Button>
                </div>

                <div className="border-b border-hairline px-2.5 py-1.5">
                  <Input
                    value={symbolFilter}
                    onChange={(e) => setSymbolFilter(e.target.value)}
                    placeholder="Filter symbols…"
                    aria-label="Filter symbols"
                    className="h-8 bg-surface text-xs max-md:min-h-11"
                  />
                </div>

                <div className="min-h-0 flex-1 overflow-y-auto p-2">
                  {filteredSymbols.inFile.length === 0 && filteredSymbols.others.length === 0 ? (
                    <p className="px-1 py-3 text-xs text-ink-3">No symbols indexed yet.</p>
                  ) : (
                    <div className="flex flex-col gap-3">
                      {filteredSymbols.inFile.length > 0 && (
                        <div>
                          <div className="mb-1 px-1 text-xs font-medium text-ink-3">Current File</div>
                          <ul className="flex flex-col gap-0.5">
                            {filteredSymbols.inFile.map((sym, idx) => {
                              const meta = SYMBOL_KIND_META[sym.kind] || SYMBOL_KIND_META.function;
                              return (
                                <li key={`cur-${sym.path}-${sym.name}-${sym.line}-${idx}`}>
                                  <button
                                    type="button"
                                    data-symbol-item={sym.name}
                                    onClick={() => jumpToLocation(sym.path, sym.line, 1)}
                                    className="flex w-full min-w-0 cursor-pointer items-center justify-between gap-1.5 rounded-md px-1.5 py-1 text-left text-xs text-ink hover:bg-surface max-md:min-h-11"
                                  >
                                    <span className="flex min-w-0 flex-1 items-center gap-1.5">
                                      <span className={`shrink-0 rounded px-1 py-0.5 font-mono text-xs ${meta.cls}`}>
                                        {meta.label}
                                      </span>
                                      <span className="min-w-0 truncate font-mono">{sym.name}</span>
                                    </span>
                                    <span className="shrink-0 tabular-nums text-ink-3">L{sym.line}</span>
                                  </button>
                                </li>
                              );
                            })}
                          </ul>
                        </div>
                      )}

                      {filteredSymbols.others.length > 0 && (
                        <div>
                          <div className="mb-1 px-1 text-xs font-medium text-ink-3">Workspace</div>
                          <ul className="flex flex-col gap-0.5">
                            {filteredSymbols.others.map((sym, idx) => {
                              const meta = SYMBOL_KIND_META[sym.kind] || SYMBOL_KIND_META.function;
                              return (
                                <li key={`ws-${sym.path}-${sym.name}-${sym.line}-${idx}`}>
                                  <button
                                    type="button"
                                    data-symbol-item={sym.name}
                                    onClick={() => jumpToLocation(sym.path, sym.line, 1)}
                                    className="flex w-full min-w-0 cursor-pointer items-center justify-between gap-1.5 rounded-md px-1.5 py-1 text-left text-xs text-ink-2 hover:bg-surface hover:text-ink max-md:min-h-11"
                                  >
                                    <span className="flex min-w-0 flex-1 items-center gap-1.5">
                                      <span className={`shrink-0 rounded px-1 py-0.5 font-mono text-xs ${meta.cls}`}>
                                        {meta.label}
                                      </span>
                                      <span className="min-w-0 truncate font-mono">{sym.name}</span>
                                    </span>
                                    <span className="shrink-0 font-mono tabular-nums text-ink-3">
                                      {shortName(sym.path)}:{sym.line}
                                    </span>
                                  </button>
                                </li>
                              );
                            })}
                          </ul>
                        </div>
                      )}
                    </div>
                  )}
                </div>
              </aside>
            )}
          </div>
        )}

        {/* Inline Luau Diagnostics & Contract Mismatches Footer */}
        {(fileDiagnostics.length > 0 || fileMismatches.length > 0) && (
          <div data-luau-diagnostics-footer className="max-h-36 overflow-y-auto border-t border-hairline bg-board/50 px-3 py-2">
            <ul className="flex flex-col gap-1 text-xs">
              {fileMismatches.map((m, idx) => (
                <li key={`fm-${m.caller_path}-${m.caller_line}-${idx}`} className="flex flex-wrap items-center gap-2 text-pill-pro-fg">
                  <button
                    type="button"
                    onClick={() => {
                      setInspectedLine(m.caller_line);
                      focusCursorAt(m.caller_line, 1);
                    }}
                    className="cursor-pointer tabular-nums underline hover:opacity-80"
                  >
                    L{m.caller_line} [{m.kind}]
                  </button>
                  <span className="min-w-0 flex-1 text-ink">{m.message}</span>
                  <button
                    type="button"
                    onClick={() => jumpToLocation(m.target_path, 1, 1)}
                    className="cursor-pointer font-mono text-blue underline hover:opacity-80"
                  >
                    {shortName(m.target_path)}
                  </button>
                </li>
              ))}
              {fileDiagnostics.map((d, idx) => (
                <li
                  key={`${d.path}-${d.line}-${idx}`}
                  className={`flex flex-wrap items-center gap-2 ${d.severity === "error" ? "text-pill-pro-fg" : "text-pill-warn-fg"}`}
                >
                  <button
                    type="button"
                    onClick={() => {
                      setInspectedLine(d.line);
                      focusCursorAt(d.line, 1);
                    }}
                    className="cursor-pointer tabular-nums underline hover:opacity-80"
                  >
                    L{d.line} [{d.code}]
                  </button>
                  <span className="min-w-0 flex-1">{d.message}</span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </div>
  );
});
