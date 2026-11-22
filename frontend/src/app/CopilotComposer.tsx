import { useMemo, useState, type FormEvent, type KeyboardEvent as ReactKeyboardEvent, type RefObject } from "react";
import { AtSign, Brain, FileDiff, Plus, Send, SlidersHorizontal, Undo2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupTextarea } from "@/components/ui/input-group";
import { Kbd } from "@/components/ui/kbd";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Pill } from "@/components/kibo-ui/pill";
import {
  MorphingDialog,
  MorphingDialogClose,
  MorphingDialogContainer,
  MorphingDialogContent,
  MorphingDialogDescription,
  MorphingDialogTitle,
  MorphingDialogTrigger,
} from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import type { ComposerMode, Place, ReasoningLevel } from "./data";

const REASONING_NEXT: Record<ReasoningLevel, ReasoningLevel> = {
  low: "medium",
  medium: "high",
  high: "low",
};

const REASONING_LABEL: Record<ReasoningLevel, string> = {
  low: "Low",
  medium: "Med",
  high: "High",
};

export function CopilotComposer({
  text,
  onTextChange,
  inputRef,
  busy,
  canUndo,
  undoReason,
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
  onOpenSettings,
  onSubmit,
  onConfirmUndo,
  showUpdateAvailable,
  showUpdateRef,
  onShowUpdateAgain,
  canCheckUpdate,
  checkingUpdate,
  onCheckUpdate,
}: {
  text: string;
  onTextChange: (next: string) => void;
  inputRef: RefObject<HTMLTextAreaElement | null>;
  busy: boolean;
  canUndo: boolean;
  undoReason: string;
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
  onOpenSettings: () => void;
  onSubmit: (e: FormEvent) => void;
  onConfirmUndo: () => void;
  showUpdateAvailable: boolean;
  showUpdateRef: RefObject<HTMLButtonElement | null>;
  onShowUpdateAgain: () => void;
  canCheckUpdate: boolean;
  checkingUpdate: boolean;
  onCheckUpdate: () => void;
}) {
  const [pickerOpen, setPickerOpen] = useState(false);

  const availablePaths = useMemo(() => {
    const out: string[] = [];
    for (const p of pluginSelection) {
      if (!out.includes(p)) out.push(p);
    }
    if (place && place !== "error") {
      for (const p of Object.keys(place.scripts || {}).sort()) {
        if (!out.includes(p)) out.push(p);
      }
      for (const p of Object.keys(place.instances || {}).sort()) {
        if (!out.includes(p)) out.push(p);
      }
    }
    return out.filter((p) => !contextPaths.includes(p)).slice(0, 24);
  }, [contextPaths, place, pluginSelection]);

  const placeholder =
    mode === "ask"
      ? "Ask about scripts, DataModel, or errors…"
      : mode === "plan"
        ? "Describe edits to stage as reviewable diffs…"
        : "Describe a system or edit for Studio…";

  const onKeyDown = (e: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (!busy && text.trim()) {
        onSubmit(e as unknown as FormEvent);
      }
    }
  };

  return (
    <div id="composer" data-copilot-composer className="flex flex-none flex-col gap-2 border-t border-hairline bg-surface px-3 py-2.5">
      {/* Context Chips + Picker */}
      <div className="flex flex-wrap items-center gap-1">
        <Button
          variant="outline"
          size="xs"
          aria-label="Attach script or instance context"
          aria-pressed={pickerOpen}
          onClick={() => setPickerOpen((v) => !v)}
          className="bg-board/50 text-ink-2 hover:text-ink max-md:min-h-11"
        >
          <Plus className="size-3" strokeWidth={1.5} aria-hidden />
          Context
        </Button>

        {contextPaths.map((p) => (
          <Pill
            key={p}
            data-context-chip={p}
            className="gap-1 bg-tag-sky-bg py-0.5 pl-2 pr-1 font-mono text-xs text-tag-sky-fg"
          >
            <AtSign className="size-3 shrink-0" strokeWidth={1.5} aria-hidden />
            <span className="max-w-[160px] truncate">{p}</span>
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label={`Remove context ${p}`}
              onClick={() => onRemoveContextPath(p)}
              className="size-6 p-0 text-tag-sky-fg hover:bg-transparent max-md:size-11"
            >
              <X className="size-3" strokeWidth={1.75} aria-hidden />
            </Button>
          </Pill>
        ))}
      </div>

      {pickerOpen && (
        <div
          data-context-picker
          className="flex max-h-32 flex-wrap gap-1 overflow-y-auto rounded-sm border border-hairline bg-board/50 p-2"
        >
          {availablePaths.length === 0 ? (
            <span className="text-xs text-ink-2">No additional scripts or instances in this place yet.</span>
          ) : (
            availablePaths.map((p) => (
              <Button
                key={p}
                variant="outline"
                size="xs"
                onClick={() => {
                  onAddContextPath(p);
                  setPickerOpen(false);
                }}
                className="bg-surface font-mono text-xs max-md:min-h-11"
              >
                @{p}
              </Button>
            ))
          )}
        </div>
      )}

      {/* Mode Switch (Agent | Plan | Ask) + Review Diffs + Reasoning + Model */}
      <div className="flex flex-wrap items-center justify-between gap-1.5">
        <SegmentedControl aria-label="Copilot execution mode" size="sm">
          <SegmentedControlItem
            data-checked={mode === "agent" ? "" : undefined}
            aria-pressed={mode === "agent"}
            onClick={() => onModeChange("agent")}
            className="px-2 text-xs max-md:!h-11"
          >
            Agent
          </SegmentedControlItem>
          <SegmentedControlItem
            data-checked={mode === "plan" ? "" : undefined}
            aria-pressed={mode === "plan"}
            onClick={() => onModeChange("plan")}
            className="px-2 text-xs max-md:!h-11"
          >
            Plan
          </SegmentedControlItem>
          <SegmentedControlItem
            data-checked={mode === "ask" ? "" : undefined}
            aria-pressed={mode === "ask"}
            onClick={() => onModeChange("ask")}
            className="px-2 text-xs max-md:!h-11"
          >
            Ask
          </SegmentedControlItem>
        </SegmentedControl>

        <div className="flex flex-wrap items-center gap-1">
          {mode !== "ask" && (
            <Button
              variant={reviewDiffs || mode === "plan" ? "secondary" : "outline"}
              size="default"
              aria-label="Toggle review diffs before applying"
              aria-pressed={reviewDiffs || mode === "plan"}
              disabled={mode === "plan"}
              onClick={() => onReviewDiffsChange(!reviewDiffs)}
              className="gap-1 text-xs max-md:min-h-11"
            >
              <FileDiff className="size-3" strokeWidth={1.5} aria-hidden />
              Review diffs
            </Button>
          )}
          <span className="text-xs text-ink-2">Effort</span>
          <Button
            variant="outline"
            size="default"
            aria-label={`Reasoning effort ${REASONING_LABEL[reasoning]}`}
            onClick={() => onReasoningChange(REASONING_NEXT[reasoning])}
            className="gap-1 bg-board/40 font-mono text-xs text-ink-2 max-md:min-h-11"
          >
            <Brain className="size-3" strokeWidth={1.5} aria-hidden />
            {REASONING_LABEL[reasoning]}
          </Button>
          <MorphingDialog transition={dialogSpring}>
            <MorphingDialogTrigger
              thing="undo"
              blocked={!canUndo}
              describedBy={undoReason ? "undo-reason" : undefined}
              aria-label="Undo"
              className="inline-flex min-h-11 cursor-pointer items-center gap-1 rounded-lg border border-hairline bg-surface px-2.5 text-xs font-medium text-ink-2 outline-none transition-[color,background-color] duration-(--dur-press) ease-(--ease-house) hover:bg-track hover:text-ink disabled:cursor-not-allowed disabled:bg-board disabled:text-ink-3 md:min-h-8"
            >
              <Undo2 className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>Undo</span>
            </MorphingDialogTrigger>
            <MorphingDialogContainer>
              <MorphingDialogContent className="relative mx-3 w-full max-w-[420px] rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift">
                <div data-undo-dialog className="flex flex-col gap-4">
                  <div className="flex flex-col gap-1.5">
                    <MorphingDialogTitle className="text-ui font-medium text-ink">
                      Undo the last Studio build?
                    </MorphingDialogTitle>
                    <MorphingDialogDescription className="text-sm leading-relaxed text-ink-2">
                      This reverts the scripts and Workspace instances modified by the most recent build in this chat.
                    </MorphingDialogDescription>
                  </div>
                  <div className="flex flex-wrap justify-end gap-2">
                    <MorphingDialogClose
                      aria-label="Keep it"
                      className="static inline-flex min-h-11 cursor-pointer items-center justify-center rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-board md:min-h-9"
                    >
                      Keep it
                    </MorphingDialogClose>
                    <MorphingDialogClose
                      data-undo-yes
                      aria-label="Yes, undo it"
                      onClick={onConfirmUndo}
                      className="static inline-flex min-h-11 cursor-pointer items-center justify-center rounded-lg border border-blue bg-blue px-4 text-sm font-medium text-blue-fg outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-blue-hover md:min-h-9"
                    >
                      Yes, undo it
                    </MorphingDialogClose>
                  </div>
                </div>
              </MorphingDialogContent>
            </MorphingDialogContainer>
          </MorphingDialog>
          <Button
            variant="ghost"
            size="xs"
            aria-label="Configure active AI model"
            onClick={onOpenSettings}
            className="max-w-[116px] gap-1 font-mono text-xs text-ink-2 hover:text-ink max-md:min-h-11"
          >
            <SlidersHorizontal className="size-3 shrink-0" strokeWidth={1.5} aria-hidden />
            <span className="truncate">{modelLabel || "Model"}</span>
          </Button>
        </div>
      </div>

      {/* Main Prompt Form */}
      <form onSubmit={onSubmit} className="flex flex-col gap-1.5">
        <div className="flex items-end gap-1.5">
          <InputGroup className="min-w-0 flex-1 bg-board/45">
            <InputGroupTextarea
              id="composer-input"
              ref={inputRef}
              rows={2}
              value={text}
              onChange={(e) => onTextChange(e.target.value)}
              onKeyDown={onKeyDown}
              placeholder={placeholder}
              disabled={busy}
              aria-label="Describe a game or change"
              className="min-h-11 resize-none text-sm"
            />
          </InputGroup>

          <Button
            type="submit"
            variant="default"
            disabled={busy || !text.trim()}
            aria-label="Send"
            className="h-11 shrink-0 px-3.5"
          >
            <Send className="size-4" strokeWidth={1.5} aria-hidden />
            <span className="sr-only sm:not-sr-only">Send</span>
          </Button>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-ink-2">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="hidden items-center gap-1 sm:inline-flex">
              <Kbd className="px-1 font-mono">Enter</Kbd>
              <span>send</span>
              <span aria-hidden>·</span>
              <Kbd className="px-1 font-mono">⌘K</Kbd>
              <span>commands</span>
            </span>
            {undoReason && <span id="undo-reason" className="truncate">{undoReason}</span>}
          </div>
          <div className="flex items-center gap-1.5">
            {showUpdateAvailable && (
              <Button
                ref={showUpdateRef}
                variant="ghost"
                size="xs"
                onClick={onShowUpdateAgain}
                className="text-ink underline-offset-2 hover:underline max-md:min-h-11"
              >
                Show update
              </Button>
            )}
            {canCheckUpdate && (
              <Button
                variant="ghost"
                size="xs"
                aria-label="Check for updates"
                disabled={checkingUpdate}
                onClick={onCheckUpdate}
                className="text-ink-2 hover:text-ink max-md:min-h-11"
              >
                Check updates
              </Button>
            )}
          </div>
        </div>
      </form>
    </div>
  );
}
