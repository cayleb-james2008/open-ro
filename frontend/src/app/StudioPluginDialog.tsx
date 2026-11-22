import { useState } from "react";
import { toast } from "sonner";
import { AtSign, Check, Download, Plug, RefreshCw, Sparkles, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Pill, PillIndicator } from "@/components/kibo-ui/pill";
import {
  MorphingDialog,
  MorphingDialogClose,
  MorphingDialogContainer,
  MorphingDialogContent,
  MorphingDialogDescription,
  MorphingDialogTitle,
} from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import { post, type StudioPluginStatus } from "./data";

export function StudioPluginDialog({
  open,
  onOpenChange,
  status,
  v0100Ready,
  onRefreshStatus,
  onAttachSelection,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  status: StudioPluginStatus;
  v0100Ready: boolean;
  onRefreshStatus: () => Promise<void>;
  onAttachSelection: (paths: string[]) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [installMsg, setInstallMsg] = useState<string | null>(null);

  const handleInstall = async () => {
    if (!v0100Ready || busy) {
      toast.info("1-click Studio Plugin install requires OpenRO v0.10.0.");
      return;
    }
    setBusy(true);
    setInstallMsg(null);
    try {
      const res = await post<{ ok?: boolean; install_path?: string; message?: string; error?: string }>(
        "/api/studio/plugin/install",
        {},
      );
      if (res.ok) {
        const msg = res.install_path
          ? `Installed OpenRoCompanion.lua to ${res.install_path}`
          : res.message || "Installed OpenRoCompanion.lua to Roblox Studio Plugins.";
        setInstallMsg(msg);
        toast.success(msg);
        await onRefreshStatus();
      } else {
        const msg = res.error || res.message || "Could not locate Roblox Studio Plugins directory automatically — download OpenRoCompanion.lua below.";
        setInstallMsg(msg);
        toast.info(msg);
      }
    } catch {
      toast.error("Could not reach /api/studio/plugin/install.");
    } finally {
      setBusy(false);
    }
  };

  const handleDownload = async () => {
    try {
      const r = await fetch("/api/studio/plugin/download", { cache: "no-store" });
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      const lua = await r.text();
      const blob = new Blob([lua], { type: "text/plain;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "OpenRoCompanion.lua";
      document.body.appendChild(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      toast.success("Downloaded OpenRoCompanion.lua");
    } catch {
      toast.error("Could not download OpenRoCompanion.lua.");
    }
  };

  return (
    <MorphingDialog transition={dialogSpring} open={open} onOpenChange={onOpenChange}>
      <MorphingDialogContainer>
        <MorphingDialogContent className="relative mx-3 flex max-h-[84dvh] w-full max-w-[560px] flex-col rounded-dialog border border-hairline bg-surface text-ink shadow-lift">
          <div data-studio-plugin-dialog className="flex min-h-0 flex-1 flex-col">
            <div className="flex items-center justify-between gap-2 border-b border-hairline px-4 py-3">
              <div className="flex min-w-0 items-center gap-2">
                <Plug className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                <MorphingDialogTitle className="text-ui font-medium text-ink">
                  Roblox Studio Companion Plugin
                </MorphingDialogTitle>
                <Pill
                  className={
                    status.connected
                      ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg"
                      : "bg-board px-2 py-0.5 text-xs text-ink-2"
                  }
                >
                  <PillIndicator variant={status.connected ? "success" : "info"} pulse={status.connected} />
                  {status.connected ? "Plugin Live" : "Standby"}
                </Pill>
              </div>
              <MorphingDialogClose
                aria-label="Close Studio Plugin dialog"
                className="static inline-flex size-11 flex-none cursor-pointer items-center justify-center rounded-lg border border-hairline bg-surface text-ink-2 outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-board hover:text-ink md:size-8"
              >
                <X className="size-4" strokeWidth={1.5} aria-hidden />
              </MorphingDialogClose>
            </div>

            <MorphingDialogDescription className="sr-only">
              Install or download the OpenRoCompanion.lua Roblox Studio plugin for live Explorer sync, Selection tracking, and direct ChangeHistoryService script editing.
            </MorphingDialogDescription>

            <div className="flex flex-col gap-3 overflow-y-auto p-4">
              <p className="text-xs leading-relaxed text-ink-2">
                Connect any open Roblox Studio window directly to OpenRO via <code className="font-mono text-ink">OpenRoCompanion.lua</code> (token-authenticated loopback bridge using <code className="font-mono text-ink">HttpService</code>, <code className="font-mono text-ink">Selection</code>, and <code className="font-mono text-ink">ChangeHistoryService</code>).
              </p>

              <div className="grid gap-2 rounded-card border border-hairline bg-board/45 p-3 text-xs sm:grid-cols-2">
                <div>
                  <span className="text-ink-3">Studio Session</span>
                  <div className="mt-0.5 font-medium text-ink">
                    {status.connected ? status.place_name || status.studio_name || "Connected Studio" : "Not connected yet"}
                  </div>
                </div>
                <div>
                  <span className="text-ink-3">Plugin Status</span>
                  <div className="mt-0.5 flex items-center gap-1.5 font-medium text-ink">
                    {status.installed ? (
                      <>
                        <Check className="size-3.5 text-pill-active-fg" strokeWidth={2} aria-hidden />
                        <span>Installed locally</span>
                      </>
                    ) : (
                      <span>Not installed in Plugins folder</span>
                    )}
                  </div>
                </div>
                {status.install_path && (
                  <div className="sm:col-span-2">
                    <span className="text-ink-3">Plugins Path</span>
                    <div className="mt-0.5 truncate font-mono text-ink">{status.install_path}</div>
                  </div>
                )}
              </div>

              {/* Live Selection from Connected Roblox Studio */}
              <div className="flex flex-col gap-2 rounded-card border border-hairline bg-board/45 p-3">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="flex items-center gap-1.5">
                    <span className="text-xs font-medium text-ink">Live Studio Selection</span>
                    <Badge className="border-transparent bg-surface px-1.5 py-0.5 font-mono text-xs text-ink-2">
                      {status.selection.length}
                    </Badge>
                  </div>
                  {status.selection.length > 0 && (
                    <Button
                      variant="outline"
                      size="xs"
                      onClick={() => {
                        onAttachSelection(status.selection);
                        onOpenChange(false);
                      }}
                      className="bg-surface max-md:min-h-11"
                    >
                      <AtSign className="size-3" strokeWidth={1.5} aria-hidden />
                      Attach Selection to Copilot
                    </Button>
                  )}
                </div>
                {status.selection.length === 0 ? (
                  <p className="text-xs text-ink-2">
                    Select parts or scripts inside Roblox Studio and they appear here for 1-click Copilot context.
                  </p>
                ) : (
                  <div className="flex flex-wrap gap-1.5">
                    {status.selection.map((p) => (
                      <Pill key={p} className="bg-surface px-2 py-0.5 font-mono text-xs text-ink">
                        @{p}
                      </Pill>
                    ))}
                  </div>
                )}
              </div>

              {installMsg && (
                <p className="rounded-sm border border-hairline bg-board/50 px-3 py-2 text-xs text-ink">
                  {installMsg}
                </p>
              )}

              <div className="flex flex-wrap items-center justify-between gap-2 pt-1">
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={busy}
                  onClick={() => void onRefreshStatus()}
                  className="max-md:min-h-11"
                >
                  <RefreshCw className="size-3.5" strokeWidth={1.5} aria-hidden />
                  Refresh Status
                </Button>
                <div className="flex flex-wrap items-center gap-1.5">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void handleDownload()}
                    className="bg-surface max-md:min-h-11"
                  >
                    <Download className="size-3.5" strokeWidth={1.5} aria-hidden />
                    Download OpenRoCompanion.lua
                  </Button>
                  <Button
                    variant="default"
                    size="sm"
                    disabled={busy}
                    aria-label="Install OpenRoCompanion.lua to Roblox Studio Plugins"
                    onClick={() => void handleInstall()}
                    className="max-md:min-h-11"
                  >
                    <Sparkles className="size-3.5" strokeWidth={1.5} aria-hidden />
                    {busy ? "Installing…" : "1-Click Install"}
                  </Button>
                </div>
              </div>
            </div>
          </div>
        </MorphingDialogContent>
      </MorphingDialogContainer>
    </MorphingDialog>
  );
}
