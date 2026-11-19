import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type MouseEvent } from "react";
import { motion } from "motion/react";
import { Toaster, toast } from "sonner";
import { Drawer } from "vaul";
import { Menu, Plus, Undo2, Send, Sparkles, Sun, Moon, PanelRight } from "lucide-react";
import { TextureButton } from "@/components/ui/texture-button";
import { Key } from "./app/Key";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { AnimatedBackground } from "@/components/ui/animated-background";
import { MorphingDialog, MorphingDialogTrigger, MorphingDialogContainer, MorphingDialogContent, MorphingDialogTitle, MorphingDialogDescription, MorphingDialogClose } from "@/components/ui/morphing-dialog";
import { Pill } from "@/components/kibo-ui/pill";
import { open } from "@/motion";
import { read, post, RELEASES_URL, SEND_FAILED, type Chat, type ChatList, type Idea, type Place } from "./app/data";
import { ThreadBody, Welcome } from "./app/Thread";
import { PartsShelf, StudioChip } from "./app/Shelf";

type Pending = { message?: string; build?: string; undo?: boolean };
type Notice = { text: string; bad?: boolean; user?: boolean };

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
  const banners = document.getElementById("server-banners");
  return {
    gameName: s?.dataset.gameName || "",
    canBuild: s?.dataset.canBuild === "true",
    studio: s?.dataset.studio === "true",
    bannerHtml: banners?.innerHTML || "",
  };
}

export function App() {
  const shell = useMemo(readShell, []);
  const [theme, setThemeState] = useState<"light" | "dark">(() => (document.documentElement.dataset.theme === "dark" ? "dark" : "light"));
  const setTheme = (t: "light" | "dark") => {
    setThemeState(t);
    document.documentElement.setAttribute("data-theme", t);
    try { localStorage.setItem("openro-theme", t); } catch { /* the choice lasts this visit only */ }
  };
  const phone = useMedia("(max-width: 759px)");
  const narrow = useMedia("(max-width: 1099px)");

  const [chats, setChats] = useState<ChatList | null>(null);
  const [lines, setLines] = useState<string[]>([]);
  const [threadError, setThreadError] = useState(false);
  const [ideas, setIdeas] = useState<Idea[]>([]);
  const [place, setPlace] = useState<Place | null | "error">(null);
  const [inflight, setInflight] = useState<Record<string, Pending>>({});
  const [, bump] = useState(0);
  const [text, setText] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const [shelfOpen, setShelfOpen] = useState(false);
  const [shelfCollapsed, setShelfCollapsed] = useState(false);
  const [version, setVersion] = useState("");
  const shownRef = useRef(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const activeId = chats?.active ?? "";
  const notices: Notice[] = activeId ? slotRead<Notice[]>(noticeKey(activeId), []) : [];
  const busy = Object.keys(inflight).length > 0;
  const building = activeId ? inflight[activeId] : undefined;
  const buildingTitle = building?.build;
  const studioOk = shell.studio;
  const canBuild = shell.canBuild;
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
    try {
      const data = await read<ChatList>("/api/chats");
      setChats(data);
    } catch { /* the list keeps its last good state */ }
  }, []);
  const refreshThread = useCallback(async () => {
    try {
      const data = await read<{ thread: string[] }>("/api/thread");
      setLines(data.thread);
      setThreadError(false);
    } catch {
      if (!lines.length) setThreadError(true);
    }
    shownRef.current = 0;
    bump((x) => x + 1);
  }, [lines.length]);
  const refreshIdeas = useCallback(async () => {
    try {
      const data = await post<{ ideas?: Idea[]; pitched_new?: boolean }>("/api/ideas", {});
      setIdeas(data.ideas || []);
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
    // The old page's one timer: the chat list and thread keep their last good state between actions.
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Undo confirm and menu are modal; a switch of chat moves the unsent text with the person.
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
    shownRef.current = 0;
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

  const onUndoClick = (e: MouseEvent<HTMLDivElement>) => {
    if ((e.target as HTMLElement).closest("[data-undo-yes]")) void undoNow();
  };

  // The server's notices: a dismiss button hides its own banner (the old page's behaviour, without a per-id key).
  const onBannerClick = (e: MouseEvent<HTMLDivElement>) => {
    const b = (e.target as HTMLElement).closest("[data-dismiss]");
    const banner = b?.closest<HTMLElement>(".banner");
    if (banner) banner.hidden = true;
  };

  const rail = (
    <nav aria-label="Chats and settings" className="flex h-full min-h-0 flex-col gap-3 bg-surface p-4 text-ink">
      <div className="flex min-w-0 items-center gap-3 px-1 pb-2 pt-1">
        <span className="grid size-10 flex-none place-items-center rounded-card bg-blue text-blue-fg shadow-primary"><Sparkles className="size-5" strokeWidth={1.5} aria-hidden /></span>
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
          className="rounded-card bg-track"
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
                  className="flex min-h-11 w-full cursor-pointer items-center gap-2 rounded-card px-3 text-left text-ui text-ink-2 outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-blue data-[checked=true]:font-medium data-[checked=true]:text-ink"
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
        <div className="flex items-center justify-between gap-2">
          <span className="text-sm text-ink-2">Appearance</span>
          <SegmentedControl aria-label="Appearance">
            <SegmentedControlItem data-checked={theme === "light" ? "" : undefined} aria-pressed={theme === "light"} onClick={() => setTheme("light")} className="gap-1.5">
              <Sun className="size-4" strokeWidth={1.5} aria-hidden />Light
            </SegmentedControlItem>
            <SegmentedControlItem data-checked={theme === "dark" ? "" : undefined} aria-pressed={theme === "dark"} onClick={() => setTheme("dark")} className="gap-1.5">
              <Moon className="size-4" strokeWidth={1.5} aria-hidden />Dark
            </SegmentedControlItem>
          </SegmentedControl>
        </div>
        <a href={`${RELEASES_URL}${version}`} target="_blank" rel="noopener" className="self-start rounded-sm">
          <Pill className="bg-track text-sm text-ink-2">OpenRO {version}</Pill>
        </a>
      </div>
    </nav>
  );

  const shelfContent = <PartsShelf place={place} error={place === "error"} />;

  return (
    <div className="flex h-dvh min-h-0 w-full overflow-hidden bg-canvas text-ink">
      {!phone && <div className="w-[264px] flex-none border-r border-hairline">{rail}</div>}
      <Drawer.Root open={menuOpen && phone} onOpenChange={setMenuOpen} direction="left">
        <Drawer.Portal>
          <Drawer.Overlay className="fixed inset-0 z-40 bg-ink/20" />
          <Drawer.Content aria-describedby={undefined} className="fixed inset-y-0 left-0 z-50 w-[min(86vw,300px)] outline-none">
            <Drawer.Title className="sr-only">Chats and settings</Drawer.Title>
            {rail}
          </Drawer.Content>
        </Drawer.Portal>
      </Drawer.Root>

      <main className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-10 flex h-14 flex-none items-center gap-3 border-b border-hairline bg-canvas px-4 md:px-6">
          {phone && (
            <button type="button" onClick={() => setMenuOpen(true)} aria-label="Open the menu" aria-expanded={menuOpen} className="grid size-11 cursor-pointer place-items-center rounded-sm text-icon outline-none hover:bg-track focus-visible:ring-2 focus-visible:ring-blue">
              <Menu className="size-5" strokeWidth={1.5} aria-hidden />
            </button>
          )}
          <h2 className="min-w-0 flex-1 truncate font-heading text-title tracking-display font-semibold">{activeTitle}</h2>
          {chats && <StudioChip connected={studioOk} />}
          {narrow && (
            <Drawer.Root open={shelfOpen} onOpenChange={setShelfOpen}>
              <Drawer.Trigger asChild>
                <button type="button" aria-label="In your game" className="grid size-11 flex-none cursor-pointer place-items-center rounded-card border border-hairline bg-surface outline-none hover:bg-track focus-visible:ring-2 focus-visible:ring-blue">
                  <PanelRight className="size-5" strokeWidth={1.5} aria-hidden />
                </button>
              </Drawer.Trigger>
              <Drawer.Portal>
                <Drawer.Overlay className="fixed inset-0 z-40 bg-ink/20" />
                <Drawer.Content className="fixed inset-x-0 bottom-0 z-50 flex max-h-[80dvh] flex-col rounded-t-panel border-t border-hairline bg-board outline-none">
                  <Drawer.Handle className="mx-auto mt-3 h-1.5 w-10 rounded-full bg-track" />
                  <Drawer.Title className="sr-only">In your game</Drawer.Title>
                  <div className="min-h-0 flex-1 overflow-y-auto">{shelfContent}</div>
                </Drawer.Content>
              </Drawer.Portal>
            </Drawer.Root>
          )}
        </header>

        <div className="server-banners" onClick={onBannerClick} dangerouslySetInnerHTML={{ __html: shell.bannerHtml }} />

        <div id="thread" aria-live="polite" aria-busy={!chats} className="min-h-0 flex-1 overflow-y-auto">
          <div className="mx-auto flex w-full max-w-[760px] flex-col gap-4 p-4 md:p-6">
            {threadError ? (
              <div className="flex flex-col items-start gap-3 rounded-panel bg-pill-pro-bg p-4 text-pill-pro-fg">
                <strong className="font-heading font-semibold">Could not load the conversation.</strong>
                <p className="text-ink">The OpenRO server did not answer. Try again, or reload the page.</p>
                <Key onClick={() => void refreshAll()}>Try again</Key>
              </div>
            ) : (
              <>
                {!lines.some((l) => l.startsWith("You: ") || l.startsWith("Playtest") || l.startsWith("Wrote ") || l.startsWith("Generated ")) && !building?.message && <Welcome canBuild={canBuild} />}
                <ThreadBody
                  lines={lines}
                  notices={notices}
                  pending={building}
                  ideas={ideas}
                  canBuild={canBuild && studioOk}
                  busy={busy}
                  buildingIdea={buildingTitle}
                  freshFrom={shownRef.current}
                  onBuild={buildIdea}
                />
              </>
            )}
          </div>
        </div>

        <div className="flex-none border-t border-hairline bg-canvas">
          <div className="mx-auto flex w-full max-w-[760px] flex-col gap-3 p-4 md:px-6 md:pb-5">
            {ideas.length > 0 && lines.some((l) => l.startsWith("You: ") || l.startsWith("Wrote ") || l.startsWith("Playtest")) && (
              <div id="ideas" className="flex flex-wrap items-center gap-2">
                <span className="mr-1 text-sm text-ink-2">Next ideas</span>
                {ideas.map((idea) => (
                  <button key={idea.id} type="button" disabled={!canBuild || busy} onClick={() => buildIdea(idea)} aria-label={`Build ${idea.title}`} className="inline-flex min-h-11 cursor-pointer items-center gap-1.5 rounded-chip border border-hairline bg-surface px-3 text-sm text-ink outline-none transition-colors hover:bg-raised focus-visible:ring-2 focus-visible:ring-blue disabled:cursor-default disabled:text-disabled">
                    <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />{idea.title}
                  </button>
                ))}
              </div>
            )}
            {undoReason && <p id="undo-reason" className="self-end text-sm text-ink-2">Undo: {undoReason.toLowerCase()}</p>}
            <form id="composer" onSubmit={send} className="grid gap-2 sm:flex sm:items-stretch">
              <InputGroup className="min-w-0 sm:flex-1">
                <InputGroupInput
                  ref={inputRef}
                  id="composer-input"
                  aria-label="Describe your game"
                  placeholder="Describe your game, for example: a lava obby with coins"
                  autoComplete="off"
                  value={text}
                  onChange={(e) => setText(e.target.value)}
                />
                <InputGroupAddon align="inline-end">
                  <MorphingDialog transition={open}>
                    <MorphingDialogTrigger thing="undo" blocked={!canUndo} aria-describedby="undo-reason" aria-label="Undo the last build" className="inline-flex min-h-11 cursor-pointer items-center gap-1.5 rounded-sm px-3 text-sm text-ink-2 outline-none hover:bg-track hover:text-ink focus-visible:ring-2 focus-visible:ring-blue disabled:cursor-default disabled:text-disabled">
                      <Undo2 className="size-4" strokeWidth={1.5} aria-hidden />Undo
                    </MorphingDialogTrigger>
                    <MorphingDialogContainer>
                      <MorphingDialogContent className="relative w-[min(420px,calc(100vw-32px))] rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift">
                        <div onClick={onUndoClick} className="flex flex-col gap-4">
                          <div>
                            <MorphingDialogTitle className="font-heading text-title font-semibold">Undo the last build?</MorphingDialogTitle>
                            <MorphingDialogDescription className="mt-1 text-ui text-ink-2">Your game goes back to how it was before that build.</MorphingDialogDescription>
                          </div>
                          <div className="flex flex-wrap justify-end gap-2">
                            <MorphingDialogClose className="inline-flex min-h-11 cursor-pointer items-center rounded-card border border-hairline bg-surface px-4 text-ui text-ink outline-none focus-visible:ring-2 focus-visible:ring-blue">Keep it</MorphingDialogClose>
                            <MorphingDialogClose data-undo-yes className="inline-flex min-h-11 cursor-pointer items-center rounded-card bg-ink-button px-4 text-ui font-medium text-ink-button-fg outline-none focus-visible:ring-2 focus-visible:ring-blue">Yes, undo it</MorphingDialogClose>
                          </div>
                        </div>
                      </MorphingDialogContent>
                    </MorphingDialogContainer>
                  </MorphingDialog>
                </InputGroupAddon>
              </InputGroup>
              <Key type="submit" disabled={busy || !text.trim()} className="w-full px-4 sm:w-auto sm:flex-none">
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
  );
}

