import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import api from './api';
import { Dialog } from './Dialog';
import { diffPreview } from './textDiff';
import { draftAfterSend, dropMissing, followupText, nextFollowup, recordResolution, resultSession, showsResultIn, trackStaged, type FollowupBatch } from './approvalFollowup';
import { approvalCopy, providerInitials } from './labels';
import { providerOptions, type ProviderKind, type ProviderOption } from '../electron/providerCatalog';
import type { SessionSnapshot, StoredChatSession } from '../electron/sessions';
import type { AgentTurnResult, ApprovalResult, ChatMessage, FileEntry, PendingApprovalView, StoredProviderView } from '../electron/types';

type Screen = 'chat' | 'models' | 'workspace' | 'integrations';

type ChatSession = StoredChatSession;

type ProviderForm = { kind: ProviderKind | null; model: string; baseUrl: string; apiKey: string };

const SUGGESTIONS: Array<{ title: string; body: string; prompt: string }> = [
  { title: 'Explore this project', body: 'Map the folder structure and summarize what each script does.', prompt: 'List the files in my project and summarize the structure.' },
  { title: 'Add a feature', body: 'Plan a small change, then review the diff before writing it.', prompt: 'Propose a small improvement to this project and show me the diff before writing anything.' },
  { title: 'Explain a file', body: 'Pick a script and walk through what it does, step by step.', prompt: 'Pick one script from my project and explain what it does.' },
  { title: 'Set up a test command', body: 'Find a safe way to check that the game still builds.', prompt: 'Suggest a safe command to validate this project. Stage it for my approval; do not run it unasked.' },
];

const NAV: Array<{ id: Screen; label: string; icon: string }> = [
  { id: 'chat', label: 'Chat', icon: 'chat' },
  { id: 'models', label: 'Models', icon: 'models' },
  { id: 'workspace', label: 'Workspace', icon: 'workspace' },
  { id: 'integrations', label: 'Integrations', icon: 'integrations' },
];

const ICONS: Record<string, ReactNode> = {
  chat: <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M2 3h12v8H6l-3 3V3z" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" /></svg>,
  models: <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><rect x="2" y="2" width="5" height="5" rx="1.5" stroke="currentColor" strokeWidth="1.3" /><rect x="9" y="2" width="5" height="5" rx="1.5" stroke="currentColor" strokeWidth="1.3" /><rect x="2" y="9" width="5" height="5" rx="1.5" stroke="currentColor" strokeWidth="1.3" /><rect x="9" y="9" width="5" height="5" rx="1.5" stroke="currentColor" strokeWidth="1.3" /></svg>,
  workspace: <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M2 13V3h4l1.5 2H14v8H2z" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" /></svg>,
  integrations: <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><rect x="2" y="5" width="12" height="6" rx="3" stroke="currentColor" strokeWidth="1.3" /><circle cx="11" cy="8" r="1.6" stroke="currentColor" strokeWidth="1.2" /></svg>,
  spark: <svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M8 1.5 9.6 6l4.4 1.6L9.6 9 8 13.5 6.4 9 2 7.6 6.4 6 8 1.5z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" /></svg>,
  folder: <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M2 13V3h4l1.5 2H14v8H2z" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" /></svg>,
  file: <svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="M4 1.5h5l3 3v10H4v-13z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" /></svg>,
  check: <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M2.5 8.5 6 12l7.5-8" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" /></svg>,
  plus: <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M8 3v10M3 8h10" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" /></svg>,
  chevron: <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="m4 6 4 4 4-4" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" /></svg>,
  shield: <svg width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="M8 1.5 13 3.5v4c0 3.6-2.2 6-5 7-2.8-1-5-3.4-5-7v-4l5-2z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" /></svg>,
  x: <svg width="12" height="12" viewBox="0 0 16 16" fill="none"><path d="m3 3 10 10M13 3 3 13" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" /></svg>,
};

function providerStatus(view: StoredProviderView, option: ProviderOption | undefined): string {
  if (option?.connection === 'cli') {
    return 'Official CLI sign-in, no key stored here';
  }
  if (option?.keyless) {
    return 'Local endpoint, no API key';
  }
  return view.hasKey ? 'API key stored with the OS' : 'Saved without a key';
}


export default function App() {
  const [screen, setScreen] = useState<Screen>('chat');
  const [providerViews, setProviderViews] = useState<StoredProviderView[]>([]);
  const [catalog, setCatalog] = useState<ProviderOption[]>(providerOptions);
  const [storage, setStorage] = useState<{ secure: boolean; services: number } | null>(null);
  const [sessions, setSessions] = useState<ChatSession[]>([]);
  const [activeSession, setActiveSession] = useState<string | null>(null);
  const [draft, setDraft] = useState('');
  const [sending, setSending] = useState(false);
  const [banner, setBanner] = useState<string | null>(null);
  const [projectRoot, setProjectRoot] = useState<string | null>(null);
  const [projectEntries, setProjectEntries] = useState<Map<string, FileEntry[]>>(new Map());
  const [expandedDirs, setExpandedDirs] = useState<Set<string>>(new Set());
  const [fileView, setFileView] = useState<{ rel: string; content: string } | null>(null);
  const [checkpoints, setCheckpoints] = useState<string[]>([]);
  const [studioCheckpoints, setStudioCheckpoints] = useState<string[]>([]);
  const [capabilities, setCapabilities] = useState<Awaited<ReturnType<typeof api.detectCapabilities>>>([]);
  const [mcpState, setMcpState] = useState<{ running: boolean; tools: Awaited<ReturnType<typeof api.mcpStatus>>['tools'] }>({ running: false, tools: [] });
  const [mcpForm, setMcpForm] = useState({ command: '', args: '' });
  const [expandedCards, setExpandedCards] = useState<Set<string>>(new Set());
  const [providerModal, setProviderModal] = useState<ProviderForm>(resetForm());
  const [providerTest, setProviderTest] = useState<{ kind: ProviderKind; ok: boolean; detail: string } | null>(null);
  const [approvals, setApprovals] = useState<PendingApprovalView[]>([]);
  /** The last decision's result and the chat it belongs to (null when the user started the action). */
  const [lastResult, setLastResult] = useState<{ sessionId: string | null; result: ApprovalResult } | null>(null);
  const actionResult = lastResult && showsResultIn(lastResult.sessionId, activeSession) ? lastResult.result : null;
  /** Approval cards whose whole diff the user opened; a cut diff must be opened before it can be approved. */
  const [expandedDiffs, setExpandedDiffs] = useState<Set<string>>(new Set());
  const [sessionsReady, setSessionsReady] = useState(false);
  const [bridgeMissing, setBridgeMissing] = useState(false);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const sessionsRef = useRef<ChatSession[]>([]);
  // Approvals each agent turn staged; once all are decided the results go back to that chat.
  const followupsRef = useRef<FollowupBatch[]>([]);
  const [readyFollowups, setReadyFollowups] = useState<FollowupBatch[]>([]);

  const activeSessionData = useMemo(() => sessions.find((session) => session.id === activeSession) ?? null, [sessions, activeSession]);
  const activeProvider = useMemo(() => providerViews.find((view) => view.active) ?? null, [providerViews]);
  const catalogByKind = useMemo(() => new Map(catalog.map((option) => [option.kind, option])), [catalog]);

  useEffect(() => {
    sessionsRef.current = sessions;
  }, [sessions]);

  useEffect(() => {
    void (async () => {
      try {
        const saved = await api.loadSessions();
        if (saved.sessions.length > 0) {
          setSessions(saved.sessions);
          setActiveSession(saved.activeId);
        }
      } catch {
        // The browser preview has no preload bridge. The shell still renders.
      } finally {
        setSessionsReady(true);
      }
      await refreshAll();
    })();
  }, []);

  useEffect(() => {
    if (!sessionsReady) {
      return;
    }
    const snapshot: SessionSnapshot = { activeId: activeSession, sessions };
    const handle = setTimeout(() => {
      void api.saveSessions(snapshot).catch(() => undefined);
    }, 250);
    return () => clearTimeout(handle);
  }, [sessions, activeSession, sessionsReady]);
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [activeSessionData?.messages.length, sending]);

  async function refreshAll() {
    if (typeof window === 'undefined' || !window.openro) {
      setBridgeMissing(true);
      return;
    }
    try {
      const [views, cat, store, root, caps, mcp] = await Promise.all([
        api.providers(),
        api.catalog(),
        api.storageState(),
        api.projectRoot(),
        api.detectCapabilities(),
        api.mcpStatus(),
      ]);
      setProviderViews(views);
      setCatalog(cat);
      setStorage(store);
      setProjectRoot(root);
      setCapabilities(caps);
      setMcpState(mcp);
      setApprovals(await api.approvals());
    } catch (error) {
      setBanner(asMessage(error));
    }
  }

  function resetForm(kind: ProviderKind | null = null): ProviderForm {
    const option = kind ? catalogByKind.get(kind) : undefined;
    return { kind, model: '', baseUrl: option?.defaultBaseUrl ?? '', apiKey: '' };
  }

  function openProviderModal(kind: ProviderKind) {
    const existing = providerViews.find((view) => view.kind === kind);
    const option = catalogByKind.get(kind);
    setProviderTest(null);
    setProviderModal({
      kind,
      model: existing?.model ?? '',
      baseUrl: existing?.baseUrl ?? option?.defaultBaseUrl ?? '',
      apiKey: '',
    });
  }

  async function saveProvider() {
    if (!providerModal.kind) {
      return;
    }
    setProviderTest(null);
    try {
      await api.saveProvider({
        kind: providerModal.kind,
        model: providerModal.model.trim(),
        baseUrl: providerModal.baseUrl.trim() ? providerModal.baseUrl.trim() : undefined,
        apiKey: providerModal.apiKey.trim() ? providerModal.apiKey.trim() : undefined,
      });
      await refreshAll();
      setProviderTest({ kind: providerModal.kind, ok: true, detail: 'Connection saved. Chains are not billed until you send a prompt.' });
    } catch (error) {
      setProviderTest({ kind: providerModal.kind, ok: false, detail: asMessage(error) });
    }
  }

  async function testProvider() {
    if (!providerModal.kind) {
      return;
    }
    setProviderTest({ kind: providerModal.kind, ok: false, detail: 'Testing connection…' });
    try {
      const result = await api.testProvider({
        kind: providerModal.kind,
        model: providerModal.model.trim(),
        baseUrl: providerModal.baseUrl.trim() || undefined,
        apiKey: providerModal.apiKey.trim() || undefined,
      });
      setProviderTest({ kind: providerModal.kind, ok: result.ok, detail: result.detail });
    } catch (error) {
      setProviderTest({ kind: providerModal.kind, ok: false, detail: asMessage(error) });
    }
  }

  async function removeProvider(kind: ProviderKind) {
    await api.deleteProvider(kind);
    await refreshAll();
  }

  function newChat() {
    const session: ChatSession = { id: `chat-${Date.now()}`, title: 'New chat', updatedAt: new Date().toISOString(), messages: [] };
    setSessions((prev) => [session, ...prev]);
    setActiveSession(session.id);
    setScreen('chat');
  }

  async function sendDraft(textOverride?: string) {
    const text = (textOverride ?? draft).trim();
    if (!text || sending) {
      return;
    }
    const prior = sessionsRef.current;
    let sessionId = activeSession;
    let historySource: ChatSession['messages'];
    if (!sessionId) {
      const fresh: ChatSession = { id: `chat-${Date.now()}`, title: text.slice(0, 40) || 'New chat', updatedAt: new Date().toISOString(), messages: [{ role: 'user', content: text }] };
      sessionId = fresh.id;
      historySource = fresh.messages;
      setSessions((prev) => [fresh, ...prev]);
      setActiveSession(fresh.id);
    } else {
      const current = prior.find((session) => session.id === sessionId);
      historySource = [...(current?.messages ?? []), { role: 'user', content: text }];
      setSessions((prev) =>
        prev.map((session) =>
          session.id === sessionId
            ? {
                ...session,
                updatedAt: new Date().toISOString(),
                title: session.messages.length === 0 ? text.slice(0, 40) : session.title,
                messages: historySource,
              }
            : session,
        ),
      );
    }
    const history: ChatMessage[] = historySource.map((message): ChatMessage => ({ role: message.role, content: message.content }));
    setDraft((current) => draftAfterSend(current, textOverride));
    setSending(true);
    try {
      const result = await api.sendPrompt({ sessionId, messages: history });
      setSessions((prev) =>
        prev.map((session) =>
          session.id === sessionId
            ? {
                ...session,
                messages: [
                  ...session.messages,
                  { role: 'assistant', content: result.assistantText || '(no text in response)', trace: result.toolTrace, error: result.ok ? undefined : result.error },
                ],
              }
            : session,
        ),
      );
      if (result.toolTrace.some((entry) => entry.status === 'staged')) {
        followupsRef.current = trackStaged(followupsRef.current, sessionId, result.toolTrace.map((entry) => (entry.status === 'staged' ? entry.approvalId ?? '' : '')));
        setApprovals(await api.approvals());
      }
    } catch (error) {
      setSessions((prev) =>
        prev.map((session) =>
          session.id === sessionId
            ? { ...session, messages: [...session.messages, { role: 'assistant', content: '', error: asMessage(error) }] }
            : session,
        ),
      );
    } finally {
      setSending(false);
    }
  }

  async function refreshWorkspaceState() {
    try {
      setCheckpoints(await api.checkpoints());
      setStudioCheckpoints(await api.studioCheckpoints());
      const root = projectRoot ?? (await api.projectRoot());
      setProjectRoot(root);
      if (root) {
        const entries = await api.listProject('.');
        setProjectEntries((prev) => new Map(prev).set('.', entries));
      }
    } catch (error) {
      setBanner(asMessage(error));
    }
  }

  async function toggleDir(rel: string) {
    const next = new Set(expandedDirs);
    if (next.has(rel)) {
      next.delete(rel);
    } else {
      next.add(rel);
      if (!projectEntries.has(rel)) {
        try {
          const entries = await api.listProject(rel);
          setProjectEntries((prev) => new Map(prev).set(rel, entries));
        } catch (error) {
          setBanner(asMessage(error));
        }
      }
    }
    setExpandedDirs(next);
  }

  async function openFile(rel: string) {
    try {
      setFileView({ rel, content: await api.readProject(rel) });
    } catch (error) {
      setBanner(asMessage(error));
    }
  }

  async function selectProject() {
    const root = await api.selectProjectRoot();
    if (root) {
      setProjectRoot(root);
      setExpandedDirs(new Set());
      setProjectEntries(new Map());
      await refreshWorkspaceState();
    }
  }

  async function restoreCheckpoint(file: string) {
    try {
      await api.restoreCheckpoint(file);
      setApprovals(await api.approvals());
      setBanner(`Staged the restore of ${file}. Review it in the approval tray; nothing changes until you approve.`);
    } catch (error) {
      setBanner(asMessage(error));
    }
  }

  async function restoreStudioCheckpoint(file: string) {
    try {
      await api.restoreStudioCheckpoint(file);
      setApprovals(await api.approvals());
      setBanner('Staged the Studio restore. Review it in Chat; nothing changes in Studio until you approve.');
    } catch (error) {
      setBanner(asMessage(error));
    }
  }

  function settleFollowups(next: { batches: FollowupBatch[]; ready: FollowupBatch[] }, live: PendingApprovalView[]) {
    const swept = dropMissing(next.batches, live.map((item) => item.id));
    followupsRef.current = swept.batches;
    const ready = [...next.ready, ...swept.ready];
    if (ready.length > 0) {
      setReadyFollowups((prev) => [...prev, ...ready]);
    }
  }

  async function resolveApproval(id: string, decision: 'approve' | 'reject') {
    try {
      const owner = resultSession(followupsRef.current, id);
      const result = await api.resolveApproval({ id, decision });
      setLastResult({ sessionId: owner, result });
      const live = await api.approvals();
      setApprovals(live);
      settleFollowups(recordResolution(followupsRef.current, id, result), live);
      if (decision === 'approve') {
        await refreshWorkspaceState();
        setMcpState(await api.mcpStatus());
      }
    } catch (error) {
      setBanner(asMessage(error));
      const live = await api.approvals().catch(() => null);
      setApprovals(live ?? []);
      // If the approval left the queue, it was attempted; tell the agent it failed.
      if (live && !live.some((item) => item.id === id)) {
        settleFollowups(recordResolution(followupsRef.current, id, { ok: false, decision, summary: asMessage(error) }), live);
      }
    }
  }

  // Send each settled batch back to the chat that staged it, once that chat is open and idle.
  useEffect(() => {
    const next = nextFollowup(readyFollowups, activeSession, sending);
    if (!next) {
      return;
    }
    setReadyFollowups((prev) => prev.filter((batch) => batch !== next));
    setLastResult(null);
    setScreen('chat');
    void sendDraft(followupText(next));
  }, [readyFollowups, sending, activeSession]);

  async function connectMcp() {
    if (!mcpForm.command.trim()) {
      return;
    }
    try {
      const args = mcpForm.args.trim() ? mcpForm.args.split(/\s+/) : [];
      await api.mcpConnect({ command: mcpForm.command.trim(), args });
      setApprovals(await api.approvals());
      setBanner('Review the server command in the approval tray. Nothing has started yet.');
    } catch (error) {
      setBanner(asMessage(error));
    }
  }

  async function disconnectMcp() {
    await api.mcpDisconnect();
    setMcpState({ running: false, tools: [] });
  }

  const hasProvider = providerViews.some((view) => view.hasKey || catalogByKind.get(view.kind)?.keyless || catalogByKind.get(view.kind)?.connection === 'cli');

  return (
    <div className="shell">
      <a className="skip-link" href="#main">Skip to content</a>
      <aside className="sidebar">
        <div className="brand-row">
          <div className="brand-mark">OR</div>
          <div className="brand-lockup">
            <strong>Open-Ro</strong>
            <span>Studio 0.1.0</span>
          </div>
          <button type="button" className="icon-btn small more" aria-label="Open the Open-Ro repository" onClick={() => void api.openExternal('https://github.com/cayleb-james2008/open-ro')}>
            {ICONS.chevron}
          </button>
        </div>
        <button type="button" className={`project-switch ${projectRoot ? 'has-project' : ''}`} onClick={() => void selectProject()}>
          {ICONS.folder}
          <span>{projectRoot ? projectRoot.split(/[\\/]/).pop() : 'Choose project folder…'}</span>
        </button>
        <div className="nav-label">Workspace</div>
        <nav className="side-nav" aria-label="Primary">
          {NAV.map((item) => (
            <button type="button" key={item.id} className={`nav-item ${screen === item.id ? 'active' : ''}`} aria-current={screen === item.id ? 'page' : undefined} onClick={() => setScreen(item.id)}>
              {ICONS[item.icon]}
              <span>{item.label}</span>
            </button>
          ))}
        </nav>
        <button type="button" className="new-chat-btn" onClick={newChat}>
          {ICONS.plus}
          <span>New chat</span>
        </button>
        <div className="recent-label nav-label">Recent</div>
        <div className="recent-chat-list">
          {sessions.map((session) => (
            <button type="button" key={session.id} className={`recent-chat ${session.id === activeSession ? 'active' : ''}`} aria-current={session.id === activeSession ? 'true' : undefined} onClick={() => { setActiveSession(session.id); setScreen('chat'); }}>
              {ICONS.chat}
              <span>{session.title}</span>
            </button>
          ))}
          {sessions.length === 0 && <p className="recent-empty">No chats yet. History is saved on this computer.</p>}
        </div>
        <div className="side-spacer" />
        <div className="connection-card">
          <div className="connection-head">
            <span className={`status-dot ${activeProvider ? 'on' : ''}`} />
            Model connection
          </div>
          <div className="connection-model">{activeProvider ? `${catalogByKind.get(activeProvider.kind)?.name ?? activeProvider.kind}${activeProvider.model ? ` · ${activeProvider.model}` : ''}` : 'No model connected'}</div>
          <button type="button" className="connection-action" onClick={() => setScreen('models')}>
            {activeProvider ? 'Manage providers' : 'Connect a model'}
            <span>→</span>
          </button>
        </div>
        <div className="sidebar-footer">
          {ICONS.shield}
          <span>Local-first · credentials stay in OS secure storage</span>
        </div>
      </aside>

      <main className="main-area" id="main">
        <div className="sr-only" aria-live="polite">{sending ? 'The agent is working.' : actionResult ? actionResult.summary : ''}</div>
        <div className="topbar">
          <div className="crumb">
            <strong>Open-Ro</strong>
            <span className="slash">/</span>
            <span>{screen === 'chat' ? (activeSessionData?.title ?? 'Chat') : screen === 'models' ? 'Models & providers' : screen === 'workspace' ? 'Workspace' : 'Integrations'}</span>
          </div>
          <div className="top-actions">
            <span className="project-pill" title={projectRoot ?? undefined}>{projectRoot ?? 'No project'}</span>
            <span className={`runtime-pill ${sending ? 'working' : ''}`} role="status">{sending ? 'Agent working' : approvals.length > 0 ? `${approvals.length} pending approval${approvals.length === 1 ? '' : 's'}` : 'Idle'}</span>
          </div>
        </div>

        {banner && (
          <div className="error-banner" role="alert">
            <span>{banner}</span>
            <button type="button" onClick={() => setBanner(null)} aria-label="Dismiss notice">{ICONS.x}</button>
          </div>
        )}

        {(approvals.length > 0 || actionResult) && <ApprovalTray />}

        {screen === 'chat' && <ChatPage />}
        {screen === 'models' && <ModelsPage />}
        {screen === 'workspace' && <WorkspacePage />}
        {screen === 'integrations' && <IntegrationsPage />}
      </main>

      {providerModal.kind && <ProviderModal />}
      {fileView && (
        <Dialog labelledBy="file-title" className="file-modal" onClose={() => setFileView(null)}>
          <div className="modal-title">
            <div>
              <strong id="file-title">{fileView.rel}</strong>
              <small>{fileView.content.length} characters, read only</small>
            </div>
            <button type="button" className="icon-btn" aria-label="Close file preview" onClick={() => setFileView(null)}>{ICONS.x}</button>
          </div>
          <pre tabIndex={0}>{fileView.content}</pre>
        </Dialog>
      )}
    </div>
  );

  function ApprovalTray() {
    return (
      <section className="approval-tray" aria-label="Pending approvals">
        {actionResult && (
          <div className={`test-result ${actionResult.ok ? 'success' : 'failed'}`}>
            <div>
              <strong>{actionResult.summary}</strong>
              {actionResult.output && <pre>{actionResult.output}</pre>}
              <div className="modal-actions">
                <button type="button" className="outline-btn" onClick={() => setLastResult(null)}>Dismiss</button>
              </div>
            </div>
          </div>
        )}
        {approvals.map((item) => {
          const showsDiff = item.kind === 'write' || item.studioOp === 'write';
          const { rows, hidden, invisibleChange } = showsDiff ? diffPreview(item.previous ?? '', item.content ?? '', expandedDiffs.has(item.id)) : { rows: [], hidden: 0, invisibleChange: false };
          const copy = approvalCopy(item);
          return (
            <article className="approval-card" key={item.id}>
              <header>
                <strong>{copy.title}</strong>
                <span>{copy.detail}</span>
              </header>
              {!showsDiff ? <pre className="command-preview" aria-label={item.kind === 'mcp' ? 'Tool arguments' : 'Command'}>{copy.preview}</pre> : (
                <pre className="diff" aria-label="Proposed diff">
                  {rows.map((row, index) => (
                    <span className={`diff-${row.kind}`} key={`${item.id}-${index}`}>{row.kind === 'add' ? '+ ' : row.kind === 'del' ? '- ' : '  '}{row.text}{'\n'}</span>
                  ))}
                </pre>
              )}
              {invisibleChange && <p className="diff-note">Only line endings or other invisible characters change.</p>}
              {hidden > 0 && (
                <button type="button" className="text-btn" onClick={() => setExpandedDiffs((prev) => new Set(prev).add(item.id))}>
                  +{hidden} more lines. Show the whole diff to approve.
                </button>
              )}
              <div className="modal-actions">
                <button type="button" className="outline-btn" onClick={() => void resolveApproval(item.id, 'reject')}>Reject</button>
                <button type="button" className="primary-btn" disabled={hidden > 0} title={hidden > 0 ? 'Show the whole diff first' : undefined} onClick={() => void resolveApproval(item.id, 'approve')}>{copy.approveLabel}</button>
              </div>
            </article>
          );
        })}
      </section>
    );
  }

  // ---- Chat ------------------------------------------------------------
  function ChatPage() {
    return (
      <div className="chat-page">
        {!activeSessionData || activeSessionData.messages.length === 0 ? (
          <div className="chat-scroll" ref={scrollRef}>
            <div className="welcome-wrap">
              <span className="welcome-chip">Local-first Roblox workspace</span>
              <h1>
                What are we <span>building<span>?</span></span>
              </h1>
              <p className="welcome-sub">
                Start with a free local model: Ollama or llama.cpp on this computer. Paid APIs are optional. Pick a project folder to give the agent file tools. Open-Ro has no account and does not send telemetry.
              </p>
              <div className="setup-row">
                <button type="button" className={`setup-tile ${activeProvider ? 'complete' : ''}`} onClick={() => setScreen('models')}>
                  <span className="tile-icon">{activeProvider ? ICONS.check : ICONS.spark}</span>
                  <span>
                    <b>Connect a model</b>
                    <small>{activeProvider ? catalogByKind.get(activeProvider.kind)?.name : 'Ollama or llama.cpp. No account.'}</small>
                  </span>
                  {activeProvider && <span className="tile-check" aria-hidden="true">{ICONS.check}</span>}
                </button>
                <button type="button" className={`setup-tile ${projectRoot ? 'complete' : ''}`} onClick={() => setScreen('workspace')}>
                  <span className="tile-icon">{projectRoot ? ICONS.check : ICONS.folder}</span>
                  <span>
                    <b>Select project</b>
                    <small>{projectRoot ? projectRoot : 'Optional — enables file tools'}</small>
                  </span>
                </button>
              </div>
              <div className="suggestion-grid">
                {SUGGESTIONS.map((item) => (
                  <button type="button" key={item.title} onClick={() => setDraft(item.prompt)}>
                    {ICONS.spark}
                    <span>
                      <b>{item.title}</b>
                      <small>{item.body}</small>
                    </span>
                  </button>
                ))}
              </div>
              <div className="welcome-foot">
                {ICONS.shield}
                <span>Every file write, command and MCP tool call needs your approval — nothing runs unchecked.</span>
              </div>
            </div>
            <Composer />
          </div>
        ) : (
          <>
            <div className="chat-scroll" ref={scrollRef}>
              <div className="message-list">
                {activeSessionData.messages.map((message, index) => (
                  <div className="message-row" key={index}>
                    <span className={`message-avatar ${message.role === 'assistant' ? 'assistant' : ''}`}>{message.role === 'assistant' ? 'RO' : 'YOU'}</span>
                    <div className="message-content">
                      <div className="message-author">
                        {message.role === 'assistant' ? 'Open-Ro' : 'You'}
                        {message.trace && message.trace.length > 0 && <span>{message.trace.map((entry) => entry.status).join(', ')}</span>}
                      </div>
                      {message.error ? (
                        <div className="test-result failed">{message.error}</div>
                      ) : (
                        <>
                          <div className="message-text">{message.content}</div>
                          {message.trace && message.trace.length > 0 && (
                            <div className="integration-summary">
                              <div>
                                <span className="status-dot on" />
                                Tool activity
                              </div>
                              <p>
                                {message.trace.map((entry) => `[${entry.status}] ${entry.name} ${entry.detail ? `— ${entry.detail}` : ''}`).join('\n')}
                              </p>
                            </div>
                          )}
                        </>
                      )}
                    </div>
                  </div>
                ))}
                {sending && (
                  <div className="message-row">
                    <span className="message-avatar assistant">RO</span>
                    <div className="message-content">
                      <div className="thinking">
                        <span /><span /><span />
                      </div>
                    </div>
                  </div>
                )}
              </div>
            </div>
            <Composer />
          </>
        )}
      </div>
    );
  }

  function Composer() {
    return (
      <div className="composer-wrap">
        {bridgeMissing && (
          <div className="composer-notice" role="status">
            <span>Desktop bridge is not loaded. Start Open-Ro with npm run dev to connect a model and a project folder.</span>
          </div>
        )}
        {!bridgeMissing && storage && !storage.secure && (
          <div className="composer-notice">
            <span>Keyring not detected. API keys cannot be saved securely until a desktop secret service is available.</span>
          </div>
        )}
        <div className="composer">
          <label className="sr-only" htmlFor="composer-input">Message</label>
          <textarea
            id="composer-input"
            placeholder="Describe a change, ask a question, or propose an experiment"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' && !event.shiftKey) {
                event.preventDefault();
                void sendDraft();
              }
            }}
          />
          <div className="composer-bottom">
            <div className="composer-hints">
              <span>↵ send · ⇧↵ newline</span>
              {hasProvider ? <span className="provider-mini">{activeProvider ? `${catalogByKind.get(activeProvider.kind)?.name}${activeProvider.model ? ` · ${activeProvider.model}` : ''}` : 'provider saved — set active'}</span> : <span>Connect a model first →</span>}
            </div>
            <div className="composer-right">
              <button type="button" className="send-btn" disabled={!draft.trim() || sending} onClick={() => void sendDraft()} aria-label="Send message">
                <svg width="14" height="14" viewBox="0 0 16 16" fill="none"><path d="M2 8h11M9 3.5 13.5 8 9 12.5" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" /></svg>
              </button>
            </div>
          </div>
        </div>
        <div className="composer-disclaimer">Model requests are sent to your selected provider. Project files stay local unless the model reads them.</div>
      </div>
    );
  }

  // ---- Models ------------------------------------------------------------
  function ModelsPage() {
    return (
      <div className="content-page">
        <div className="page-heading">
          <div>
            <span className="eyebrow">Model connections</span>
            <h1>Models & providers</h1>
            <p>The default path is a local model on this machine. Paid APIs and subscription CLIs are optional. API keys, when you choose one, are stored with Electron safeStorage. Open-Ro does not require an account and does not send telemetry.</p>
          </div>
          <div className="heading-actions">
            <span className="outline-btn" style={{ cursor: 'default' }}>{bridgeMissing || !storage ? 'Secure storage: open the desktop app' : storage.secure ? 'Secure storage available' : 'No desktop keyring'}</span>
          </div>
        </div>

        <div className="notice-card">
          <span className="notice-icon">{ICONS.shield}</span>
          <div>
            <b>Billing boundary</b>
            <p>Usage is billed by the provider you connect. Open-Ro sends requests only to your selected provider endpoint; subscription sign-in happens entirely inside the official CLI.</p>
          </div>
        </div>

        <div className="section-title">
          <span>Connected providers</span>
          <span>{providerViews.length} saved</span>
        </div>
        <div className="provider-list">
          {providerViews.map((view) => {
            const option = catalogByKind.get(view.kind);
            return (
              <div className={`provider-row ${view.active ? 'selected' : ''}`} key={view.kind}>
                <span className={`provider-logo ${view.kind}`}>{providerInitials(option?.name ?? view.kind)}</span>
                <span className="provider-info">
                  <b>{option?.name ?? view.kind}</b>
                  <small>{providerStatus(view, option)}{view.model ? ` · ${view.model}` : ''}</small>
                </span>
                {view.active && <span className="active-tag">Active</span>}
                <button type="button" className="outline-btn" onClick={() => openProviderModal(view.kind)}>Edit</button>
                {!view.active && <button type="button" className="outline-btn" onClick={async () => { await api.setActive(view.kind); await refreshAll(); }}>Use</button>}
                <button type="button" className="icon-btn small danger" aria-label={`Remove ${option?.name ?? view.kind}`} onClick={() => void removeProvider(view.kind)}>{ICONS.x}</button>
              </div>
            );
          })}
          {providerViews.length === 0 && (
            <div className="empty-state">
              <span className="empty-icon">{ICONS.spark}</span>
              <b>No provider saved yet</b>
              <p>Connect one of the routes below. You can skip everything else and still use chat.</p>
            </div>
          )}
        </div>

        <div className="section-title provider-section-head">
          <span>Available routes</span>
          <span>official setup links</span>
        </div>
        <div className="provider-catalog">
          {catalog.map((option) => (
            <div className="provider-choice" key={option.kind}>
              <span className={`provider-logo ${option.kind}`}>{providerInitials(option.name)}</span>
              <span className="choice-copy">
                <b>{option.name}</b>
                <small>{option.setup}</small>
                <span className="choice-cost">{option.costNote}</span>
              </span>
              <button type="button" className="outline-btn" onClick={() => openProviderModal(option.kind)}>Connect</button>
            </div>
          ))}
        </div>
        <div className="footnote">
          {ICONS.shield}
          <span>Open-Ro never reads provider credential stores and never scrapes subscription tokens. See docs/AUTH.md for each official flow.</span>
        </div>
      </div>
    );
  }

  function ProviderModal() {
    const option = providerModal.kind ? catalogByKind.get(providerModal.kind) : undefined;
    const kind = providerModal.kind as ProviderKind;
    const isCli = option?.connection === 'cli';
    const test = providerTest && providerTest.kind === kind ? providerTest : null;
    return (
      <Dialog labelledBy="provider-title" className="provider-modal" onClose={() => setProviderModal(resetForm())}>
          <div className="modal-title">
            <div>
              <strong id="provider-title">{option?.name}</strong>
              <small>{option?.setup}</small>
            </div>
            <button type="button" className="icon-btn" aria-label="Close provider setup" onClick={() => setProviderModal(resetForm())}>{ICONS.x}</button>
          </div>
          <div className="provider-steps">
            <div className="steps-heading">Setup steps</div>
            {(option?.setup ?? '').split(/[.;]/).filter((step) => step.trim()).map((step, index) => (
              <div className="step-line" key={index}>
                <span>{index + 1}</span>
                <p>{step.trim()}</p>
              </div>
            ))}
            <a className="link-btn" href={option?.url} target="_blank" rel="noreferrer">
              Open official page ↗
            </a>
          </div>
          {isCli ? (
            <div className="cli-card">
              <div>
                <b>Official CLI route</b>
                <p>Open-Ro launches <code>{option?.cliCommand}</code> in your project dir and reports the result. Sign-in state and subscription limits are handled inside the official CLI; Open-Ro does not read its auth files or scrape tokens.</p>
              </div>
            </div>
          ) : null}
          <div className="form-stack">
            <label>
              Model ID {option?.modelHint ? <em style={{ color: 'var(--color-dim)' }}>({option.modelHint})</em> : null}
              <input value={providerModal.model} placeholder={option?.modelHint} onChange={(event) => setProviderModal({ ...providerModal, model: event.target.value })} />
            </label>
            {!isCli && (
              <label>
                API key {option?.keyless ? '(not required for local Ollama)' : ''}
                <input type="password" value={providerModal.apiKey} placeholder={option?.keyless ? 'no key needed' : 'paste key — stored via safeStorage'} onChange={(event) => setProviderModal({ ...providerModal, apiKey: event.target.value })} />
              </label>
            )}
            {(option?.needsBaseUrl || option?.kind === 'ollama') && (
              <label>
                Base URL
                <input value={providerModal.baseUrl} placeholder="http://127.0.0.1:11434/v1" onChange={(event) => setProviderModal({ ...providerModal, baseUrl: event.target.value })} />
              </label>
            )}
          </div>
          <div className="cost-note">
            {ICONS.shield}
            <span>{option?.costNote}</span>
          </div>
          {test && <div className={`test-result ${test.ok ? 'success' : 'failed'}`}>{test.detail}</div>}
          <div className="modal-actions">
            <button type="button" className="text-btn" onClick={() => void testProvider()}>Check connection</button>
            <button type="button" className="outline-btn" onClick={() => setProviderModal(resetForm())}>Cancel</button>
            <button type="button" className="primary-btn" onClick={() => void saveProvider()}>Save and set active</button>
          </div>
      </Dialog>
    );
  }

  // ---- Workspace ------------------------------------------------------------
  function WorkspacePage() {
    return (
      <div className="content-page">
        <div className="page-heading">
          <div>
            <span className="eyebrow">Local project</span>
            <h1>Workspace</h1>
            <p>File browsing is read-only in 0.1.0; agent write proposals are reviewed before anything lands, and every accepted write leaves a checkpoint.</p>
          </div>
          <div className="heading-actions">
            <button type="button" className="outline-btn" onClick={() => void refreshWorkspaceState()}>Refresh</button>
            <button type="button" className="primary-btn" onClick={() => void selectProject()}>Choose folder</button>
          </div>
        </div>

        <div className="integration-summary">
          <div>
            <span className="status-dot on" />
            {projectRoot ?? 'No project folder selected'}
          </div>
          <p>{projectRoot ? 'Agent tools run relative to this root and are blocked from escaping it.' : 'Pick a folder to enable project-aware chats.'}</p>
        </div>

        <div className="section-title">
          <span>Files</span>
          <span>{projectEntries.get('.')?.length ?? 0} top-level entries</span>
        </div>
        {projectRoot && projectEntries.get('.') && (
          projectEntries.get('.')!.length === 0 ? (
            <div className="empty-state">
              <span className="empty-icon">{ICONS.folder}</span>
              <b>This folder has no visible files</b>
              <p>Build folders, .git, and likely secret files such as .env are hidden.</p>
            </div>
          ) : <FileTree dir="." depth={0} />
        )}

        <div className="section-title" style={{ marginTop: 28 }}>
          <span>Checkpoints</span>
          <span>restoring needs approval</span>
        </div>
        <div className="integration-list">
          {checkpoints.map((file) => (
            <div className="integration-card" key={file}>
              <button type="button" className="integration-top" onClick={() => void restoreCheckpoint(file)}>
                <span className="integration-icon">{ICONS.shield}</span>
                <span className="integration-copy">
                  <b>{file}</b>
                  <small>File before an accepted write. Restoring needs approval</small>
                </span>
                <span className="state-pill">stage restore</span>
              </button>
            </div>
          ))}
          {checkpoints.length === 0 && studioCheckpoints.length === 0 && (
            <div className="empty-state">
              <span className="empty-icon">{ICONS.shield}</span>
              <b>No checkpoints yet</b>
              <p>Each approved agent write records the previous file state here first.</p>
            </div>
          )}
          {studioCheckpoints.map((file) => (
            <div className="integration-card" key={`studio-${file}`}>
              <button type="button" className="integration-top" onClick={() => void restoreStudioCheckpoint(file)}>
                <span className="integration-icon">{ICONS.shield}</span>
                <span className="integration-copy">
                  <b>{file}</b>
                  <small>Studio script before an approved write. Restoring needs approval</small>
                </span>
                <span className="state-pill">stage restore</span>
              </button>
            </div>
          ))}
        </div>
      </div>
    );
  }

  // ---- Integrations ------------------------------------------------------------
  function IntegrationsPage() {
    const groups: Array<{ title: string; category: string }> = [
      { title: 'Roblox Studio', category: 'studio' },
      { title: 'Project tools', category: 'vcs' },
      { title: 'Asset tools', category: 'asset' },
      { title: 'Provider CLIs', category: 'provider-cli' },
    ];
    return (
      <div className="content-page">
        <div className="page-heading">
          <div>
            <span className="eyebrow">Optional capabilities</span>
            <h1>Integrations</h1>
            <p>Detect, test, install or skip. No capability below is required — the chat workspace works with only a model connection.</p>
          </div>
          <div className="heading-actions">
            <button type="button" className="outline-btn" onClick={() => void refreshAll()}>Re-detect</button>
          </div>
        </div>

        <div className="integration-summary">
          <div>
            <span className={mcpState.running ? 'status-dot on' : 'status-dot'} />
            MCP server: {mcpState.running ? 'connected' : 'not connected'} · {mcpState.tools.length} tools
          </div>
          <p>One active stdio server at a time (0.1.0 limit). Starting a server and every agent tool call wait for your approval.</p>
        </div>

        <div className="section-title">
          <span>MCP stdio server</span>
          <span>one server</span>
        </div>
        <div className="form-stack" style={{ maxWidth: 560 }}>
          <label>
            Command
            <input placeholder="npx" value={mcpForm.command} onChange={(event) => setMcpForm({ ...mcpForm, command: event.target.value })} />
          </label>
          <label>
            Arguments (space-separated)
            <input placeholder="mcp-server --port …" value={mcpForm.args} onChange={(event) => setMcpForm({ ...mcpForm, args: event.target.value })} />
          </label>
          <div style={{ display: 'flex', gap: 8 }}>
            <button type="button" className="primary-btn" disabled={mcpState.running} onClick={() => void connectMcp()}>Connect</button>
            {mcpState.running && <button type="button" className="outline-btn" onClick={() => void disconnectMcp()}>Stop server</button>}
          </div>
          {mcpState.tools.length > 0 && (
            <div className="provider-steps">
              <div className="steps-heading">Discovered tools</div>
              {mcpState.tools.slice(0, 12).map((tool) => (
                <div className="step-line" key={tool.name}>
                  <span>{ICONS.file}</span>
                  <p><code>{tool.name}</code> — {tool.description.slice(0, 120)}</p>
                </div>
              ))}
            </div>
          )}
        </div>

        {groups.map((group) => {
          const items = capabilities.filter((capability) => capability.category === group.category);
          if (items.length === 0) {
            return null;
          }
          return (
          <div key={group.title} style={{ marginTop: 30 }}>
            <div className="section-title">
              <span>{group.title}</span>
              <span>{items.length} listed</span>
            </div>
            <div className="integration-list">
              {items.map((capability) => (
                <div className={`integration-card ${expandedCards.has(capability.id) ? 'expanded' : ''}`} key={capability.id}>
                  <button type="button" className="integration-top" aria-expanded={expandedCards.has(capability.id)} onClick={() => setExpandedCards((prev) => { const next = new Set(prev); if (next.has(capability.id)) { next.delete(capability.id); } else { next.add(capability.id); } return next; })}>
                    <span className="integration-icon">{ICONS.spark}</span>
                    <span className="integration-copy">
                      <b>{capability.name}</b>
                      <small>{capability.detail}</small>
                    </span>
                    <span className={`state-pill ${capability.state}`}>{capability.state}</span>
                    <svg className={expandedCards.has(capability.id) ? 'rotate' : ''} style={{ color: 'var(--color-dim)' }} width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="m4 6 4 4 4-4" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" /></svg>
                  </button>
                  <div className="integration-detail">
                    {capability.installUrl ? <p>Official page: <a href={capability.installUrl} target="_blank" rel="noreferrer">{capability.installUrl}</a></p> : <p>No separate installer. Use the tool's own documentation.</p>}
                    <div className="integration-meta">
                      <span>Category: {capability.category}</span>
                      <span>Status: {capability.state}</span>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </div>
          );
        })}

        <div className="footnote">
          {ICONS.shield}
          <span>Open-Ro never auto-installs extensions. Setup guides, permissions and uninstall paths are documented in docs/EXTENSIONS.md.</span>
        </div>
      </div>
    );
  }

  function FileTree({ dir, depth }: { dir: string; depth: number }) {
    const entries = projectEntries.get(dir) ?? [];
    return (
      <ul className="file-tree" style={{ marginLeft: depth === 0 ? 0 : 16 }}>
        {entries.map((entry) => (
          <li key={entry.path}>
            {entry.type === 'dir' ? (
              <>
                <button type="button" className="tree-row" aria-expanded={expandedDirs.has(entry.path)} onClick={() => void toggleDir(entry.path)}>
                  {ICONS.folder}
                  <span>{entry.name}</span>
                </button>
                {expandedDirs.has(entry.path) && <FileTree dir={entry.path} depth={depth + 1} />}
              </>
            ) : (
              <button type="button" className="tree-row" onClick={() => void openFile(entry.path)}>
                {ICONS.file}
                <span>{entry.name}</span>
              </button>
            )}
          </li>
        ))}
      </ul>
    );
  }
}

function asMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  const detail = typeof error === 'string' ? error : JSON.stringify(error);
  try {
    const parsed = JSON.parse(detail) as { message?: string };
    return parsed.message ?? detail;
  } catch {
    return detail || 'Unexpected error.';
  }
}
