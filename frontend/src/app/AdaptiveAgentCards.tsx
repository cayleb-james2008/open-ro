import { useMemo, useState } from "react";
import { toast } from "sonner";
import {
  Check,
  ChevronDown,
  CircleCheck,
  CircleX,
  ExternalLink,
  FileCode2,
  Hammer,
  Image as ImageIcon,
  Loader2,
  Play,
  RefreshCw,
  ShieldAlert,
  Sparkles,
  Upload,
  Wrench,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import { Expandable, ExpandableTrigger } from "@/components/ui/expandable";
import {
  MorphingDialog,
  MorphingDialogClose,
  MorphingDialogContainer,
  MorphingDialogContent,
  MorphingDialogTitle,
  MorphingDialogTrigger,
} from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import { call, enc, type CreatorHubSetupResult } from "./ship/api";

export type AdaptiveAgentCardsProps = {
  text: string;
  projectId?: string | null;
  onPrefillPrompt?: (prompt: string) => void;
  onQuickCreatorHubSetup?: () => Promise<CreatorHubSetupResult | void>;
  onOpenShip?: () => void;
  onRetry?: () => void;
  retryDisabled?: boolean;
};

type ArtCardItem = {
  path: string;
  targetPath: string;
  category: string;
  isPending: boolean;
};

type PlaytestSummary = {
  runId: string | null;
  passed: boolean | null;
  consoleErrors: number | null;
  coreLoop: { move?: string; act?: string; hud?: string };
  captureUrl: string | null;
  captureName: string | null;
};

type CreatorHubAction = {
  blockedReason: "login_required" | "2fa_required" | "captcha_required" | "account_moderated" | null;
  universeId: string | null;
  placeId: string | null;
  url: string | null;
};

type ToolActivity = {
  kind: "edit" | "build" | "check" | "upload";
  label: string;
  detail: string;
};

const ART_RE = /\b(art\/(?:cards|weapons|store|ui|worlds)\/[A-Za-z0-9_.-]+(?:\/[A-Za-z0-9_.-]+)*\.png)\b/gi;
const FILE_RE = /\b((?:src|assets|ServerScriptService|StarterPlayerScripts|ReplicatedStorage)\/[A-Za-z0-9_.-]+(?:\/[A-Za-z0-9_.-]+)*\.(?:lua|luau|json|rbxlx?|png|ogg|wav|obj|fbx)|(?:openro|default)\.project\.json|assets\.lock\.json)\b/gi;
const TOOL_RE = /\b(rojo\s+build|selene|stylua|assets_upload|assets_codegen|products_sync|products_codegen|creator_hub_setup|creator_hub_configure|project_publish|opencloud_status|studio_sync|art_generate)\b/gi;
const BLOCKED_RE = /\b(login_required|2fa_required|captcha_required|account_moderated)\b/i;
const URL_RE = /https?:\/\/(?:create\.roblox\.com|(?:www\.)?roblox\.com\/games\/\d+)[^\s"'`),;>]*/gi;

function extractArtItems(text: string): ArtCardItem[] {
  const seen = new Set<string>();
  const out: ArtCardItem[] = [];
  for (const m of text.matchAll(ART_RE)) {
    const raw = m[1].replace(/[.,;:!?]+$/, "");
    if (!raw || raw.startsWith("art/references/") || seen.has(raw)) continue;
    seen.add(raw);
    const pendingMatch = raw.match(/^(.*)\.pending-angle\.\d+(\.png)$/i);
    const targetPath = pendingMatch ? `${pendingMatch[1]}${pendingMatch[2]}` : raw;
    const category = raw.split("/")[1] || "art";
    out.push({ path: raw, targetPath, category, isPending: Boolean(pendingMatch) });
    if (out.length >= 4) break;
  }
  return out;
}

function normalizeLoopState(raw: string | undefined): string | undefined {
  if (!raw) return undefined;
  const v = raw.toLowerCase();
  if (v === "true" || v === "pass" || v === "passed" || v === "ok" || v === "tested") return "passed";
  if (v === "false" || v === "fail" || v === "failed") return "failed";
  if (v === "untested" || v === "skipped") return v;
  return v;
}

function extractPlaytest(text: string, projectId?: string | null): PlaytestSummary | null {
  const runIdMatch = text.match(/\b((?:run|pt)-[A-Za-z0-9_-]{4,})\b/i);
  const directShotMatch = text.match(/(\/api\/projects\/[^\s"'`),;>]+\/playtests\/[^\s"'`),;>]+\.(?:png|jpg|jpeg))/i);
  const relShotMatch = text.match(/\b(?:playtests\/[^/\s]+\/shots|screenshots)\/([A-Za-z0-9_.-]+\.(?:png|jpg|jpeg))\b/i);

  const moveMatch = text.match(/\bmove\b(?:\s*[:=]\s*|\s+is\s+|\s+)(passed|pass|failed|fail|tested|ok|untested|skipped|true|false)\b/i);
  const actMatch = text.match(/\bact\b(?:\s*[:=]\s*|\s+is\s+|\s+)(passed|pass|failed|fail|tested|ok|untested|skipped|true|false)\b/i);
  const hudMatch = text.match(/\bhud\b(?:\s*[:=]\s*|\s+is\s+|\s+)(passed|pass|failed|fail|tested|ok|untested|skipped|true|false)\b/i);
  const jsonCoreMatch = (key: "move" | "act" | "hud") => {
    const r = new RegExp(`"${key}"\\s*:\\s*\\{[^}]*?"(?:status|passed)"\\s*:\\s*"?(true|false|passed|failed|untested|skipped)"?`, "i");
    return text.match(r)?.[1];
  };

  const move = normalizeLoopState(moveMatch?.[1] || jsonCoreMatch("move"));
  const act = normalizeLoopState(actMatch?.[1] || jsonCoreMatch("act"));
  const hud = normalizeLoopState(hudMatch?.[1] || jsonCoreMatch("hud"));
  const hasCoreLoop = Boolean(move || act || hud || /\bcore_loop\b/i.test(text));

  let passed: boolean | null = null;
  if (/\bplaytest\s+(?:has\s+)?passed\b|"passed"\s*:\s*true|\bverdict\s*[:=]\s*pass(?:ed)?\b|\bplaytest\b[^.\n]{0,30}\bpassed\b/i.test(text)) {
    passed = true;
  } else if (/\bplaytest\s+(?:has\s+)?failed\b|"passed"\s*:\s*false|\bverdict\s*[:=]\s*fail(?:ed)?\b|\bplaytest\b[^.\n]{0,30}\bfailed\b/i.test(text)) {
    passed = false;
  }

  const errMatch =
    text.match(/\b(\d+)\s+console\s+errors?\b/i) ||
    text.match(/\bconsole[_\s]errors?\s*[:=]\s*(\d+)\b/i);
  const consoleErrors = errMatch ? Number(errMatch[1]) : null;

  const looksLikePlaytest = Boolean(
    runIdMatch ||
      directShotMatch ||
      (hasCoreLoop && (move || act || hud)) ||
      (passed !== null && (consoleErrors !== null || relShotMatch || /\bplaytest\b/i.test(text))),
  );
  if (!looksLikePlaytest) return null;

  const runId = runIdMatch?.[1] ?? null;
  let captureUrl: string | null = directShotMatch?.[1] ?? null;
  let captureName: string | null = null;
  if (!captureUrl && relShotMatch && runId && projectId) {
    captureName = relShotMatch[1];
    captureUrl = `/api/projects/${enc(projectId)}/playtests/${enc(runId)}/shots/${enc(captureName)}`;
  } else if (captureUrl) {
    captureName = captureUrl.split("/").pop() || "Playtest capture";
  } else if (relShotMatch) {
    captureName = relShotMatch[1];
  }

  return {
    runId,
    passed,
    consoleErrors,
    coreLoop: { move, act, hud },
    captureUrl,
    captureName,
  };
}

function extractCreatorHubAction(text: string): CreatorHubAction | null {
  const blocked = text.match(BLOCKED_RE)?.[1]?.toLowerCase() as CreatorHubAction["blockedReason"] | undefined;
  const univMatch = text.match(/\buniverse[_\s]?id["'\s:=]+(\d{5,})\b/i) || text.match(/creations\/experiences\/(\d{5,})/i);
  const placeMatch = text.match(/\b(?:root_?)?place[_\s]?id["'\s:=]+(\d{5,})\b/i) || text.match(/roblox\.com\/games\/(\d{5,})/i);
  const urlMatch = text.match(URL_RE)?.[0]?.replace(/[.,;:!?]+$/, "") ?? null;
  const mentionsSetup = /\b(action_required|creator[_\s]?hub|open[_\s]?cloud)\b/i.test(text);

  if (!blocked && !urlMatch && !(mentionsSetup && (univMatch || placeMatch))) return null;
  return {
    blockedReason: blocked ?? null,
    universeId: univMatch?.[1] ?? null,
    placeId: placeMatch?.[1] ?? null,
    url: urlMatch,
  };
}

function extractToolActivities(text: string): ToolActivity[] {
  const seen = new Set<string>();
  const out: ToolActivity[] = [];

  for (const m of text.matchAll(TOOL_RE)) {
    const tool = m[1].toLowerCase().replace(/\s+/g, " ");
    const key = `tool:${tool}`;
    if (seen.has(key)) continue;
    seen.add(key);
    const kind: ToolActivity["kind"] =
      tool.includes("upload") || tool.includes("publish") || tool.includes("sync")
        ? "upload"
        : tool.includes("selene") || tool.includes("stylua") || tool.includes("status")
          ? "check"
          : "build";
    out.push({ kind, label: kind === "upload" ? "Sync" : kind === "check" ? "Check" : "Build", detail: tool });
    if (out.length >= 8) return out;
  }

  for (const m of text.matchAll(FILE_RE)) {
    const file = m[1].replace(/[.,;:!?]+$/, "");
    const key = `file:${file}`;
    if (seen.has(key)) continue;
    seen.add(key);
    const kind: ToolActivity["kind"] = file.startsWith("assets/") || file === "assets.lock.json" ? "upload" : "edit";
    out.push({ kind, label: kind === "upload" ? "Asset" : "File", detail: file });
    if (out.length >= 8) break;
  }

  return out;
}

function blockedLabel(reason: CreatorHubAction["blockedReason"]): string {
  switch (reason) {
    case "login_required":
      return "Creator Hub login needed";
    case "2fa_required":
      return "2FA confirmation needed";
    case "captcha_required":
      return "Security check needed";
    case "account_moderated":
      return "Account review needed";
    default:
      return "Creator Hub & Open Cloud";
  }
}

function loopChipTone(state?: string): string {
  if (state === "passed") return "bg-pill-active-bg text-pill-active-fg";
  if (state === "failed") return "bg-pill-pro-bg text-pill-pro-fg";
  return "bg-board text-ink-2";
}

function activityIcon(kind: ToolActivity["kind"]) {
  if (kind === "build") return <Hammer className="size-3 shrink-0 text-blue" strokeWidth={1.5} aria-hidden />;
  if (kind === "check") return <ShieldAlert className="size-3 shrink-0 text-dot-amber" strokeWidth={1.5} aria-hidden />;
  if (kind === "upload") return <Upload className="size-3 shrink-0 text-dot-green" strokeWidth={1.5} aria-hidden />;
  return <FileCode2 className="size-3 shrink-0 text-ink-2" strokeWidth={1.5} aria-hidden />;
}

export function AdaptiveAgentCards({
  text,
  projectId,
  onPrefillPrompt,
  onQuickCreatorHubSetup,
  onOpenShip,
  onRetry,
  retryDisabled,
}: AdaptiveAgentCardsProps) {
  const [approvingPath, setApprovingPath] = useState<string | null>(null);
  const [approvedPaths, setApprovedPaths] = useState<Record<string, boolean>>({});
  const [failedImages, setFailedImages] = useState<Record<string, boolean>>({});
  const [setupBusy, setSetupBusy] = useState(false);

  const artItems = useMemo(() => extractArtItems(text), [text]);
  const playtest = useMemo(() => extractPlaytest(text, projectId), [text, projectId]);
  const creatorHub = useMemo(() => extractCreatorHubAction(text), [text]);
  const activities = useMemo(() => extractToolActivities(text), [text]);

  if (!artItems.length && !playtest && !creatorHub && !activities.length) {
    return null;
  }

  const handleApproveArt = async (item: ArtCardItem) => {
    if (!projectId) {
      toast.error("Open a project first to approve art.");
      return;
    }
    setApprovingPath(item.path);
    try {
      const r = await call<{ path?: string }>("/api/art/approve", {
        project: projectId,
        candidate_path: item.path,
        target_path: item.targetPath,
        reviewer: "owner",
        verdict: "pass",
      });
      if (r.ok) {
        setApprovedPaths((prev) => ({ ...prev, [item.path]: true }));
        toast.success(`Approved ${item.targetPath.split("/").pop() || "artwork"}`);
      } else {
        toast.error(r.error || "Could not approve artwork.");
      }
    } finally {
      setApprovingPath(null);
    }
  };

  const handleQuickSetup = async () => {
    if (!onQuickCreatorHubSetup || setupBusy) return;
    setSetupBusy(true);
    try {
      await onQuickCreatorHubSetup();
    } finally {
      setSetupBusy(false);
    }
  };

  return (
    <div className="mt-2 flex flex-col gap-2" data-adaptive-cards>
      {/* 1. Art / Weapon-Sheet Preview Card */}
      {artItems.length > 0 && (
        <div
          className="flex flex-col gap-2 rounded-sm border border-hairline bg-board/50 p-2"
          data-adaptive-card="art"
        >
          <div className="flex items-center justify-between gap-2">
            <span className="inline-flex items-center gap-1.5 text-xs font-medium text-ink">
              <ImageIcon className="size-3.5 shrink-0 text-blue" strokeWidth={1.5} aria-hidden />
              Art preview
            </span>
            <Pill className="bg-surface px-1.5 py-0 text-xs text-ink-2">
              <span className="font-mono tabular-nums">{artItems.length}</span>
            </Pill>
          </div>
          <ul className="flex flex-col gap-2">
            {artItems.map((item, idx) => {
              const src = projectId ? `/api/art/image?project=${enc(projectId)}&path=${enc(item.path)}` : "";
              const imgBroken = !src || Boolean(failedImages[item.path]);
              const approved = Boolean(approvedPaths[item.path]);
              const approving = approvingPath === item.path;
              const name = item.targetPath.split("/").pop() || item.path;

              return (
                <li
                  key={item.path}
                  className="flex items-start gap-2.5 rounded-sm border border-hairline bg-surface/80 p-2"
                >
                  {!imgBroken ? (
                    <MorphingDialog transition={dialogSpring}>
                      <MorphingDialogTrigger
                        aria-label={`Preview ${name}`}
                        className="shrink-0 overflow-hidden rounded-sm border border-hairline bg-board outline-none active:opacity-80 focus-visible:ring-2 focus-visible:ring-focus"
                      >
                        <img
                          src={src}
                          alt={name}
                          loading={idx === 0 ? "eager" : "lazy"}
                          fetchPriority={idx === 0 ? "high" : undefined}
                          onError={() => setFailedImages((p) => ({ ...p, [item.path]: true }))}
                          className="size-16 object-cover"
                        />
                      </MorphingDialogTrigger>
                      <MorphingDialogContainer>
                        <MorphingDialogContent className="relative mx-3 flex max-h-[88dvh] w-full max-w-[780px] flex-col gap-3 rounded-dialog border border-hairline bg-surface p-4 text-ink shadow-lift">
                          <MorphingDialogTitle className="font-heading text-ui font-semibold">{name}</MorphingDialogTitle>
                          <img src={src} alt={name} className="min-h-0 w-full flex-1 rounded-sm object-contain" />
                          <p className="truncate font-mono text-xs text-ink-3">{item.path}</p>
                          <MorphingDialogClose
                            aria-label="Close preview"
                            className="static inline-flex min-h-11 cursor-pointer items-center justify-center self-end rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none hover:bg-board focus-visible:ring-2 focus-visible:ring-focus"
                          >
                            Close
                          </MorphingDialogClose>
                        </MorphingDialogContent>
                      </MorphingDialogContainer>
                    </MorphingDialog>
                  ) : (
                    <div className="flex size-16 shrink-0 items-center justify-center rounded-sm border border-hairline bg-board text-ink-3">
                      <ImageIcon className="size-5" strokeWidth={1.5} aria-hidden />
                    </div>
                  )}

                  <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                    <div className="flex flex-wrap items-center gap-1.5">
                      <span className="truncate text-xs font-medium text-ink" title={item.path}>
                        {name}
                      </span>
                      <Pill className="bg-board px-1.5 py-0 text-xs text-ink-2">
                        {item.category}
                      </Pill>
                      {item.isPending && !approved && (
                        <Pill className="bg-pill-warn-bg px-1.5 py-0 text-xs text-pill-warn-fg">
                          Needs review
                        </Pill>
                      )}
                      {approved && (
                        <Pill className="bg-pill-active-bg px-1.5 py-0 text-xs text-pill-active-fg">
                          Approved
                        </Pill>
                      )}
                    </div>
                    <p className="truncate font-mono text-xs text-ink-3" title={item.path}>
                      {item.path}
                    </p>
                    <div className="flex flex-wrap items-center gap-1.5">
                      {projectId && !approved && (
                        <Button
                          variant="default"
                          size="xs"
                          disabled={approving}
                          onClick={() => void handleApproveArt(item)}
                          className="min-h-8 gap-1 px-2 text-xs active:opacity-80 max-md:min-h-11"
                        >
                          {approving ? (
                            <Loader2 className="size-3 motion-safe:animate-spin" strokeWidth={1.5} aria-hidden />
                          ) : (
                            <Check className="size-3" strokeWidth={1.5} aria-hidden />
                          )}
                          Approve
                        </Button>
                      )}
                      {onPrefillPrompt && (
                        <Button
                          variant="outline"
                          size="xs"
                          onClick={() =>
                            onPrefillPrompt(
                              `Redraw ${item.targetPath} keeping clean proportions, full canvas framing, and cel-shaded color balance.`,
                            )
                          }
                          className="min-h-8 gap-1 px-2 text-xs active:opacity-80 max-md:min-h-11"
                        >
                          <RefreshCw className="size-3" strokeWidth={1.5} aria-hidden />
                          Redraw
                        </Button>
                      )}
                    </div>
                  </div>
                </li>
              );
            })}
          </ul>
        </div>
      )}

      {/* 2. Live Playtest Card */}
      {playtest && (
        <div
          className="flex flex-col gap-2 rounded-sm border border-hairline bg-board/50 p-2"
          data-adaptive-card="playtest"
        >
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="inline-flex items-center gap-1.5 text-xs font-medium text-ink">
              <Play className="size-3.5 shrink-0 text-blue" strokeWidth={1.5} aria-hidden />
              Live playtest
              {playtest.runId && (
                <span className="font-mono text-xs font-normal text-ink-3">{playtest.runId}</span>
              )}
            </span>
            <div className="flex items-center gap-1.5">
              {playtest.consoleErrors !== null && (
                <Pill
                  className={`px-1.5 py-0 text-xs ${
                    playtest.consoleErrors > 0 ? "bg-pill-pro-bg text-pill-pro-fg" : "bg-surface text-ink-2"
                  }`}
                >
                  <span className="font-mono tabular-nums">{playtest.consoleErrors}</span> errors
                </Pill>
              )}
              {playtest.passed !== null && (
                <Pill
                  className={`gap-1 px-2 py-0 text-xs ${
                    playtest.passed ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-pro-bg text-pill-pro-fg"
                  }`}
                >
                  {playtest.passed ? (
                    <CircleCheck className="size-3" strokeWidth={1.5} aria-hidden />
                  ) : (
                    <CircleX className="size-3" strokeWidth={1.5} aria-hidden />
                  )}
                  {playtest.passed ? "Passed" : "Failed"}
                </Pill>
              )}
            </div>
          </div>

          {(playtest.coreLoop.move || playtest.coreLoop.act || playtest.coreLoop.hud) && (
            <div className="flex flex-wrap gap-1.5" aria-label="Core gameplay loop">
              {(["move", "act", "hud"] as const).map((k) => {
                const st = playtest.coreLoop[k];
                if (!st) return null;
                return (
                  <Pill key={k} className={`px-2 py-0.5 text-xs ${loopChipTone(st)}`}>
                    {k.toUpperCase()} {st}
                  </Pill>
                );
              })}
            </div>
          )}

          {playtest.captureUrl && !failedImages[playtest.captureUrl] && (
            <MorphingDialog transition={dialogSpring}>
              <MorphingDialogTrigger
                aria-label={`Open ${playtest.captureName || "playtest capture"}`}
                className="block w-full overflow-hidden rounded-sm border border-hairline bg-surface text-left outline-none focus-visible:ring-2 focus-visible:ring-focus"
              >
                <img
                  src={playtest.captureUrl}
                  alt={playtest.captureName || "Playtest capture"}
                  loading="eager"
                  fetchPriority="high"
                  onError={() => setFailedImages((p) => ({ ...p, [playtest.captureUrl!]: true }))}
                  className="aspect-video w-full object-cover"
                />
              </MorphingDialogTrigger>
              <MorphingDialogContainer>
                <MorphingDialogContent className="relative mx-3 flex max-h-[88dvh] w-full max-w-[900px] flex-col gap-3 rounded-dialog border border-hairline bg-surface p-4 text-ink shadow-lift">
                  <MorphingDialogTitle className="font-heading text-ui font-semibold">
                    {playtest.captureName || "Playtest capture"}
                  </MorphingDialogTitle>
                  <img
                    src={playtest.captureUrl}
                    alt={playtest.captureName || "Playtest capture"}
                    className="min-h-0 w-full flex-1 rounded-sm object-contain"
                  />
                  <MorphingDialogClose
                    aria-label="Close capture"
                    className="static inline-flex min-h-11 cursor-pointer items-center justify-center self-end rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none hover:bg-board focus-visible:ring-2 focus-visible:ring-focus"
                  >
                    Close
                  </MorphingDialogClose>
                </MorphingDialogContent>
              </MorphingDialogContainer>
            </MorphingDialog>
          )}
        </div>
      )}

      {/* 3. Creator Hub / Open Cloud Action Card */}
      {creatorHub && (
        <div
          className="flex flex-col gap-2 rounded-sm border border-hairline bg-board/50 p-2"
          data-adaptive-card="creator-hub"
        >
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="inline-flex items-center gap-1.5 text-xs font-medium text-ink">
              <Sparkles className="size-3.5 shrink-0 text-blue" strokeWidth={1.5} aria-hidden />
              {blockedLabel(creatorHub.blockedReason)}
            </span>
            {creatorHub.blockedReason && (
              <Pill className="bg-pill-warn-bg px-1.5 py-0 text-xs text-pill-warn-fg">
                Action needed
              </Pill>
            )}
          </div>

          {(creatorHub.universeId || creatorHub.placeId) && (
            <div className="flex flex-wrap gap-1.5">
              {creatorHub.universeId && (
                <Pill className="bg-surface px-2 py-0.5 text-xs text-ink-2">
                  Universe <span className="font-mono tabular-nums text-ink">{creatorHub.universeId}</span>
                </Pill>
              )}
              {creatorHub.placeId && (
                <Pill className="bg-surface px-2 py-0.5 text-xs text-ink-2">
                  Place <span className="font-mono tabular-nums text-ink">{creatorHub.placeId}</span>
                </Pill>
              )}
            </div>
          )}

          <div className="flex flex-wrap items-center gap-1.5">
            {onQuickCreatorHubSetup && (
              <Button
                variant="default"
                size="xs"
                disabled={setupBusy}
                onClick={() => void handleQuickSetup()}
                className="min-h-8 gap-1 px-2 text-xs max-md:min-h-11"
              >
                {setupBusy ? (
                  <Loader2 className="size-3 motion-safe:animate-spin" strokeWidth={1.5} aria-hidden />
                ) : (
                  <Sparkles className="size-3" strokeWidth={1.5} aria-hidden />
                )}
                Auto-setup from open browser
              </Button>
            )}
            {onOpenShip && (
              <Button
                variant="outline"
                size="xs"
                onClick={onOpenShip}
                className="min-h-8 px-2 text-xs max-md:min-h-11"
              >
                Open Ship
              </Button>
            )}
            {creatorHub.blockedReason && onRetry && (
              <Button
                variant="outline"
                size="xs"
                disabled={retryDisabled}
                onClick={onRetry}
                className="min-h-8 gap-1 px-2 text-xs max-md:min-h-11"
              >
                <RefreshCw className="size-3" strokeWidth={1.5} aria-hidden />
                Retry step
              </Button>
            )}
            {creatorHub.url && (
              <a
                href={creatorHub.url}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex min-h-8 items-center gap-1 rounded-sm px-2 text-xs font-medium text-blue outline-none hover:underline focus-visible:ring-2 focus-visible:ring-focus max-md:min-h-11"
              >
                Open Creator Hub
                <ExternalLink className="size-3" strokeWidth={1.5} aria-hidden />
              </a>
            )}
          </div>
        </div>
      )}

      {/* 4. Compact Collapsible Tool / File Activity Pills */}
      {activities.length > 0 && (
        <div
          className="rounded-sm border border-hairline bg-board/40 px-2 py-1"
          data-adaptive-card="activity"
        >
          <Expandable expandDirection="vertical" expandBehavior="push">
            {({ isExpanded }) => (
              <>
                <ExpandableTrigger
                  aria-label="Toggle tool and file activity"
                  className="flex min-h-8 w-full cursor-pointer items-center justify-between gap-2 text-left text-xs text-ink-2 outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-focus max-md:min-h-11"
                >
                  <span className="inline-flex min-w-0 items-center gap-1.5">
                    <Wrench className="size-3.5 shrink-0 text-ink-3" strokeWidth={1.5} aria-hidden />
                    <span className="truncate font-medium text-ink">Tools and files</span>
                    <Pill className="bg-surface px-1.5 py-0 text-xs text-ink-2">
                      <span className="font-mono tabular-nums">{activities.length}</span>
                    </Pill>
                  </span>
                  <ChevronDown
                    className={`size-3.5 shrink-0 text-ink-3 duration-(--dur-swap) ease-(--ease-house) motion-safe:transition-transform ${isExpanded ? "rotate-180" : ""}`}
                    strokeWidth={1.5}
                    aria-hidden
                  />
                </ExpandableTrigger>
                {isExpanded && (
                  <ul className="mt-1.5 flex flex-wrap gap-1.5 border-t border-hairline pt-1.5">
                    {activities.map((a, idx) => (
                      <li key={`${a.kind}-${a.detail}-${idx}`} className="max-w-full">
                        <Pill className="max-w-full gap-1 bg-surface px-2 py-0.5 text-xs text-ink-2">
                          {activityIcon(a.kind)}
                          <span className="shrink-0 font-medium text-ink">{a.label}</span>
                          <span className="truncate font-mono text-ink-3">{a.detail}</span>
                        </Pill>
                      </li>
                    ))}
                  </ul>
                )}
              </>
            )}
          </Expandable>
        </div>
      )}
    </div>
  );
}
