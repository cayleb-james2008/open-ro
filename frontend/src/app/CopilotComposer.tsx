import { useEffect, useMemo, useRef, useState, type DragEvent, type FormEvent, type KeyboardEvent as ReactKeyboardEvent, type RefObject } from "react";
import * as Popover from "@radix-ui/react-popover";
import { toast } from "sonner";
import { AtSign, Check, ChevronDown, Paperclip, Send, SlidersHorizontal, Square, Undo2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupTextarea } from "@/components/ui/input-group";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
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
import { post, read, type AntigravityModel, type ComposerMode, type ConfigSnap, type Place, type ReasoningLevel } from "./data";

const REASONING_LABELS: { id: ReasoningLevel; label: string }[] = [
  { id: "low", label: "Low" },
  { id: "medium", label: "Med" },
  { id: "high", label: "High" },
];

export function CopilotComposer({
  text,
  onTextChange,
  inputRef,
  busy,
  swarmRunning,
  onStopSwarm,
  canUndo,
  place,
  contextPaths,
  pluginSelection,
  onAddContextPath,
  onRemoveContextPath,
  mode,
  onModeChange,
  reviewDiffs,
  onReviewDiffsChange,
  reasoning,
  onReasoningChange,
  modelLabel,
  onModelSelected,
  onOpenSettings,
  onSubmit,
  onConfirmUndo,
  onQueuePlan,
}: {
  text: string;
  onTextChange: (next: string) => void;
  inputRef: RefObject<HTMLTextAreaElement | null>;
  busy: boolean;
  swarmRunning?: boolean;
  onStopSwarm?: () => void;
  canUndo: boolean;
  place: Place | null | "error";
  contextPaths: string[];
  pluginSelection: string[];
  onAddContextPath: (path: string) => void;
  onRemoveContextPath: (path: string) => void;
  mode: ComposerMode;
  onModeChange: (m: ComposerMode) => void;
  reviewDiffs: boolean;
  onReviewDiffsChange: (v: boolean) => void;
  reasoning: ReasoningLevel;
  onReasoningChange: (r: ReasoningLevel) => void;
  modelLabel: string;
  onModelSelected?: (modelId: string) => void;
  onOpenSettings: () => void;
  onSubmit: (e: FormEvent) => void;
  onConfirmUndo: () => void;
  onQueuePlan?: () => void;
}) {
  const [attachOpen, setAttachOpen] = useState(false);
  const [modelOpen, setModelOpen] = useState(false);
  const [optsOpen, setOptsOpen] = useState(false);
  const [undoOpen, setUndoOpen] = useState(false);
  const [dragOver, setDragOver] = useState(false);
  const [models, setModels] = useState<AntigravityModel[]>([]);
  const [gateways, setGateways] = useState<{ base_url: string; model_id: string }[]>([]);
  const [activeModel, setActiveModel] = useState("");
  const fileRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(180, Math.max(72, el.scrollHeight))}px`;
  }, [text, inputRef]);

  const loadModels = async () => {
    try {
      const [m, c] = await Promise.all([
        read<{ models?: AntigravityModel[] }>("/api/models").catch(() => ({ models: [] })),
        read<ConfigSnap>("/api/config"),
      ]);
      setModels(m.models ?? c.antigravity_models ?? []);
      setGateways(c.detected_gateways ?? []);
      setActiveModel(c.model_id || "");
    } catch { /* ignore */ }
  };

  const switchModel = async (next: { provider: "antigravity" | "openai"; model_id: string; base_url?: string }) => {
    try {
      const res = await post<ConfigSnap & { ok?: boolean; error?: string }>("/api/config", {
        provider: next.provider,
        base_url: next.provider === "antigravity" ? "agy://antigravity" : next.base_url || "",
        model_id: next.model_id,
      });
      if (res.ok === false) { toast.error(res.error || "Could not switch model."); return; }
      setActiveModel(res.model_id || next.model_id);
      onModelSelected?.(res.model_id || next.model_id);
      setModelOpen(false);
      toast.success(`Model: ${next.model_id}`);
    } catch {
      toast.error("Could not switch model.");
    }
  };

  const availablePaths = useMemo(() => {
    const out: string[] = [];
    for (const p of pluginSelection) if (!out.includes(p)) out.push(p);
    if (place && place !== "error") {
      for (const p of Object.keys(place.scripts || {}).sort()) if (!out.includes(p)) out.push(p);
      for (const p of Object.keys(place.instances || {}).sort()) if (!out.includes(p)) out.push(p);
    }
    return out.filter((p) => !contextPaths.includes(p)).slice(0, 20);
  }, [contextPaths, place, pluginSelection]);

  const onDrop = (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setDragOver(false);
    const files = [...e.dataTransfer.files];
    for (const f of files) onAddContextPath(f.name);
    const droppedText = e.dataTransfer.getData("text/plain")?.trim();
    if (!files.length && droppedText) onAddContextPath(droppedText);
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (!busy && text.trim()) onSubmit(e as unknown as FormEvent);
    }
  };

  const canStop = (mode === "swarm" && swarmRunning) || busy;

  return (
    <div id="composer" data-copilot-composer className="flex flex-none flex-col border-t border-hairline bg-surface p-2.5">
      <form
        onSubmit={onSubmit}
        onDragOver={(e) => { e.preventDefault(); setDragOver(true); }}
        onDragLeave={() => setDragOver(false)}
        onDrop={(e) => onDrop(e as unknown as DragEvent<HTMLDivElement>)}
        className={`flex flex-col gap-1.5 rounded-card border bg-board/40 p-2 transition-[border-color,background-color] duration-(--dur-press) ease-(--ease-house) ${dragOver ? "border-blue bg-blue-soft/30" : "border-hairline"}`}
      >
        {contextPaths.length > 0 && (
          <div className="flex flex-wrap items-center gap-1">
            {contextPaths.map((p) => (
              <Pill key={p} data-context-chip={p} className="gap-1 bg-tag-sky-bg py-0.5 pl-2 pr-1 font-mono text-xs text-tag-sky-fg">
                <AtSign className="size-3 shrink-0" strokeWidth={1.5} aria-hidden />
                <span className="max-w-[140px] truncate">{p}</span>
                <Button variant="ghost" size="icon-xs" aria-label={`Remove ${p}`} onClick={() => onRemoveContextPath(p)} className="size-5 p-0 text-tag-sky-fg hover:bg-transparent max-md:size-11">
                  <X className="size-3" strokeWidth={1.75} aria-hidden />
                </Button>
              </Pill>
            ))}
          </div>
        )}

        <InputGroup className="bg-surface">
          <InputGroupTextarea
            id="composer-input"
            ref={inputRef}
            rows={3}
            value={text}
            onChange={(e) => onTextChange(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder="Describe your game idea or paste reference URLs..."
            disabled={busy}
            aria-label="Describe your game idea"
            className="min-h-[72px] max-h-48 resize-none px-2.5 py-2 text-sm leading-relaxed"
          />
        </InputGroup>

        <input
          ref={fileRef}
          type="file"
          accept="image/*"
          multiple
          className="hidden"
          onChange={(e) => {
            for (const f of [...(e.target.files ?? [])]) onAddContextPath(f.name);
            e.target.value = "";
          }}
        />

        {/* Compact Single Footer Row */}
        <div className="flex flex-wrap items-center justify-between gap-1.5 border-t border-hairline/70 pt-1.5">
          <div className="flex min-w-0 items-center gap-1">
            <Popover.Root open={attachOpen} onOpenChange={setAttachOpen}>
              <Popover.Trigger asChild>
                <Button variant="ghost" size="icon-sm" aria-label="Attach reference or file" className="shrink-0 text-ink-2 hover:text-ink max-md:size-11">
                  <Paperclip className="size-3.5" strokeWidth={1.5} aria-hidden />
                </Button>
              </Popover.Trigger>
              <Popover.Portal>
                <Popover.Content sideOffset={6} align="start" className="z-50 flex w-64 flex-col gap-1.5 rounded-dialog border border-hairline bg-surface p-2.5 text-ink shadow-lift">
                  <Button variant="outline" size="sm" onClick={() => { fileRef.current?.click(); setAttachOpen(false); }} className="justify-start max-md:min-h-11">
                    Attach reference image…
                  </Button>
                  {availablePaths.length > 0 && (
                    <div className="flex max-h-40 flex-col gap-1 overflow-y-auto border-t border-hairline pt-1.5">
                      {availablePaths.map((p) => (
                        <Button key={p} variant="ghost" size="xs" onClick={() => { onAddContextPath(p); setAttachOpen(false); }} className="justify-start truncate font-mono text-xs max-md:min-h-11">
                          @{p}
                        </Button>
                      ))}
                    </div>
                  )}
                </Popover.Content>
              </Popover.Portal>
            </Popover.Root>

            <SegmentedControl aria-label="Agent mode" size="sm">
              {(["agent", "plan", "ask", "swarm"] as const).map((m) => (
                <SegmentedControlItem
                  key={m}
                  data-checked={mode === m ? "" : undefined}
                  aria-pressed={mode === m}
                  onClick={() => onModeChange(m)}
                  className="!px-2 sm:text-xs max-md:!h-11"
                >
                  {m === "swarm" ? "Swarm" : m === "agent" ? "Agent" : m === "plan" ? "Plan" : "Ask"}
                </SegmentedControlItem>
              ))}
            </SegmentedControl>
          </div>

          <div className="ml-auto flex items-center gap-1">
            {canUndo && (
              <Button
                variant="outline"
                size="xs"
                onClick={() => setUndoOpen(true)}
                aria-label="Undo"
                className="gap-1 px-2 text-xs text-ink max-md:min-h-11"
              >
                <Undo2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>Undo</span>
              </Button>
            )}
            <Popover.Root open={modelOpen} onOpenChange={(o) => { setModelOpen(o); if (o) void loadModels(); }}>
              <Popover.Trigger asChild>
                <Button variant="ghost" size="xs" aria-label="Switch AI model" className="max-w-[148px] gap-1 px-2 text-xs text-ink-2 hover:text-ink max-md:min-h-11">
                  <span className="truncate">{modelLabel ? modelLabel.replace(/^gemini-4-/, "").replace(/-/g, " ") : "Model"}</span>
                  <ChevronDown className="size-3 shrink-0" strokeWidth={1.5} aria-hidden />
                </Button>
              </Popover.Trigger>
              <Popover.Portal>
                <Popover.Content sideOffset={6} align="end" className="z-50 flex w-64 flex-col gap-1 rounded-dialog border border-hairline bg-surface p-2 text-ink shadow-lift">
                  <div className="max-h-56 overflow-y-auto">
                    {models.map((m) => (
                      <Button key={m.id} variant={activeModel === m.id ? "secondary" : "ghost"} size="sm" onClick={() => void switchModel({ provider: "antigravity", model_id: m.id })} className="w-full justify-between text-xs max-md:min-h-11">
                        <span className="truncate">{m.label || m.id}</span>
                        {activeModel === m.id && <Check className="size-3.5 shrink-0" strokeWidth={2} aria-hidden />}
                      </Button>
                    ))}
                    {gateways.map((g) => (
                      <Button key={g.model_id} variant={activeModel === g.model_id ? "secondary" : "ghost"} size="sm" onClick={() => void switchModel({ provider: "openai", model_id: g.model_id, base_url: g.base_url })} className="w-full justify-between font-mono text-xs max-md:min-h-11">
                        <span className="truncate">{g.model_id}</span>
                        {activeModel === g.model_id && <Check className="size-3.5 shrink-0" strokeWidth={2} aria-hidden />}
                      </Button>
                    ))}
                  </div>
                  <Button variant="outline" size="xs" onClick={() => { setModelOpen(false); onOpenSettings(); }} className="mt-1 border-t border-hairline pt-1 max-md:min-h-11">
                    All settings…
                  </Button>
                </Popover.Content>
              </Popover.Portal>
            </Popover.Root>

            <Popover.Root open={optsOpen} onOpenChange={setOptsOpen}>
              <Popover.Trigger asChild>
                <Button variant="ghost" size="icon-sm" aria-label="Effort and options" className="shrink-0 text-ink-2 hover:text-ink max-md:size-11">
                  <SlidersHorizontal className="size-3.5" strokeWidth={1.5} aria-hidden />
                </Button>
              </Popover.Trigger>
              <Popover.Portal>
                <Popover.Content sideOffset={6} align="end" className="z-50 flex w-56 flex-col gap-2.5 rounded-dialog border border-hairline bg-surface p-3 text-ink shadow-lift">
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-xs font-medium text-ink-2">Effort</span>
                    <SegmentedControl aria-label="Effort" size="sm">
                      {REASONING_LABELS.map((r) => (
                        <SegmentedControlItem key={r.id} data-checked={reasoning === r.id ? "" : undefined} aria-pressed={reasoning === r.id} onClick={() => onReasoningChange(r.id)} className="!px-1.5 sm:text-xs max-md:!h-11">
                          {r.label}
                        </SegmentedControlItem>
                      ))}
                    </SegmentedControl>
                  </div>
                  {mode !== "ask" && (
                    <Button variant={reviewDiffs || mode === "plan" ? "secondary" : "outline"} size="sm" disabled={mode === "plan"} onClick={() => onReviewDiffsChange(!reviewDiffs)} className="justify-between text-xs max-md:min-h-11">
                      <span>Review diffs</span>
                      <span>{reviewDiffs || mode === "plan" ? "On" : "Off"}</span>
                    </Button>
                  )}
                  {onQueuePlan && text.trim() && (
                    <Button variant="outline" size="sm" onClick={() => { setOptsOpen(false); onQueuePlan(); }} className="justify-start text-xs max-md:min-h-11">
                      Queue background plan
                    </Button>
                  )}
                  {canUndo && (
                    <Button variant="outline" size="sm" onClick={() => { setOptsOpen(false); setUndoOpen(true); }} className="justify-start gap-1.5 text-xs max-md:min-h-11">
                      <Undo2 className="size-3.5" strokeWidth={1.5} aria-hidden />Undo last build
                    </Button>
                  )}
                </Popover.Content>
              </Popover.Portal>
            </Popover.Root>

            {canStop && onStopSwarm && !text.trim() ? (
              <Button type="button" variant="outline" size="sm" onClick={onStopSwarm} aria-label="Stop" className="shrink-0 gap-1 max-md:min-h-11">
                <Square className="size-3.5" strokeWidth={1.5} aria-hidden />Stop
              </Button>
            ) : (
              <Button type="submit" variant="default" size="sm" disabled={busy || !text.trim()} aria-label="Send" className="shrink-0 gap-1 max-md:min-h-11">
                <Send className="size-3.5" strokeWidth={1.5} aria-hidden />Send
              </Button>
            )}
          </div>
        </div>
      </form>

      <MorphingDialog transition={dialogSpring} open={undoOpen} onOpenChange={setUndoOpen}>
        <MorphingDialogContainer>
          <MorphingDialogContent className="relative mx-3 w-full max-w-[420px] rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift">
            <div data-undo-dialog className="flex flex-col gap-4">
              <div className="flex flex-col gap-1.5">
                <MorphingDialogTitle className="text-ui font-medium text-ink">Undo the last build?</MorphingDialogTitle>
                <MorphingDialogDescription className="text-sm text-ink-2">Reverts the scripts and parts from the last build in this chat.</MorphingDialogDescription>
              </div>
              <div className="flex flex-wrap justify-end gap-2">
                <MorphingDialogClose aria-label="Keep it" className="static inline-flex min-h-11 cursor-pointer items-center justify-center rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink hover:bg-board md:min-h-9">Keep it</MorphingDialogClose>
                <MorphingDialogClose data-undo-yes aria-label="Yes, undo it" onClick={onConfirmUndo} className="static inline-flex min-h-11 cursor-pointer items-center justify-center rounded-lg border border-blue bg-blue px-4 text-sm font-medium text-blue-fg hover:bg-blue-hover md:min-h-9">Yes, undo it</MorphingDialogClose>
              </div>
            </div>
          </MorphingDialogContent>
        </MorphingDialogContainer>
      </MorphingDialog>
    </div>
  );
}
