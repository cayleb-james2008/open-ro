import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent,
} from "react";
import { MotionConfig } from "motion/react";
import { Toaster, toast } from "sonner";
import { Drawer } from "vaul";
import {
  Blocks,
  ExternalLink,
  FolderTree,
  Menu,
  Moon,
  Plus,
  RefreshCw,
  Search,
  Send,
  Sun,
  Undo2,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupAddon, InputGroupInput, InputGroupTextarea } from "@/components/ui/input-group";
import { Kbd } from "@/components/ui/kbd";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { AnimatedBackground } from "@/components/ui/animated-background";
import { SlidingNumber } from "@/components/ui/sliding-number";
import {
  MorphingDialog,
  MorphingDialogClose,
  MorphingDialogContainer,
  MorphingDialogContent,
  MorphingDialogDescription,
  MorphingDialogTitle,
  MorphingDialogTrigger,
} from "@/components/ui/morphing-dialog";
import { Pill } from "@/components/kibo-ui/pill";
import { glide, open } from "@/motion";
import {
  RELEASES_URL,
  SEND_FAILED,
  normalizeChatList,
  normalizeIdeas,
  normalizePlace,
  normalizeThread,
  post,
  read,
  type Chat,
  type ChatList,
  type Idea,
  type Place,
} from "./app/data";
import { BlueprintWorkbench } from "./app/Blueprints";
import { ThreadFeed } from "./app/Thread";
import { PartsShelf, StudioChip } from "./app/Shelf";
import { UpdateStrip, useUpdate } from "./app/Update";

type Pending = { message?: string; build?: string; undo?: boolean };
type Notice = { text: string; bad?: boolean; user?: boolean };
type ShellWindow = Window & { openroSetTheme?: (t: string) => void; __OPENRO_DEMO__?: boolean };

const noticeKey = (id: string) => "openro-notices:" + id;
const draftKey = (id: string) => "openro-draft:" + id;

function slotRead<T>(key: string, fallback: T): T {
  try {
    const v = sessionStorage.getItem(key);
    return v === null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}

function slotWrite(key: string, value: unknown) {
  try {
    if (value === "" || (Array.isArray(value) && !value.length)) sessionStorage.removeItem(key);
    else sessionStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* storage unavailable: in-memory render continues cleanly */
  }
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

function readShell() {
  const s = document.getElementById("server-state");
  return {
    gameName: s?.dataset.gameName || "Studio Place",
    canBuild: s?.dataset.canBuild === "true",
    studio: s?.dataset.studio === "true",
    demo: s?.dataset.demo === "true" || Boolean((window as ShellWindow).__OPENRO_DEMO__),
    bannerHtml: document.getElementById("server-banners")?.innerHTML || "",
  };
}

export function App() {
  const shell = useMemo(readShell, []);
  const [theme, setThemeState] = useState<"light" | "dark">(() =>
    document.documentElement.dataset.theme === "dark" ? "dark" : "light",
  );
  const setTheme = (t: "light" | "dark") => {
    setThemeState(t);
    document.documentElement.setAttribute("data-theme", t);
    (window as ShellWindow).openroSetTheme?.(t);
  };

  // <768px collapses Left Rail to Vaul drawer (at 768px the rail stays visible so probe.mjs reaches Light/Dark).
  const phone = useMedia("(max-width: 767px)");
  const compactInput = useMedia("(max-width: 960px)");
  // <1100px collapses Right Explorer Shelf to Vaul bottom drawer.
  const narrow = useMedia("(max-width: 1099px)");

  const [chats, setChats] = useState<ChatList | null>(null);
  const [chatsError, setChatsError] = useState(false);
  const [lines, setLines] = useState<string[]>([]);
  const [threadError, setThreadError] = useState(false);
  const [ideas, setIdeas] = useState<Idea[]>([]);
  const [builtTitles, setBuiltTitles] = useState<string[]>([]);
  const [place, setPlace] = useState<Place | null | "error">(null);
  const [inflight, setInflight] = useState<Record<string, Pending>>({});
  const [, bump] = useState(0);
  const [text, setText] = useState("");
  const [chatQuery, setChatQuery] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const [shelfOpen, setShelfOpen] = useState(false);
  const [shelfCollapsed, setShelfCollapsed] = useState(false);
  const [studioOk, setStudioOk] = useState(shell.studio);
  const [modelOk, setModelOk] = useState(shell.canBuild || !shell.studio);
  const [version, setVersion] = useState("");
  const [lastTriedIdea, setLastTriedIdea] = useState<Idea | null>(null);

  const inputRef = useRef<HTMLTextAreaElement>(null);
  const chatSearchRef = useRef<HTMLInputElement>(null);
  const menuKeyRef = useRef<HTMLButtonElement>(null);
  const shelfKeyRef = useRef<HTMLButtonElement>(null);
  const wasMenuOpenRef = useRef(false);
  const wasShelfOpenRef = useRef(false);
  const showUpdateRef = useRef<HTMLButtonElement>(null);
  const threadRef = useRef<HTMLDivElement>(null);

  const update = useUpdate(showUpdateRef);

  useEffect(() => {
    if (wasMenuOpenRef.current && !menuOpen) {
      const focusTrigger = () => {
        if (document.activeElement === inputRef.current) return;
        (menuKeyRef.current ?? document.querySelector<HTMLElement>('[aria-label="Open the menu"]'))?.focus();
      };
      requestAnimationFrame(focusTrigger);
      const t1 = window.setTimeout(focusTrigger, 120);
      const t2 = window.setTimeout(focusTrigger, 420);
      wasMenuOpenRef.current = false;
      return () => {
        window.clearTimeout(t1);
        window.clearTimeout(t2);
      };
    }
    wasMenuOpenRef.current = menuOpen;
  }, [menuOpen]);

  useEffect(() => {
    if (wasShelfOpenRef.current && !shelfOpen) {
      const focusTrigger = () => {
        if (document.activeElement === inputRef.current) return;
        (shelfKeyRef.current ?? document.querySelector<HTMLElement>('[aria-label="In your game"]'))?.focus();
      };
      requestAnimationFrame(focusTrigger);
      const t1 = window.setTimeout(focusTrigger, 120);
      const t2 = window.setTimeout(focusTrigger, 420);
      wasShelfOpenRef.current = false;
      return () => {
        window.clearTimeout(t1);
        window.clearTimeout(t2);
      };
    }
    wasShelfOpenRef.current = shelfOpen;
  }, [shelfOpen]);

  const activeId = chats?.active ?? "";
  const notices: Notice[] = activeId ? slotRead<Notice[]>(noticeKey(activeId), []) : [];
  const busy = Object.keys(inflight).length > 0;
  const building = activeId ? inflight[activeId] : undefined;
  const canBuild = studioOk && modelOk;

  const totalPieces =
    place && place !== "error"
      ? Object.keys(place.scripts || {}).length + Object.keys(place.instances || {}).length
      : 0;
  const builtCount = place && place !== "error" ? totalPieces : null;

  const undoState = !chats
    ? "loading"
    : busy || place === null
      ? "busy"
      : place === "error"
        ? "unknown"
        : !studioOk
          ? "nostudio"
          : !chats.can_undo
            ? "nothing"
            : "ok";
  const undoReason =
    undoState === "nostudio"
      ? "Connect Studio to inspect or undo builds"
      : undoState === "nothing" || (builtCount === 0 && undoState !== "ok")
        ? "Nothing to undo yet"
        : "";
  const canUndo = undoState === "ok";
  const activeTitle = chats?.chats.find((c) => c.id === activeId)?.title || "New chat";
  const hasConversation = useMemo(
    () => lines.some((l) => l.startsWith("You: ")) || notices.length > 0 || Boolean(building),
    [lines, notices.length, building],
  );

  const addNotice = useCallback((chatId: string, n: Notice) => {
    const key = noticeKey(chatId || "default");
    slotWrite(key, [...slotRead<Notice[]>(key, []), n]);
    bump((x) => x + 1);
  }, []);

  const begin = (chatId: string, p: Pending) => {
    slotWrite(noticeKey(chatId || "default"), "");
    setInflight((s) => ({ ...s, [chatId || "default"]: p }));
  };
  const end = (chatId: string) =>
    setInflight((s) => {
      const next = { ...s };
      delete next[chatId || "default"];
      return next;
    });

  const refreshChats = useCallback(async () => {
    try {
      const raw = await read("/api/chats");
      setChats(normalizeChatList(raw));
      setChatsError(false);
    } catch {
      setChatsError(true);
    }
  }, []);

  const refreshThread = useCallback(async () => {
    try {
      const raw = await read("/api/thread");
      setLines(normalizeThread(raw));
      setThreadError(false);
    } catch {
      setThreadError(true);
    }
  }, []);

  const refreshIdeas = useCallback(async () => {
    try {
      const raw = await post("/api/ideas", {});
      const norm = normalizeIdeas(raw);
      setIdeas(norm.ideas);
      setBuiltTitles(norm.builtTitles);
      if (norm.pitchedNew) await refreshThread();
    } catch {
      /* retain last good ideas list */
    }
  }, [refreshThread]);

  const refreshPlace = useCallback(async () => {
    try {
      const raw = await read("/api/place-tree");
      setPlace(normalizePlace(raw));
    } catch {
      setPlace("error");
    }
  }, []);

  const refreshAll = useCallback(async () => {
    await Promise.all([refreshChats(), refreshThread(), refreshPlace()]);
    await refreshIdeas();
  }, [refreshChats, refreshThread, refreshPlace, refreshIdeas]);

  useEffect(() => {
    void refreshAll();
    read<{ version?: unknown }>("/api/version")
      .then((v) => {
        if (v && typeof v.version === "string") setVersion(v.version);
      })
      .catch(() => {});
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const prevChatRef = useRef<string | null>(null);
  const prevLenRef = useRef(0);

  // Reset scroll to top on chat switch / initial load; auto-scroll to bottom when new lines arrive in the same chat.
  useEffect(() => {
    const el = threadRef.current;
    if (!el) return;
    if (!hasConversation || prevChatRef.current !== activeId) {
      el.scrollTop = 0;
    } else if (lines.length > prevLenRef.current || Boolean(building)) {
      el.scrollTop = el.scrollHeight;
    }
    prevChatRef.current = activeId;
    prevLenRef.current = lines.length;
  }, [activeId, lines.length, notices.length, Boolean(building), hasConversation]);

  // Global `/` shortcut jumps to chat filter on desktop or prompt box on mobile.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "/" || e.metaKey || e.ctrlKey || e.altKey) return;
      const el = e.target as HTMLElement | null;
      if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) return;
      e.preventDefault();
      if (!phone && chatSearchRef.current) chatSearchRef.current.focus();
      else inputRef.current?.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [phone]);

  const cantBuildWhy = !studioOk
    ? "Building needs Roblox Studio open with the OpenRO MCP bridge. Use Check again once Studio is open."
    : !modelOk
      ? "Chat prompts need a model API key configured on this server. Built-in Studio blueprints still run once Studio connects."
      : "";

  const checkStudio = async () => {
    try {
      const s = await read<{ studio_connected?: unknown; model_configured?: unknown }>("/api/status");
      const nextStudio = s?.studio_connected === true;
      const nextModel = s?.model_configured === true;
      if (nextStudio && !studioOk) toast.success("Studio reconnected");
      else if (!nextStudio) toast.error("Studio is still disconnected");
      setStudioOk(nextStudio);
      setModelOk(nextModel);
      if (nextStudio) await refreshPlace();
    } catch {
      toast.error("Could not reach the OpenRO server.");
    }
  };

  const switchChat = async (id: string) => {
    if (!id || id === activeId) return;
    try {
      const r = await fetch("/api/chats/select", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ id }),
      });
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
      const r = await fetch("/api/chats", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: "{}",
      });
      if (!r.ok) throw new Error("new chat");
    } catch {
      toast.error("Could not reach the OpenRO server.");
      return;
    }
    setChatQuery("");
    setMenuOpen(false);
    await refreshAll();
    inputRef.current?.focus();
  };

  const buildIdea = async (idea: Idea) => {
    const chatId = activeId || "default";
    setLastTriedIdea(idea);
    begin(chatId, { build: idea.title });
    let failure: string | null = null;
    try {
      const data = await post<{ ok?: boolean; error?: string }>("/api/ideas/build", { id: idea.id });
      if (!data.ok) failure = typeof data.error === "string" && data.error ? data.error : "The build did not finish.";
      else toast.success(`Built ${idea.title} in Studio`);
    } catch {
      failure = "Could not reach the OpenRO server.";
    }
    end(chatId);
    if (failure) addNotice(chatId, { text: failure, bad: true });
    await refreshAll();
    await new Promise((r) => setTimeout(r, 60));
    const active = document.activeElement;
    if (!active || active === document.body) {
      const key = document.querySelector<HTMLButtonElement>(`[data-focus-key="idea:${CSS.escape(idea.title)}"]`);
      if (key && !key.disabled) key.focus();
      else inputRef.current?.focus();
    }
  };

  const submitMessage = async () => {
    const message = text.trim();
    if (!message || busy) return;
    const chatId = activeId || "default";
    setText("");
    begin(chatId, { message });
    let failure: string | null = null;
    try {
      const r = await fetch("/api/chat", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ message }),
      });
      const raw = await r.text();
      let data: { ok?: boolean; error?: string } = {};
      try {
        data = JSON.parse(raw) as { ok?: boolean; error?: string };
      } catch {
        /* non-JSON proxy or crash reply */
      }
      if (r.status === 403 || r.status === 413 || r.status === 415) {
        failure = data.error || "OpenRO did not accept that message.";
      } else if (!r.ok && !((r.status === 502 || r.status === 503) && data.ok === false)) {
        failure = SEND_FAILED;
      }
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

  const send = (e: FormEvent) => {
    e.preventDefault();
    void submitMessage();
  };

  const onComposerKeyDown = (e: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void submitMessage();
    }
  };

  const undoNow = async () => {
    const chatId = activeId || "default";
    begin(chatId, { undo: true });
    let msg: string | null = null;
    let bad = false;
    try {
      const data = await post<{ ok?: boolean; restored?: unknown[]; error?: string }>("/api/revert", {});
      if (data.ok) {
        const count = Array.isArray(data.restored) ? data.restored.length : 0;
        msg = `Undid the last build. ${count} change(s) were put back.`;
      } else if (data.error === "nothing to revert") {
        msg = "Nothing to undo. No build is waiting to be undone in this chat.";
      } else {
        msg = typeof data.error === "string" && data.error ? data.error : "The undo did not finish.";
        bad = true;
      }
    } catch {
      msg = "Could not reach the OpenRO server.";
      bad = true;
    }
    end(chatId);
    if (msg) {
      addNotice(chatId, { text: msg, bad });
      if (bad) toast.error(msg);
      else toast.success(msg);
    }
    await refreshAll();
  };

  const onUndoClick = (e: MouseEvent<HTMLDivElement>) => {
    if ((e.target as HTMLElement).closest("[data-undo-yes]")) void undoNow();
  };

  const onBannerClick = (e: MouseEvent<HTMLDivElement>) => {
    const banner = (e.target as HTMLElement).closest("[data-dismiss]")?.closest<HTMLElement>(".banner");
    if (banner) banner.hidden = true;
  };

  const filteredChats = useMemo(() => {
    const all = chats?.chats || [];
    const q = chatQuery.trim().toLowerCase();
    return q ? all.filter((c) => (c.title || "New chat").toLowerCase().includes(q)) : all;
  }, [chats, chatQuery]);

  const rail = (
    <nav aria-label="Chats and Studio controls" className="flex h-full min-h-0 flex-col gap-3 bg-surface p-3.5 text-ink">
      <div className="flex min-w-0 items-center gap-2.5 px-0.5 pt-0.5">
        <span className="grid size-9 flex-none place-items-center rounded-card border border-hairline bg-board text-ink">
          <Blocks className="size-4.5" strokeWidth={1.5} aria-hidden />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <h1 className="text-base font-medium leading-tight text-ink">OpenRO</h1>
            {shell.demo && (
              <Pill className="bg-tag-ops-bg px-1.5 py-0 font-mono text-xs text-tag-ops-fg">fixture demo</Pill>
            )}
          </div>
          <div className="truncate text-xs text-ink-2" title={shell.gameName}>
            {shell.gameName}
          </div>
        </div>
      </div>

      {/* Live Studio connection block inside Left Rail on desktop */}
      {!phone && (
        <div
          data-answers="2"
          className="flex flex-wrap items-center justify-between gap-2 rounded-card border border-hairline bg-board/60 px-2.5 py-2"
        >
          <StudioChip connected={studioOk} />
          {!studioOk && (
            <Button variant="outline" size="xs" onClick={() => void checkStudio()} className="bg-surface">
              <RefreshCw className="size-3" strokeWidth={1.5} aria-hidden />
              Check again
            </Button>
          )}
        </div>
      )}

      <Button
        variant="secondary"
        onClick={() => void newChat()}
        className="w-full justify-start border-ink-button bg-ink-button text-ink-button-fg hover:bg-ink-button/90 max-md:min-h-11"
      >
        <Plus className="size-4" strokeWidth={1.5} aria-hidden />
        New chat
      </Button>

      <InputGroup className="bg-board/45">
        <InputGroupAddon align="inline-start" className="ps-2.5">
          <Search className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
        </InputGroupAddon>
        <InputGroupInput
          ref={chatSearchRef}
          type="search"
          value={chatQuery}
          onChange={(e) => setChatQuery(e.target.value)}
          aria-label="Search chats"
          placeholder="Search chats"
          className="[&_input]:max-md:!min-h-11 [&_input]:md:!h-8"
        />
        {chatQuery ? (
          <InputGroupAddon align="inline-end" className="has-[>button]:me-0 pe-1">
            <Button
              variant="ghost"
              size="icon-xs"
              onClick={() => setChatQuery("")}
              aria-label="Clear chat search"
              className="max-md:size-11"
            >
              <X className="size-3.5" strokeWidth={1.5} aria-hidden />
            </Button>
          </InputGroupAddon>
        ) : (
          <InputGroupAddon align="inline-end" className="has-[>kbd:last-child]:me-0 pe-2">
            <Kbd className="px-1.5 font-mono">/</Kbd>
          </InputGroupAddon>
        )}
      </InputGroup>

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        {chatsError && !chats ? (
          <div className="flex flex-col items-start gap-2 rounded-card border border-hairline bg-board p-3 text-xs text-ink-2">
            <span>Could not load chats from server.</span>
            <Button variant="outline" size="xs" onClick={() => void refreshAll()} className="max-md:min-h-11">
              Retry chats
            </Button>
          </div>
        ) : filteredChats.length === 0 ? (
          <div className="flex flex-col items-start gap-2 rounded-card border border-hairline bg-board/50 p-3 text-xs text-ink-2">
            <span>No chats match &ldquo;{chatQuery}&rdquo;.</span>
            {chatQuery && (
              <Button variant="outline" size="xs" onClick={() => setChatQuery("")} className="max-md:min-h-11">
                Show all chats
              </Button>
            )}
          </div>
        ) : (
          <AnimatedBackground
            defaultValue={activeId}
            onValueChange={(id) => id && id !== activeId && void switchChat(id)}
            className="rounded-card bg-blue-soft"
            transition={glide}
            enableHover={false}
          >
            <ul id="chat-list" className="flex flex-col gap-0.5" aria-label="Your chats">
              {filteredChats.map((c: Chat) => (
                <li key={c.id} className="relative">
                  <Button
                    variant="ghost"
                    data-id={c.id}
                    aria-current={c.id === activeId ? "true" : undefined}
                    onClick={() => void switchChat(c.id)}
                    className="chat-btn h-auto min-h-9 w-full justify-between rounded-card border-transparent px-2.5 py-1.5 text-left font-normal text-ink-2 hover:bg-board/70 hover:text-ink data-[checked=true]:font-medium data-[checked=true]:text-ink max-md:min-h-11"
                  >
                    <span className="min-w-0 flex-1 truncate text-sm">{c.title || "New chat"}</span>
                    {inflight[c.id] && (
                      <Pill className="bg-surface px-1.5 py-0 font-mono text-xs text-ink">working</Pill>
                    )}
                  </Button>
                </li>
              ))}
            </ul>
          </AnimatedBackground>
        )}
      </div>

      <div className="flex flex-col gap-2.5 border-t border-hairline pt-3">
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs font-medium text-ink-2">Appearance</span>
          <SegmentedControl aria-label="Appearance" size="sm">
            <SegmentedControlItem
              data-checked={theme === "light" ? "" : undefined}
              aria-pressed={theme === "light"}
              onClick={() => setTheme("light")}
              className="gap-1 max-md:!h-11 max-md:px-3"
            >
              <Sun className="size-3.5" strokeWidth={1.5} aria-hidden />
              Light
            </SegmentedControlItem>
            <SegmentedControlItem
              data-checked={theme === "dark" ? "" : undefined}
              aria-pressed={theme === "dark"}
              onClick={() => setTheme("dark")}
              className="gap-1 max-md:!h-11 max-md:px-3"
            >
              <Moon className="size-3.5" strokeWidth={1.5} aria-hidden />
              Dark
            </SegmentedControlItem>
          </SegmentedControl>
        </div>

        <div className="flex items-center justify-between gap-2 text-xs text-ink-2">
          <a
            href={RELEASES_URL + (version || "0.3.0")}
            target="_blank"
            rel="noreferrer"
            className="inline-flex min-h-11 items-center gap-1 rounded-sm px-1 font-mono text-xs text-ink-2 outline-none hover:text-ink hover:underline md:min-h-7"
          >
            <span>OpenRO v{version || "0.3.0"}</span>
            <ExternalLink className="size-3 text-icon" strokeWidth={1.5} aria-hidden />
          </a>
          <span className="truncate text-right text-xs text-ink-3">
            {update.busy === "check"
              ? "Checking…"
              : update.snap?.staged
                ? `${update.snap.staged} staged`
                : update.snap?.available
                  ? `${update.snap.latest} available`
                  : update.snap?.checked_at
                    ? "Up to date"
                    : "Rust + Rune"}
          </span>
        </div>
      </div>
    </nav>
  );

  return (
    <MotionConfig reducedMotion="user">
      <div className="flex h-dvh min-h-0 w-full overflow-hidden bg-canvas text-ink" data-app="openro">
        <a className="skip" href="#composer-input">
          Skip to message box
        </a>

        {!phone && <div className="w-[256px] flex-none border-r border-hairline">{rail}</div>}

        <main className="flex min-w-0 flex-1 flex-col">
          <header className="flex min-h-14 flex-none flex-col justify-center gap-2 border-b border-hairline bg-surface px-3 py-2 md:flex-row md:items-center md:justify-between md:px-6">
            <div className="flex min-w-0 flex-1 items-center justify-between gap-2">
              <div className="flex min-w-0 flex-1 items-center gap-2">
                {phone && (
                  <Drawer.Root open={menuOpen} onOpenChange={setMenuOpen} direction="left" autoFocus>
                    <Drawer.Trigger asChild>
                      <Button
                        ref={menuKeyRef}
                        variant="ghost"
                        size="icon"
                        aria-label="Open the menu"
                        className="size-11"
                      >
                        <Menu className="size-5" strokeWidth={1.5} aria-hidden />
                      </Button>
                    </Drawer.Trigger>
                    <Drawer.Portal>
                      <Drawer.Overlay className="fixed inset-0 z-40 bg-ink/30" />
                      <Drawer.Content
                        onEscapeKeyDown={() => setMenuOpen(false)}
                        onCloseAutoFocus={(e) => {
                          e.preventDefault();
                          (menuKeyRef.current ?? document.querySelector<HTMLElement>('[aria-label="Open the menu"]'))?.focus();
                        }}
                        className="fixed inset-y-0 left-0 z-50 w-[min(86vw,288px)] border-r border-hairline bg-surface outline-none"
                      >
                        <Drawer.Title className="sr-only">Chats and Studio controls</Drawer.Title>
                        <Drawer.Description className="sr-only">
                          Switch Studio chats, start a new chat, filter conversations, and toggle Light or Dark appearance.
                        </Drawer.Description>
                        {rail}
                      </Drawer.Content>
                    </Drawer.Portal>
                  </Drawer.Root>
                )}
                <h2 className="min-w-0 flex-1 truncate text-base font-medium text-ink md:text-title">
                  {activeTitle}
                </h2>
              </div>

              {narrow && (
                <Drawer.Root open={shelfOpen} onOpenChange={setShelfOpen} autoFocus>
                  <Drawer.Trigger asChild>
                    <Button
                      ref={shelfKeyRef}
                      variant="outline"
                      size="sm"
                      aria-label="In your game"
                      data-answers-via="3"
                      className="shrink-0 gap-1.5 max-md:min-h-11 max-md:px-3"
                    >
                      <FolderTree className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                      <span className="hidden sm:inline">In your game</span>
                      <span className="rounded-chip bg-board px-1.5 py-0.5 font-mono text-xs tabular-nums text-ink">
                        <SlidingNumber value={totalPieces} />
                      </span>
                    </Button>
                  </Drawer.Trigger>
                  <Drawer.Portal>
                    <Drawer.Overlay className="fixed inset-0 z-40 bg-ink/30" />
                    <Drawer.Content
                      onEscapeKeyDown={() => setShelfOpen(false)}
                      onCloseAutoFocus={(e) => {
                        e.preventDefault();
                        (shelfKeyRef.current ?? document.querySelector<HTMLElement>('[aria-label="In your game"]'))?.focus();
                      }}
                      className="fixed inset-x-0 bottom-0 z-50 flex max-h-[82dvh] flex-col rounded-t-panel border-t border-hairline bg-surface outline-none"
                    >
                      <Drawer.Handle className="mx-auto mt-2.5 mb-1 h-1.5 w-10 rounded-full bg-track" />
                      <Drawer.Title className="sr-only">In your game</Drawer.Title>
                      <Drawer.Description className="sr-only">
                        Roblox Studio DataModel Explorer listing Workspace parts and Luau scripts in your place.
                      </Drawer.Description>
                      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
                        <PartsShelf
                          gameName={shell.gameName}
                          builtTitles={builtTitles}
                          place={place}
                          error={place === "error"}
                        />
                      </div>
                    </Drawer.Content>
                  </Drawer.Portal>
                </Drawer.Root>
              )}
            </div>

            {/* Mobile Studio status + Check again + fixture demo pill cleanly separated in first fold */}
            {phone && (
              <div className="flex w-full flex-wrap items-center justify-between gap-2 border-t border-hairline pt-2">
                <div data-answers="2" className="flex items-center gap-1.5">
                  <StudioChip connected={studioOk} />
                  {!studioOk && (
                    <Button variant="outline" size="sm" onClick={() => void checkStudio()} className="min-h-11">
                      <RefreshCw className="size-3.5" strokeWidth={1.5} aria-hidden />
                      Check again
                    </Button>
                  )}
                </div>
                {shell.demo && (
                  <Pill className="shrink-0 bg-tag-ops-bg px-2 py-0.5 font-mono text-xs text-tag-ops-fg">fixture demo</Pill>
                )}
              </div>
            )}
          </header>

          <div
            className="server-banners"
            onClick={onBannerClick}
            dangerouslySetInnerHTML={{ __html: shell.bannerHtml }}
          />
          {update.show && <UpdateStrip u={update} />}

          <div
            id="thread"
            ref={threadRef}
            aria-live="polite"
            aria-busy={!chats && !chatsError}
            className="min-h-0 flex-1 overflow-y-auto"
          >
            <div className="mx-auto flex w-full max-w-[840px] flex-col gap-5 p-3.5 md:p-6">
              {threadError ? (
                <div role="alert" className="flex flex-col items-start gap-3 rounded-card border border-hairline bg-surface p-4">
                  <strong className="text-ui font-medium text-pill-pro-fg">Could not load the conversation.</strong>
                  <p className="text-sm text-ink-2">The OpenRO server did not answer. Try again, or reload the page.</p>
                  <Button variant="default" onClick={() => void refreshAll()} className="max-md:min-h-11">
                    Try again
                  </Button>
                </div>
              ) : !hasConversation ? (
                <BlueprintWorkbench
                  ideas={ideas}
                  builtTitles={builtTitles}
                  canBuild={canBuild}
                  cantBuildWhy={cantBuildWhy}
                  busy={busy}
                  buildingTitle={building?.build}
                  onBuild={(idea) => void buildIdea(idea)}
                />
              ) : (
                <>
                  <div className="sr-only" id="stage-title">
                    Studio build thread
                  </div>
                  <ThreadFeed
                    lines={lines}
                    notices={notices}
                    pending={building}
                    ideas={ideas}
                    builtTitles={builtTitles}
                    canBuild={canBuild}
                    busy={busy}
                    buildingIdea={building?.build}
                    freshFrom={lines.length}
                    lastTriedIdea={lastTriedIdea}
                    onBuild={(idea) => void buildIdea(idea)}
                  />
                  {!building && ideas.some((i) => !builtTitles.includes(i.title)) && !lines.some((l) => l.startsWith("OpenRO: That landed.")) && (
                    <BlueprintWorkbench
                      ideas={ideas}
                      builtTitles={builtTitles}
                      canBuild={canBuild}
                      cantBuildWhy={cantBuildWhy}
                      busy={busy}
                      onBuild={(idea) => void buildIdea(idea)}
                    />
                  )}
                </>
              )}
            </div>
          </div>

          {/* Hairline Bottom Composer Dock (zero torn-paper polygon, zero -me-2 overflow) */}
          <div data-answers="4" className="flex-none border-t border-hairline bg-surface">
            <div className="mx-auto flex w-full max-w-[840px] flex-col gap-2 px-3 py-2.5 md:px-6 md:py-3.5">
              <form id="composer" onSubmit={send} className="grid gap-2 sm:flex sm:items-end">
                <InputGroup className="min-w-0 bg-canvas/55 sm:flex-1">
                  <InputGroupTextarea
                    ref={inputRef}
                    id="composer-input"
                    rows={1}
                    aria-label="Describe your game"
                    placeholder={
                      compactInput
                        ? "Describe your game…"
                        : "Describe a game or change, e.g. lava obby with coins…"
                    }
                    value={text}
                    onChange={(e) => setText(e.target.value)}
                    onKeyDown={onComposerKeyDown}
                    className="[&_textarea]:!min-h-11 [&_textarea]:max-h-28 [&_textarea]:!py-2.5 [&_textarea]:leading-5"
                  />
                  <InputGroupAddon align="inline-end" className="has-[>button]:me-0 self-center pe-1.5">
                    <MorphingDialog transition={open}>
                      <MorphingDialogTrigger
                        thing="undo"
                        blocked={!canUndo}
                        describedBy={undoReason ? "undo-reason" : undefined}
                        aria-label="Undo"
                        className="inline-flex min-h-11 items-center gap-1.5 rounded-md px-2.5 text-sm font-medium text-ink-2 outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-track hover:text-ink disabled:text-ink-3 md:min-h-8"
                      >
                        <Undo2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                        <span>Undo</span>
                      </MorphingDialogTrigger>
                      <MorphingDialogContainer>
                        <MorphingDialogContent className="relative mx-4 w-full max-w-[420px] rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift">
                          <div onClick={onUndoClick} data-undo-dialog className="flex flex-col gap-4">
                            <div>
                              <MorphingDialogTitle className="text-title font-medium text-ink">
                                Undo the last build?
                              </MorphingDialogTitle>
                              <MorphingDialogDescription className="mt-1 text-sm text-ink-2">
                                OpenRO restores the scripts and Workspace instances modified by the most recent build in this chat.
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
                                className="static inline-flex min-h-11 cursor-pointer items-center justify-center rounded-lg border border-blue bg-blue px-4 text-sm font-medium text-blue-fg outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-blue-hover md:min-h-9"
                              >
                                Yes, undo it
                              </MorphingDialogClose>
                            </div>
                          </div>
                        </MorphingDialogContent>
                      </MorphingDialogContainer>
                    </MorphingDialog>
                  </InputGroupAddon>
                </InputGroup>

                <Button
                  type="submit"
                  variant="default"
                  disabled={busy || !text.trim()}
                  className="h-11 w-full px-4 sm:w-auto"
                >
                  <Send className="size-4" strokeWidth={1.5} aria-hidden />
                  Send
                </Button>
              </form>

              <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 text-xs text-ink-2">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="hidden items-center gap-1 sm:inline-flex">
                    <Kbd className="px-1.5 font-mono">Enter</Kbd>
                    <span>send</span>
                    <span aria-hidden>·</span>
                    <Kbd className="px-1.5 font-mono">Shift+Enter</Kbd>
                    <span>new line</span>
                  </span>
                  {undoReason && <span id="undo-reason">{undoReason}</span>}
                </div>

                {/* Always-reachable update controls at every viewport width (including 390px) per BANNER-FIX-BRIEF */}
                <div className="flex items-center gap-1.5">
                  {update.isDismissedAvailable && (
                    <Button
                      ref={showUpdateRef}
                      variant="ghost"
                      size="xs"
                      onClick={() => update.showAgain()}
                      className="text-ink underline-offset-2 hover:underline max-md:min-h-11 max-md:px-2.5"
                    >
                      Show update
                    </Button>
                  )}
                  {update.canCheck && (
                    <Button
                      variant="ghost"
                      size="xs"
                      aria-label="Check for updates"
                      disabled={update.busy !== ""}
                      onClick={() => void update.check()}
                      className="text-ink-2 hover:text-ink max-md:min-h-11 max-md:px-2.5"
                    >
                      <RefreshCw className="size-3" strokeWidth={1.5} aria-hidden />
                      <span>Check updates</span>
                    </Button>
                  )}
                </div>
              </div>
            </div>
          </div>
        </main>

        {!narrow && (
          <div className="flex-none border-l border-hairline" style={{ width: shelfCollapsed ? "auto" : 300 }}>
            <PartsShelf
              gameName={shell.gameName}
              builtTitles={builtTitles}
              place={place}
              error={place === "error"}
              collapsed={shelfCollapsed}
              onToggle={() => setShelfCollapsed((c) => !c)}
              answerAttr
            />
          </div>
        )}

        <Toaster position="bottom-right" theme={theme} />
      </div>
    </MotionConfig>
  );
}
