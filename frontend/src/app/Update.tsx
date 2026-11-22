import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { toast } from "sonner";
import { ChevronDown, ExternalLink, RefreshCw, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Expandable, ExpandableContent, ExpandableTrigger } from "@/components/ui/expandable";
import { RELEASES_URL, REPO_RELEASES_URL, normalizeSnap, post, read, safeHttpUrl, type Snap } from "./data";

const DISMISS_KEY = "openro-update-dismissed";
export const isLocalHost = () => ["localhost", "127.0.0.1", "[::1]"].includes(location.hostname);

/** Exact BANNER-FIX-BRIEF.md plain-English mapping; raw server text stays inside Details disclosure only. */
export function plainUpdateError(raw: string): string {
  if (/could not reach|Connection/i.test(raw)) return "Could not reach GitHub. Check your internet connection, then try again.";
  if (/checksum/i.test(raw)) return "The download did not pass its safety check, so it was not installed. Nothing changed. Try again in a few minutes.";
  if (/answered HTTP/i.test(raw)) return "GitHub had a problem. Nothing changed. Try again in a few minutes.";
  if (/only allowed from this machine/i.test(raw)) return "Updating works only on the computer OpenRO runs on. Open it there to update.";
  if (/does not run on this machine/i.test(raw)) return "The new version does not run on this computer, so it was not installed. Nothing changed.";
  return "The update did not finish. Nothing changed. Try again in a few minutes.";
}

function checkedWords(secs: number) {
  const mins = Math.max(0, Math.round((Date.now() - secs * 1000) / 60000));
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins} min ago`;
  return `${Math.round(mins / 60)} h ago`;
}

export function resultLine(s: Snap): string {
  if (s.mode === "off") return "Update checks are turned off on this OpenRO.";
  if (s.staged) return `Version ${s.staged} is installed. Restart to use it.`;
  if (s.available && s.latest) return `Version ${s.latest} is available.`;
  if (!s.checked_at) return s.current ? `OpenRO ${s.current} is running.` : "Not checked yet.";
  return `You are on the newest version (checked ${checkedWords(s.checked_at)}).`;
}

export function useUpdate(showUpdateRef?: RefObject<HTMLButtonElement | null>) {
  const [snap, setSnap] = useState<Snap | null>(null);
  const [dismissed, setDismissed] = useState(() => {
    try { return localStorage.getItem(DISMISS_KEY) || ""; } catch { return ""; }
  });
  const [busy, setBusy] = useState<"" | "apply" | "restart" | "check">("");
  const [error, setError] = useState<{ plain: string; raw: string; retry: () => void } | null>(null);
  const [returning, setReturning] = useState(false);

  const load = useCallback(() => {
    read("/api/update")
      .then((raw) => {
        const s = normalizeSnap(raw);
        setSnap(s);
        if (s.error) {
          setError((prev) => prev ?? { plain: plainUpdateError(s.error!), raw: s.error!, retry: () => void checkRef.current() });
        }
      })
      .catch(() => {});
  }, []);

  const checkRef = useRef<() => Promise<void>>(async () => {});

  const showAgain = useCallback(() => {
    try { localStorage.removeItem(DISMISS_KEY); } catch { /* ignore */ }
    setDismissed("");
  }, []);

  const check = useCallback(async () => {
    setBusy("check");
    setError(null);
    // Manual check clears any prior dismissal per BANNER-FIX-BRIEF item 2.
    showAgain();
    try {
      const raw = await post("/api/update/check", {});
      const s = normalizeSnap(raw);
      setSnap(s);
      if (s.error) {
        const plain = plainUpdateError(s.error);
        setError({ plain, raw: s.error, retry: () => void check() });
        toast.error(plain);
      } else {
        toast.success(resultLine(s));
      }
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e);
      const plain = plainUpdateError(raw);
      setError({ plain, raw, retry: () => void check() });
    }
    setBusy("");
  }, [showAgain]);

  checkRef.current = check;

  useEffect(() => {
    load();
    const t = setInterval(load, 30 * 60 * 1000);
    return () => clearInterval(t);
  }, [load]);

  const apply = useCallback(async () => {
    setBusy("apply");
    setError(null);
    try {
      const raw = await post<Record<string, unknown>>("/api/update/apply", {});
      if (raw && typeof raw === "object" && raw.ok === false) {
        throw new Error(typeof raw.error === "string" ? raw.error : "apply failed");
      }
      const next = normalizeSnap(raw);
      const applied = typeof raw?.applied === "string" ? raw.applied : null;
      setSnap((prev) => ({
        ...prev,
        ...next,
        staged: next.staged || applied || next.latest || prev?.latest || null,
      }));
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e);
      setError({ plain: plainUpdateError(raw), raw, retry: () => void apply() });
    }
    setBusy("");
  }, []);

  const restart = useCallback(async () => {
    setBusy("restart");
    setError(null);
    try {
      const raw = await post<{ ok?: boolean; error?: string }>("/api/update/restart", {});
      if (raw && raw.ok === false) throw new Error(raw.error || "restart refused");
    } catch (e) {
      if (!(e instanceof TypeError)) {
        const raw = e instanceof Error ? e.message : String(e);
        setError({ plain: plainUpdateError(raw), raw, retry: () => void restart() });
        setBusy("");
        return;
      }
    }
    setReturning(true);
    const started = Date.now();
    const tick = async () => {
      if (Date.now() - started > 60000) {
        setReturning(false);
        setBusy("");
        setError({
          plain: "OpenRO did not come back within a minute. Start OpenRO again, then reload this page.",
          raw: "",
          retry: () => void restart(),
        });
        return;
      }
      try {
        const r = await fetch("/api/version", { cache: "no-store" });
        if (r.ok) {
          location.reload();
          return;
        }
      } catch { /* still restarting */ }
      setTimeout(tick, 1000);
    };
    setTimeout(tick, 1000);
  }, []);

  const dismiss = useCallback((key: string) => {
    if (key) {
      try { localStorage.setItem(DISMISS_KEY, key); } catch { /* ignore */ }
      setDismissed(key);
    }
    setError(null);
    // Focus "Show update" button after Dismiss per BANNER-FIX-BRIEF item 4.
    setTimeout(() => {
      showUpdateRef?.current?.focus();
    }, 40);
  }, [showUpdateRef]);

  const hasAvailable = !!snap?.available && !!snap?.latest;
  const isDismissedAvailable = hasAvailable && !snap?.staged && dismissed === snap!.latest;
  const show =
    !!snap?.staged ||
    (hasAvailable && dismissed !== snap!.latest) ||
    !!error ||
    busy === "apply" ||
    busy === "restart" ||
    returning;

  return {
    snap,
    busy,
    error,
    returning,
    check,
    apply,
    restart,
    dismiss,
    showAgain,
    show,
    dismissed,
    isDismissedAvailable,
    canCheck: isLocalHost() && snap?.mode !== "off",
  };
}

export function UpdateStrip({ u }: { u: ReturnType<typeof useUpdate> }) {
  const statusRef = useRef<HTMLParagraphElement>(null);
  const restartRef = useRef<HTMLButtonElement>(null);
  const wasApplyingRef = useRef(false);

  const s = u.snap;
  const local = isLocalHost();
  const staged = s?.staged;
  const busy = u.busy === "apply" || u.busy === "restart" || u.returning;
  const failed = !!u.error;
  const canApply = local && !!s?.can_apply && !staged && !failed && !busy;
  const whatsNewHref = safeHttpUrl(s?.release_url) || (s?.latest ? RELEASES_URL + s.latest : REPO_RELEASES_URL);

  // BANNER-FIX-BRIEF item 4: focus status while applying, then Restart now when staged arrives.
  useEffect(() => {
    if (u.busy === "apply") {
      wasApplyingRef.current = true;
      statusRef.current?.focus();
    } else if (wasApplyingRef.current && staged) {
      wasApplyingRef.current = false;
      restartRef.current?.focus();
    }
  }, [u.busy, staged]);

  const headline = busy
    ? (u.busy === "restart" || u.returning ? "Restarting OpenRO…" : "Downloading and checking update…")
    : failed
      ? u.error!.plain
      : staged
        ? `Version ${staged} is installed. Restart to use it.`
        : `Version ${s?.latest ?? ""} is available.`;

  const subline = busy || failed || staged
    ? ""
    : canApply
      ? "Update now downloads it, checks it, and replaces this app. Restart to start using it."
      : !local
        ? "To update, open OpenRO on the computer it runs on."
        : s?.dev_build
          ? "This is a developer copy, so it does not update itself. Install a released version to get updates."
          : "Automatic install is turned off on this copy.";

  return (
    <section
      aria-label="OpenRO update"
      className="mx-3 mt-3 flex flex-col gap-2 rounded-card border border-hairline bg-surface p-3 text-sm md:mx-6"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <p ref={statusRef} tabIndex={-1} role="status" aria-live="polite" className="font-medium text-ink outline-none [overflow-wrap:anywhere]">
            {headline}
          </p>
          {subline && <p className="mt-1 text-sm text-ink-2 [overflow-wrap:anywhere]">{subline}</p>}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {!busy && !failed && (s?.latest || staged) && (
            <a
              href={whatsNewHref}
              target="_blank"
              rel="noreferrer"
              className="inline-flex min-h-11 items-center gap-1 rounded-lg border border-hairline bg-board px-3 text-sm font-medium text-ink outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-track md:min-h-8"
            >
              What&apos;s new
              <ExternalLink className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
            </a>
          )}
          {!busy && failed && (
            <Button variant="outline" onClick={() => u.error?.retry()} className="max-md:min-h-11">
              <RefreshCw className="size-3.5" strokeWidth={1.5} aria-hidden />
              Try again
            </Button>
          )}
          {!busy && !failed && staged && (
            <Button ref={restartRef} variant="default" onClick={() => void u.restart()} className="max-md:min-h-11">
              Restart now
            </Button>
          )}
          {canApply && (
            <Button
              variant="default"
              onClick={() => {
                statusRef.current?.focus();
                void u.apply();
              }}
              className="max-md:min-h-11"
            >
              Update now
            </Button>
          )}
          {!busy && !staged && (
            <Button
              variant="ghost"
              size="icon"
              aria-label="Dismiss update"
              onClick={() => u.dismiss(s?.latest || "error")}
              className="max-md:size-11"
            >
              <X className="size-4" strokeWidth={1.5} aria-hidden />
            </Button>
          )}
        </div>
      </div>

      {failed && u.error?.raw && (
        <Expandable expandDirection="vertical" expandBehavior="push">
          {({ isExpanded }) => (
            <div className="border-t border-hairline pt-2">
              <ExpandableTrigger
                aria-label={isExpanded ? "Hide update error details" : "Show update error details"}
                className="inline-flex min-h-11 items-center gap-1 rounded-sm px-2 text-xs font-medium text-ink-2 outline-none hover:bg-board hover:text-ink md:min-h-7"
              >
                <span>Details</span>
                <ChevronDown className={`size-3.5 transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`} strokeWidth={1.5} aria-hidden />
              </ExpandableTrigger>
              <ExpandableContent preset="fade">
                <pre className="mt-1.5 whitespace-pre-wrap break-all rounded-sm bg-board p-2.5 font-mono text-xs text-ink-2">{u.error?.raw}</pre>
              </ExpandableContent>
            </div>
          )}
        </Expandable>
      )}

      {!failed && !busy && s?.notes && s.notes.trim().length > 0 && (
        <Expandable expandDirection="vertical" expandBehavior="push">
          {({ isExpanded }) => (
            <div className="border-t border-hairline pt-2">
              <ExpandableTrigger
                aria-label={isExpanded ? "Hide release notes" : "Show release notes"}
                className="inline-flex min-h-11 items-center gap-1 rounded-sm px-2 text-xs font-medium text-ink-2 outline-none hover:bg-board hover:text-ink md:min-h-7"
              >
                <span>Release notes</span>
                <ChevronDown className={`size-3.5 transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`} strokeWidth={1.5} aria-hidden />
              </ExpandableTrigger>
              <ExpandableContent preset="fade">
                <p className="mt-1.5 whitespace-pre-wrap break-words rounded-sm bg-board p-2.5 text-sm text-ink-2">{s.notes}</p>
              </ExpandableContent>
            </div>
          )}
        </Expandable>
      )}
    </section>
  );
}
