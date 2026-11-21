import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type MouseEvent } from "react";
import { MotionConfig } from "motion/react";
import { Toaster, toast } from "sonner";
import { Drawer } from "vaul";
import { Menu, Plus, Undo2, Send, Sparkles, Sun, Moon, PanelRight, RefreshCw } from "lucide-react";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { AnimatedBackground } from "@/components/ui/animated-background";
import { MorphingDialog, MorphingDialogTrigger, MorphingDialogContainer, MorphingDialogContent, MorphingDialogTitle, MorphingDialogDescription, MorphingDialogClose } from "@/components/ui/morphing-dialog";
import { Pill } from "@/components/kibo-ui/pill";
import { open } from "@/motion";
import { read, post, SEND_FAILED, type Chat, type ChatList, type Idea, type Place } from "./app/data";
import { ThreadBody } from "./app/Thread";
import { Quest } from "./app/Quest";
import { Expandable, ExpandableContent, ExpandableTrigger } from "@/components/ui/expandable";
import { PartsShelf, StudioChip } from "./app/Shelf";
import { Key } from "./app/Key";
import { useUpdate, UpdateStrip } from "./app/Update";

type Pending = { message?: string; build?: string; undo?: boolean };
type Notice = { text: string; bad?: boolean; user?: boolean };
type ShellWindow = Window & { openroSetTheme?: (t: string) => void };

const noticeKey = (id: string) => "openro-notices:" + id;
const draftKey = (id: string) => "openro-draft:" + id;
function slotRead<T>(key: string, fallback: T): T {
  try { const v = sessionStorage.getItem(key); return v === null ? fallback : (JSON.parse(v) as T); } catch { return fallback; }
}
function slotWrite(key: string, value: unknown) {
  try {
    if (value === "" || (Array.isArray(value) && !value.length)) sessionStorage.removeItem(key);
    else sessionStorage.setItem(key, JSON.stringify(value));
  } catch { /* storage refused: the notice still shows while the chat is open */ }
}
const useMedia = (q: string) => {
  const [m, setM] = useState(() => window.matchMedia(q).matches);
  useEffect(() => {
    const mq = window.matchMedia(q);
    const on = () => setM(mq.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, [q]);
  return m;
};

/** Server-filled values from the shell (the Rust server replaces the placeholders before serving). */
function readShell() {
  const s = document.getElementById("server-state");
  return {
    gameName: s?.dataset.gameName || "",
    canBuild: s?.dataset.canBuild === "true",
    studio: s?.dataset.studio === "true",
    bannerHtml: document.getElementById("server-banners")?.innerHTML || "",
  };
}

export function App() {
  const shell = useMemo(readShell, []);
  const [theme, setThemeState] = useState<"light" | "dark">(() => (document.documentElement.dataset.theme === "dark" ? "dark" : "light"));
  const setTheme = (t: "light" | "dark") => {
    setThemeState(t);
    document.documentElement.setAttribute("data-theme", t);
    (window as ShellWindow).openroSetTheme?.(t);
  };
  const phone = useMedia("(max-width: 759px)");
  const narrow = useMedia("(max-width: 1099px)");

  const [chats, setChats] = useState<ChatList | null>(null);
  const [lines, setLines] = useState<string[]>([]);
  const [threadError, setThreadError] = useState(false);
  const [ideas, setIdeas] = useState<Idea[]>([]);
  const [builtTitles, setBuiltTitles] = useState<string[]>([]);
  const [place, setPlace] = useState<Place | null | "error">(null);
  const [inflight, setInflight] = useState<Record<string, Pending>>({});
  const [, bump] = useState(0);
  const [text, setText] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const [shelfOpen, setShelfOpen] = useState(false);
  const [shelfCollapsed, setShelfCollapsed] = useState(false);
  const [studioOk, setStudioOk] = useState(shell.studio);
  const [modelOk, setModelOk] = useState(shell.canBuild || !shell.studio);
  const [version, setVersion] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const menuKeyRef = useRef<HTMLButtonElement>(null);
  const update = useUpdate();

  const activeId = chats?.active ?? "";
  const notices: Notice[] = activeId ? slotRead<Notice[]>(noticeKey(activeId), []) : [];
  const busy = Object.keys(inflight).length > 0;
  const building = activeId ? inflight[activeId] : undefined;
  const canBuild = studioOk && modelOk;
  const builtCount = place && place !== "error" ? Object.keys(place.scripts || {}).length + Object.keys(place.instances || {}).length : null;
  const undoState = !chats ? "loading" : busy || place === null ? "busy" : place === "error" ? "unknown" : !studioOk ? "nostudio" : !chats.can_undo ? "nothing" : "ok";
  const undoReason = undoState === "nostudio" ? "Connect Studio to see what is built" : undoState === "nothing" || (builtCount === 0 && undoState !== "ok") ? "Nothing built yet" : "";
  const canUndo = undoState === "ok";
  const activeTitle = chats?.chats.find((c) => c.id === activeId)?.title || "New chat";

  const addNotice = useCallback((chatId: string, n: Notice) => {
    slotWrite(noticeKey(chatId), [...slotRead<Notice[]>(noticeKey(chatId), []), n]);
    bump((x) => x + 1);
  }, []);
  const begin = (chatId: string, p: Pending) => {
    slotWrite(noticeKey(chatId), "");
    setInflight((s) => ({ ...s, [chatId]: p }));
  };
  const end = (chatId: string) => setInflight((s) => {
    const next = { ...s };
    delete next[chatId];
    return next;
  });

  const refreshChats = useCallback(async () => {
    try { setChats(await read<ChatList>("/api/chats")); } catch { /* the list keeps its last good state */ }
  }, []);
  const refreshThread = useCallback(async () => {
    try {
      const data = await read<{ thread: string[] }>("/api/thread");
      setLines(data.thread);
      setThreadError(false);
    } catch {
      setThreadError(true);
    }
  }, []);
  const refreshIdeas = useCallback(async () => {
    try {
      const data = await post<{ ideas?: Idea[]; pitched_new?: boolean; built_titles?: string[] }>("/api/ideas", {});
      setIdeas(data.ideas || []);
      setBuiltTitles(data.built_titles || []);
      if (data.pitched_new) await refreshThread();
    } catch { /* the cards keep their last good state */ }
  }, [refreshThread]);
  const refreshPlace = useCallback(async () => {
    try { setPlace(await read<Place>("/api/place-tree")); } catch { setPlace("error"); }
  }, []);
  const refreshAll = useCallback(async () => {
    await refreshChats();
    await refreshThread();
    await refreshIdeas();
    await refreshPlace();
  }, [refreshChats, refreshThread, refreshIdeas, refreshPlace]);

  useEffect(() => {
    void refreshAll();
    read<{ version: string }>("/api/version").then((v) => setVersion(v.version)).catch(() => {});
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const threadRef = useRef<HTMLDivElement>(null);
  const cantBuildWhy = !studioOk ? "Building needs Roblox Studio open with the OpenRO connection. Use Check again at the top once it is open."
    : !modelOk ? "Building needs a model key. Ask whoever runs OpenRO to add one, then restart it." : "";
  const lastBad = [...notices].reverse().find((n) => n.bad)?.text ?? null;

  const checkStudio = async () => {
    try {
      const s = await read<{ studio_connected: boolean; model_configured: boolean }>("/api/status");
      if (s.studio_connected && !studioOk) toast.success("Studio reconnected");
      setStudioOk(s.studio_connected);
      setModelOk(s.model_configured);
    } catch {
      toast.error("Could not reach the OpenRO server.");
    }
  };

  const switchChat = async (id: string) => {
    if (id === activeId) return;
    try {
      const r = await fetch("/api/chats/select", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ id }) });
      if (!r.ok) throw new Error("select");
    } catch {
      addNotice(activeId || id, { text: "Could not switch chats. The server did not answer.", bad: true });
      return;
    }
    if (activeId && text.trim()) slotWrite(draftKey(activeId), text);
    setText(slotRead<string>(draftKey(id), ""));
    slotWrite(draftKey(id), "");
    setMenuOpen(false);
    await refreshAll();
  };

  const newChat = async () => {
    try {
      const r = await fetch("/api/chats", { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" });
      if (!r.ok) throw new Error("new chat");
    } catch {
      toast.error("Could not reach the OpenRO server.");
      return;
    }
    setMenuOpen(false);
    await refreshAll();
    inputRef.current?.focus();
  };

  const buildIdea = async (idea: Idea) => {
    const chatId = activeId;
    begin(chatId, { build: idea.title });
    let failure: string | null = null;
    try {
      const data = await post<{ ok?: boolean; error?: string }>("/api/ideas/build", { id: idea.id });
      if (!data.ok) failure = data.error || "The build did not finish.";
    } catch {
      failure = "Could not reach the OpenRO server.";
    }
    end(chatId);
    if (failure) addNotice(chatId, { text: failure, bad: true });
    await refreshAll();
    // The tile that was pressed may have been re-rendered: focus goes back to its key, else to the box.
    await new Promise((r) => setTimeout(r, 60));
    const a = document.activeElement;
    if (!a || a === document.body) {
      const t = document.querySelector<HTMLButtonElement>(`[data-focus-key="idea:${CSS.escape(idea.title)}"]`);
      if (t && !t.disabled) t.focus(); else inputRef.current?.focus();
    }
  };

  const send = async (e: FormEvent) => {
    e.preventDefault();
    const message = text.trim();
    if (!message) return;
    const chatId = activeId;
    setText("");
    begin(chatId, { message });
    let failure: string | null = null;
    try {
      const r = await fetch("/api/chat", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ message }) });
      const raw = await r.text();
      let data: { ok?: boolean; error?: string } = {};
      try { data = JSON.parse(raw); } catch { /* not JSON: a proxy or a crash answered */ }
      if (r.status === 403 || r.status === 413 || r.status === 415) failure = data.error || "OpenRO did not take that message.";
      else if (!r.ok && !((r.status === 502 || r.status === 503) && data.ok === false)) failure = SEND_FAILED;
    } catch {
      failure = SEND_FAILED;
    }
    end(chatId);
    if (failure) {
      addNotice(chatId, { text: message, user: true });
      addNotice(chatId, { text: failure, bad: true });
      setText(message);
    }
    await refreshAll();
    inputRef.current?.focus();
  };

  const undoNow = async () => {
    const chatId = activeId;
    begin(chatId, { undo: true });
    let msg: string | null = null;
    let bad = false;
    try {
      const data = await post<{ ok?: boolean; restored?: unknown[]; error?: string }>("/api/revert", undefined);
      if (data.ok) msg = `Undid the last build. ${data.restored?.length ?? 0} change(s) were put back.`;
      else if (data.error === "nothing to revert") msg = "Nothing to undo. No build is waiting to be undone in this chat.";
      else { msg = data.error || "The undo did not finish."; bad = true; }
    } catch {
      msg = "Could not reach the OpenRO server.";
      bad = true;
    }
    end(chatId);
    if (msg) {
      addNotice(chatId, { text: msg, bad });
      if (bad) toast.error(msg); else toast.success(msg);
    }
    await refreshAll();
  };

  // The confirm's "Yes, undo it" is marked in the markup; the dialog's own close button keeps the Keep it path.
  // Escape inside the confirm returns focus to the Undo key (the dialog keeps the Tab cycle itself).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !document.activeElement?.closest("[data-undo-dialog]")) return;
      setTimeout(() => {
        const undoKey = [...document.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Undo");
        undoKey?.focus();
      }, 80);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);
  const onUndoClick = (e: MouseEvent<HTMLDivElement>) => {
    if ((e.target as HTMLElement).closest("[data-undo-yes]")) void undoNow();
  };
  // The server's notices: a dismiss button hides its own banner.
  const onBannerClick = (e: MouseEvent<HTMLDivElement>) => {
    const banner = (e.target as HTMLElement).closest("[data-dismiss]")?.closest<HTMLElement>(".banner");
    if (banner) banner.hidden = true;
  };

  const rail = (
    <nav aria-label="Chats and settings" className="flex h-full min-h-0 flex-col gap-3 bg-surface p-4 text-ink">
      <div className="flex min-w-0 items-center gap-3 px-1 pb-2 pt-1">
        <span className="grid size-10 flex-none place-items-center rounded-card bg-ink-button text-ink-button-fg"><Sparkles className="size-5" strokeWidth={1.5} aria-hidden /></span>
        <div className="min-w-0">
          <h1 className="font-heading text-title tracking-display font-semibold leading-tight">OpenRO</h1>
          <div className="truncate text-sm text-ink-2">{shell.gameName}</div>
        </div>
      </div>
      <Key onClick={newChat} className="w-full justify-start">
        <Plus className="size-4" strokeWidth={1.5} aria-hidden />New chat
      </Key>
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        <AnimatedBackground
          defaultValue={activeId}
          onValueChange={(id) => id && id !== activeId && switchChat(id)}
          className="rounded-card bg-blue-soft"
          transition={open}
          enableHover={false}
        >
          <ul id="chat-list" className="flex flex-col gap-1" aria-label="Your chats">
            {(chats?.chats || []).map((c: Chat) => (
              <li key={c.id} className="relative">
                <button
                  type="button"
                  data-id={c.id}
                  aria-current={c.id === activeId ? "true" : undefined}
                  onClick={() => switchChat(c.id)}
                  className="chat-btn flex min-h-11 w-full cursor-pointer items-center gap-2 rounded-card px-3 text-left text-ui text-ink-2 outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-focus data-[checked=true]:font-medium data-[checked=true]:text-ink"
                >
                  <span className="min-w-0 flex-1 truncate">{c.title || "New chat"}</span>
                  {inflight[c.id] && <span className="flex-none text-sm text-ink-2">working</span>}
                </button>
              </li>
            ))}
          </ul>
        </AnimatedBackground>
      </div>
      <div className="flex flex-col gap-3 border-t border-hairline pt-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-sm text-ink-2">Appearance</span>
          <SegmentedControl aria-label="Appearance">
            <SegmentedControlItem data-checked={theme === "light" ? "" : undefined} aria-pressed={theme === "light"} onClick={() => setTheme("light")} className="!h-11 gap-1.5">
              <Sun className="size-4" strokeWidth={1.5} aria-hidden />Light
            </SegmentedControlItem>
            <SegmentedControlItem data-checked={theme === "dark" ? "" : undefined} aria-pressed={theme === "dark"} onClick={() => setTheme("dark")} className="!h-11 gap-1.5">
              <Moon className="size-4" strokeWidth={1.5} aria-hidden />Dark
            </SegmentedControlItem>
          </SegmentedControl>
        </div>
        <div className="flex min-h-11 items-center justify-between gap-2 text-sm text-ink-2">
          <span className="min-w-0 truncate">OpenRO {version} · {update.busy === "check" ? "Checking…" : update.show && update.snap?.staged ? "Installed, restart to use it" : update.show ? "Update available" : update.snap?.checked_at ? "Up to date" : "Not checked yet"}</span>
          {update.show && update.dismissed && update.snap?.available && (
            <button type="button" onClick={() => update.showAgain()} className="inline-flex min-h-11 flex-none cursor-pointer items-center rounded-sm px-2 text-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-focus">Show update</button>
          )}
          {update.canCheck && (
            <button type="button" aria-label="Check for updates" onClick={() => void update.check()} disabled={update.busy !== ""} className="grid size-11 flex-none cursor-pointer place-items-center rounded-sm text-icon outline-none transition-[color,background-color] hover:bg-track hover:text-ink focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default">
              <RefreshCw className="size-4" strokeWidth={1.5} aria-hidden />
            </button>
          )}
        </div>
      </div>
    </nav>
  );

  return (
    <MotionConfig reducedMotion="user">
      <div className="flex h-dvh min-h-0 w-full overflow-hidden text-ink" data-app="openro">
        <a className="skip" href="#composer-input">Skip to message box</a>
        {!phone && <div className="w-[264px] flex-none border-r border-hairline">{rail}</div>}
        <Drawer.Root open={menuOpen && phone} onOpenChange={setMenuOpen} direction="left" autoFocus>
          <Drawer.Portal>
            <Drawer.Overlay className="fixed inset-0 z-40 bg-ink/20" />
            <Drawer.Content onCloseAutoFocus={(e) => { e.preventDefault(); menuKeyRef.current?.focus(); }} className="fixed inset-y-0 left-0 z-50 w-[min(86vw,300px)] outline-none">
              <Drawer.Title className="sr-only">Chats and settings</Drawer.Title>
              {rail}
            </Drawer.Content>
          </Drawer.Portal>
        </Drawer.Root>

        <main className="flex min-w-0 flex-1 flex-col">
          <header className="sticky top-0 z-10 flex min-h-14 flex-none flex-wrap items-center gap-x-3 gap-y-2 border-b border-hairline bg-canvas px-4 py-2 md:px-6">
            {phone && (
              <button ref={menuKeyRef} type="button" onClick={() => setMenuOpen(true)} aria-label="Open the menu" aria-expanded={menuOpen} className="grid size-11 flex-none cursor-pointer place-items-center rounded-sm text-icon outline-none hover:bg-track focus-visible:ring-2 focus-visible:ring-focus">
                <Menu className="size-5" strokeWidth={1.5} aria-hidden />
              </button>
            )}
            <h2 className="min-w-0 flex-1 truncate font-heading text-title tracking-display font-semibold">{activeTitle}</h2>
            {narrow && (
              <Drawer.Root open={shelfOpen} onOpenChange={setShelfOpen} autoFocus>
                <Drawer.Trigger asChild>
                  <button type="button" aria-label="In your game" className="grid size-11 flex-none cursor-pointer place-items-center rounded-card border border-hairline bg-surface outline-none hover:bg-track focus-visible:ring-2 focus-visible:ring-focus">
                    <PanelRight className="size-5" strokeWidth={1.5} aria-hidden />
                  </button>
                </Drawer.Trigger>
                <Drawer.Portal>
                  <Drawer.Overlay className="fixed inset-0 z-40 bg-ink/20" />
                  <Drawer.Content className="fixed inset-x-0 bottom-0 z-50 flex max-h-[80dvh] flex-col rounded-t-panel border-t border-hairline bg-board outline-none">
                    <Drawer.Handle className="mx-auto mt-3 h-1.5 w-10 rounded-full bg-track" />
                    <Drawer.Title className="sr-only">In your game</Drawer.Title>
                    <div className="min-h-0 flex-1 overflow-y-auto">
                      <PartsShelf place={place} error={place === "error"} />
                    </div>
                  </Drawer.Content>
                </Drawer.Portal>
              </Drawer.Root>
            )}
            <div className="order-last flex w-full flex-wrap items-center gap-2 sm:order-none sm:w-auto">
              {chats && <StudioChip connected={studioOk} />}
              {chats && !studioOk && (
                <button type="button" onClick={() => void checkStudio()} className="inline-flex min-h-11 cursor-pointer items-center gap-1.5 rounded-sm px-2 text-sm text-ink outline-none hover:bg-track focus-visible:ring-2 focus-visible:ring-focus">
                  <RefreshCw className="size-4" strokeWidth={1.5} aria-hidden />Check again
                </button>
              )}
            </div>
          </header>

          <div className="server-banners" onClick={onBannerClick} dangerouslySetInnerHTML={{ __html: shell.bannerHtml }} />
          {update.show && <UpdateStrip u={update} />}

          <div id="thread" ref={threadRef} aria-live="polite" aria-busy={!chats} className="min-h-0 flex-1 overflow-y-auto">
            <div className="mx-auto flex w-full max-w-[760px] flex-col gap-4 p-4 md:p-6">
              {threadError ? (
                <div className="card-stock flex flex-col items-start gap-3 p-4">
                  <strong className="font-heading font-semibold text-pill-pro-fg">Could not load the conversation.</strong>
                  <p className="text-ink">The OpenRO server did not answer. Try again, or reload the page.</p>
                  <Key onClick={() => void refreshAll()}>Try again</Key>
                </div>
              ) : (
                <>
                  <Quest lines={lines} ideas={ideas} builtTitles={builtTitles} place={place} pending={building} canBuild={canBuild} cantBuildWhy={cantBuildWhy}
                    busy={busy} lastBad={lastBad} onBuild={buildIdea} onDescribe={() => inputRef.current?.focus()} />
                  {(lines.some((l) => l.startsWith("You: ")) || notices.length > 0) && (
                    <Expandable expandDirection="vertical" expandBehavior="push" className="mx-auto w-full max-w-[640px] shrink-0">
                      {({ isExpanded }: { isExpanded: boolean }) => (
                        <>
                          <ExpandableTrigger aria-expanded={isExpanded} aria-controls="conversation"
                            className="flex min-h-11 w-full cursor-pointer items-center justify-between gap-3 rounded-card border border-hairline bg-surface px-4 text-left text-ui text-ink outline-none hover:bg-raised focus-visible:ring-2 focus-visible:ring-focus">
                            <span>The whole conversation</span>
                            <span className="text-sm text-ink-2">{isExpanded ? "Hide" : "Show"}</span>
                          </ExpandableTrigger>
                          <ExpandableContent preset="fade">
                            <div id="conversation" className="mt-4 flex flex-col gap-4">
                              <ThreadBody lines={lines} notices={notices} pending={building} ideas={ideas} builtTitles={builtTitles} canBuild={canBuild}
                                busy={busy} buildingIdea={building?.build} freshFrom={lines.length} onBuild={buildIdea} />
                            </div>
                          </ExpandableContent>
                        </>
                      )}
                    </Expandable>
                  )}
                </>
              )}
            </div>
          </div>

          <div className="torn flex-none bg-raised">
            <div className="mx-auto flex w-full max-w-[760px] flex-col gap-2 px-3 pb-3 pt-4 md:gap-3 md:px-6 md:pb-5 md:pt-5">
              {undoReason && <p id="undo-reason" className="self-end text-sm text-ink-2">{undoReason}</p>}
              <form id="composer" onSubmit={send} className="grid gap-2 sm:flex sm:items-stretch">
                <InputGroup className="min-w-0 sm:flex-1">
                  <InputGroupInput
                    ref={inputRef}
                    id="composer-input"
                    aria-label="Describe your game"
                    placeholder="Describe your game, for example: a lava obby with coins"
                    className="[&_input]:!h-11 [&_input]:!leading-11"
                    autoComplete="off"
                    value={text}
                    onChange={(e) => setText(e.target.value)}
                  />
                  <InputGroupAddon align="inline-end">
                    <MorphingDialog transition={open}>
                      <MorphingDialogTrigger thing="undo" blocked={!canUndo} aria-describedby="undo-reason" aria-label="Undo the last build" className="inline-flex min-h-11 cursor-pointer items-center gap-1.5 rounded-sm px-3 text-sm text-ink-2 outline-none hover:bg-track hover:text-ink focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:text-disabled">
                        <Undo2 className="size-4" strokeWidth={1.5} aria-hidden />Undo
                      </MorphingDialogTrigger>
                      <MorphingDialogContainer>
                        <MorphingDialogContent className="relative w-full max-w-[420px] rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift">
                          <div onClick={onUndoClick} data-undo-dialog className="flex flex-col gap-4">
                            <div>
                              <MorphingDialogTitle className="font-heading text-title font-semibold">Undo the last build?</MorphingDialogTitle>
                              <MorphingDialogDescription className="mt-1 text-ui text-ink-2">Your game goes back to how it was before that build.</MorphingDialogDescription>
                            </div>
                            <div className="flex flex-wrap justify-end gap-2">
                              <MorphingDialogClose aria-label="Keep it" className="static inline-flex min-h-11 cursor-pointer items-center rounded-card border border-hairline bg-surface px-4 text-ui text-ink outline-none hover:bg-raised focus-visible:ring-2 focus-visible:ring-focus">Keep it</MorphingDialogClose>
                              <MorphingDialogClose data-undo-yes aria-label="Yes, undo it" className="static inline-flex min-h-11 cursor-pointer items-center rounded-card bg-blue px-4 text-ui font-medium text-blue-fg shadow-[0_3px_0_var(--key-edge)] outline-none focus-visible:ring-2 focus-visible:ring-focus">Yes, undo it</MorphingDialogClose>
                            </div>
                          </div>
                        </MorphingDialogContent>
                      </MorphingDialogContainer>
                    </MorphingDialog>
                  </InputGroupAddon>
                </InputGroup>
                <Key type="submit" disabled={busy || !text.trim()} className="w-full px-4 sm:w-auto">
                  <Send className="size-4" strokeWidth={1.5} aria-hidden />Send
                </Key>
              </form>
            </div>
          </div>
        </main>

        {!narrow && (
          <div className="flex-none border-l border-hairline" style={{ width: shelfCollapsed ? "auto" : 300 }}>
            <PartsShelf place={place} error={place === "error"} collapsed={shelfCollapsed} onToggle={() => setShelfCollapsed((c) => !c)} />
          </div>
        )}
        <Toaster position="bottom-right" theme={theme} />
      </div>
    </MotionConfig>
  );
}

