import { useEffect, useMemo, useRef, useState } from 'react';
import api from './api';
import type { ProviderKind, ProviderOption } from '../electron/providerCatalog';
import type { AgentTurnResult, ChatMessage, FileEntry, StoredProviderView } from '../electron/types';

type Screen = 'chat' | 'models' | 'workspace' | 'integrations';

interface ChatSession {
  id: string;
  title: string;
  messages: Array<{ role: 'user' | 'assistant'; content: string; trace?: AgentTurnResult['toolTrace']; error?: string }>;
}

type ProviderForm = { kind: ProviderKind | null; model: string; baseUrl: string; apiKey: string };

const SUGGESTIONS: Array<{ title: string; body: string; prompt: string }> = [
  { title: 'Explore this project', body: 'Map the folder structure and summarize what each script does.', prompt: 'List the files in my project and summarize the structure.' },
  { title: 'Add a feature', body: 'Plan a small change, then review the diff before writing it.', prompt: 'Propose a small improvement to this project and show me the diff before writing anything.' },
  { title: 'Explain a file', body: 'Pick a script and walk through what it does, step by step.', prompt: 'Pick one script from my project and explain what it does.' },
  { title: 'Set up a test command', body: 'Find a safe way to check that the game still builds.', prompt: 'Suggest a safe command to validate this project. Stage it for my approval; do not run it unasked.' },
];

const ICONS: Record<string, React.ReactNode> = {
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

function initials(name: string): string {
  const letters = name.split(/[\s-]/).filter(Boolean).slice(0, 2);
  return letters.map((part) => part[0]?.toUpperCase() ?? '').join('') || 'OP';
}

export default function App() {
  const [screen, setScreen] = useState<Screen>('chat');
  const [providerViews, setProviderViews] = useState<StoredProviderView[]>([]);
  const [catalog, setCatalog] = useState<ProviderOption[]>([]);
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
  const [capabilities, setCapabilities] = useState<Awaited<ReturnType<typeof api.detectCapabilities>>>([]);
  const [mcpState, setMcpState] = useState<{ running: boolean; tools: Awaited<ReturnType<typeof api.mcpStatus>>['tools'] }>({ running: false, tools: [] });
  const [mcpForm, setMcpForm] = useState({ command: '', args: '' });
  const [expandedCards, setExpandedCards] = useState<Set<string>>(new Set());
  const [providerModal, setProviderModal] = useState<ProviderForm>(resetForm());
  const [providerTest, setProviderTest] = useState<{ kind: ProviderKind; ok: boolean; detail: string } | null>(null);
  const [commandBar, setCommandBar] = useState<{ command: string; token: string; cwd: string; result?: { code: number | null; stdout: string; stderr: string }; failed?: string } | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  const activeSessionData = useMemo(() => sessions.find((session) => session.id === activeSession) ?? null, [sessions, activeSession]);
  const activeProvider = useMemo(() => providerViews.find((view) => view.active) ?? null, [providerViews]);
  const catalogByKind = useMemo(() => new Map(catalog.map((option) => [option.kind, option])), [catalog]);

  useEffect(() => {
    void refreshAll();
  }, []);
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [activeSessionData?.messages.length, sending]);

  async function refreshAll() {
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
      // Only submit the typed key when the user re-entered one; otherwise test the stored one.
      const result = await api.testProvider();
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
    const session: ChatSession = { id: `chat-${Date.now()}`, title: 'New chat', messages: [] };
    setSessions((prev) => [session, ...prev]);
    setActiveSession(session.id);
    setScreen('chat');
  }

  async function sendDraft() {
    const text = draft.trim();
    if (!text || sending) {
      return;
    }
    let sessionId = activeSession;
    if (!sessionId) {
      const fresh: ChatSession = { id: `chat-${Date.now()}`, title: text.slice(0, 40) || 'New chat', messages: [{ role: 'user', content: text }] };
      sessionId = fresh.id;
      setSessions((prev) => [fresh, ...prev]);
      setActiveSession(fresh.id);
    } else {
      setSessions((prev) =>
        prev.map((session) =>
          session.id === sessionId
            ? {
                ...session,
                title: session.messages.length === 0 ? text.slice(0, 40) : session.title,
                messages: [...session.messages, { role: 'user', content: text }],
              }
            : session,
        ),
      );
    }
    const history: ChatMessage[] = [...(sessions.find((session) => session.id === sessionId)?.messages ?? []), { role: 'user' as const, content: text }].map((message): ChatMessage => ({ role: message.role, content: message.content }));
    setDraft('');
    setSending(true);
    try {
      const result = await api.sendPrompt({ sessionId, messages: history, projectRoot: projectRoot ?? '' });
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
        // Changes are staged, never auto-applied: surface the approval drawer state in Workspace.
        void refreshWorkspaceState();
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
      setBanner(`Restored ${file}. Re-check the file tree to see the reverted content.`);
    } catch (error) {
      setBanner(asMessage(error));
    }
  }

  async function connectMcp() {
    if (!mcpForm.command.trim()) {
      return;
    }
    try {
      const args = mcpForm.args.trim() ? mcpForm.args.split(/\s+/) : [];
      const tools = await api.mcpConnect({ command: mcpForm.command.trim(), args });
      setMcpState({ running: true, tools });
      setBanner(`MCP server connected. Discovered ${tools.length} tools; single active server limit applies.`);
    } catch (error) {
      setBanner(asMessage(error));
    }
  }

  async function disconnectMcp() {
    await api.mcpDisconnect();
    setMcpState({ running: false, tools: [] });
  }

  const hasProvider = providerViews.some((view) => view.hasKey || catalogByKind.get(view.kind)?.keyless) || providerViews.length > 0;

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand-row">
          <div className="brand-mark">OR</div>
          <div className="brand-lockup">
            <strong>Open-Ro</strong>
            <span>STUDIO 0.1.0</span>
          </div>
          <button className="icon-btn small more" title="Settings and docs" onClick={() => window.open('https://github.com/cayleb-james2008/open-ro', '_self')}>
            {ICONS.chevron}
          </button>
        </div>
        <button className={`project-switch ${projectRoot ? 'has-project' : ''}`} onClick={() => void selectProject()}>
          {ICONS.folder}
          <span>{projectRoot ? projectRoot.split(/[\\/]/).pop() : 'Choose project folder…'}</span>
        </button>
        <div className="nav-label">WORKSPACE</div>
        <button className="new-chat-btn" onClick={newChat}>
          {ICONS.plus}
          <span>New chat</span>
        </button>
        <div className="recent-label nav-label">RECENT</div>
        <div className="recent-chat-list">
          {sessions.map((session) => (
            <button key={session.id} className={`recent-chat ${session.id === activeSession ? 'active' : ''}`} onClick={() => { setActiveSession(session.id); setScreen('chat'); }}>
              {ICONS.chat}
              <span>{session.title}</span>
            </button>
          ))}
          {sessions.length === 0 && <div className="recent-chat" style={{ opacity: 0.5 }}>Session history stays this session</div>}
        </div>
        <div className="side-spacer" />
        <div className="connection-card">
          <div className="connection-head">
            <span className={`status-dot ${activeProvider ? 'on' : ''}`} />
            MODEL CONNECTION
          </div>
          <div className="connection-model">{activeProvider ? `${catalogByKind.get(activeProvider.kind)?.name ?? activeProvider.kind}${activeProvider.model ? ` · ${activeProvider.model}` : ''}` : 'No model connected'}</div>
          <button className="connection-action" onClick={() => setScreen('models')}>
            {activeProvider ? 'Manage providers' : 'Connect a model'}
            <span>→</span>
          </button>
        </div>
        <div className="sidebar-footer">
          {ICONS.shield}
          <span>Local-first · credentials stay in OS secure storage</span>
        </div>
      </aside>

      <main className="main-area">
        <div className="topbar">
          <div className="crumb">
            <strong>Open-Ro</strong>
            <span className="slash">/</span>
            <span>{screen === 'chat' ? (activeSessionData?.title ?? 'Chat') : screen === 'models' ? 'Models & providers' : screen === 'workspace' ? 'Workspace' : 'Integrations'}</span>
          </div>
          <div className="top-actions">
            <span className="project-pill" title={projectRoot ?? undefined}>{projectRoot ?? 'No project'}</span>
            <span className={`runtime-pill ${sending ? 'working' : ''}`}>{sending ? 'Agent working…' : 'Idle'}</span>
          </div>
        </div>

        {banner && (
          <div className="error-banner">
            <span>{banner}</span>
            <button onClick={() => setBanner(null)} aria-label="Dismiss">{ICONS.x}</button>
          </div>
        )}

        {screen === 'chat' && <ChatPage />}
        {screen === 'models' && <ModelsPage />}
        {screen === 'workspace' && <WorkspacePage />}
        {screen === 'integrations' && <IntegrationsPage />}
      </main>

      {providerModal.kind && <ProviderModal />}
      {fileView && (
        <div className="modal-backdrop" onClick={() => setFileView(null)}>
          <div className="file-modal" onClick={(event) => event.stopPropagation()}>
            <div className="modal-title">
              <div>
                <strong>{fileView.rel}</strong>
                <small>{fileView.content.length} characters · read only in 0.1.0</small>
              </div>
              <button className="icon-btn" onClick={() => setFileView(null)}>{ICONS.x}</button>
            </div>
            <pre>{fileView.content}</pre>
          </div>
        </div>
      )}
    </div>
  );

  // ---- Chat ------------------------------------------------------------
  function ChatPage() {
    return (
      <div className="chat-page">
        {!activeSessionData || activeSessionData.messages.length === 0 ? (
          <div className="chat-scroll" ref={scrollRef}>
            <div className="welcome-wrap">
              <span className="welcome-chip">LOCAL-FIRST ROBLOX WORKSPACE</span>
              <h1>
                What are we <span>building<span>?</span></span>
              </h1>
              <p className="welcome-sub">
                Bring your own model — OpenAI, Anthropic, OpenRouter, OpenCode, Ollama or any OpenAI-compatible endpoint. Pick a project folder to give the agent file tools. Nothing leaves your machine except the model requests you send.
              </p>
              <div className="setup-row">
                <button className={`setup-tile ${activeProvider ? 'complete' : ''}`} onClick={() => setScreen('models')}>
                  <span className="tile-icon">{ICONS.check}</span>
                  <span>
                    <b>Connect a model</b>
                    <small>{activeProvider ? catalogByKind.get(activeProvider.kind)?.name : 'API key or official CLI sign-in'}</small>
                  </span>
                  {activeProvider && <span className="tile-check">{ICONS.check}</span>}
                </button>
                <button className="setup-tile" onClick={() => setScreen('workspace')}>
                  <span className="tile-icon">{ICONS.folder}</span>
                  <span>
                    <b>Select project</b>
                    <small>{projectRoot ? projectRoot : 'Optional — enables file tools'}</small>
                  </span>
                </button>
              </div>
              <div className="suggestion-grid">
                {SUGGESTIONS.map((item) => (
                  <button key={item.title} onClick={() => setDraft(item.prompt)}>
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
                <span>Every file write and command needs your approval — nothing runs unchecked.</span>
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
        {!storage?.secure && (
          <div className="composer-notice">
            <span>⚠ Keyring not detected — API keys cannot be saved securely. Install a desktop secret service and restart to persist keys.</span>
          </div>
        )}
        <div className="composer">
          <textarea
            placeholder="Describe a change, ask a question, or propose an experiment…"
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
              <button className="send-btn" disabled={!draft.trim() || sending} onClick={() => void sendDraft()} aria-label="Send">
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
            <span className="eyebrow">MODEL CONNECTIONS</span>
            <h1>Models & providers</h1>
            <p>One connection is all you need. API keys are stored with Electron safeStorage; subscription routes run the official CLI and never read its credentials.</p>
          </div>
          <div className="heading-actions">
            <span className="outline-btn" style={{ cursor: 'default' }}>{storage?.secure ? 'Secure storage: OK' : 'No desktop keyring'}</span>
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
          <span>CONNECTED PROVIDERS</span>
          <span>{providerViews.length} saved</span>
        </div>
        <div className="provider-list">
          {providerViews.map((view) => {
            const option = catalogByKind.get(view.kind);
            return (
              <div className={`provider-row ${view.active ? 'selected' : ''}`} key={view.kind}>
                <span className={`provider-logo ${view.kind}`}>{initials(option?.name ?? view.kind)}</span>
                <span className="provider-info">
                  <b>{option?.name ?? view.kind}</b>
                  <small>{view.hasKey ? 'API key stored securely' : 'keyless / save pending'}{view.model ? ` · ${view.model}` : ''}</small>
                </span>
                {view.active && <span className="active-tag">ACTIVE</span>}
                <button className="outline-btn" onClick={() => openProviderModal(view.kind)}>Edit</button>
                {!view.active && <button className="outline-btn" onClick={async () => { await api.setActive(view.kind); await refreshAll(); }}>Use</button>}
                <button className="icon-btn small danger" title="Remove stored key" onClick={() => void removeProvider(view.kind)}>{ICONS.x}</button>
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
          <span>AVAILABLE ROUTES</span>
          <span>official setup links</span>
        </div>
        <div className="provider-catalog">
          {catalog.map((option) => (
            <div className="provider-choice" key={option.kind}>
              <span className={`provider-logo ${option.kind}`}>{initials(option.name)}</span>
              <span className="choice-copy">
                <b>{option.name}</b>
                <small>{option.setup}</small>
                <span className="choice-cost">{option.costNote}</span>
              </span>
              <button className="outline-btn" onClick={() => openProviderModal(option.kind)}>Connect</button>
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
      <div className="modal-backdrop" onClick={() => setProviderModal(resetForm())}>
        <div className="provider-modal" onClick={(event) => event.stopPropagation()}>
          <div className="modal-title">
            <div>
              <strong>{option?.name}</strong>
              <small>{option?.setup}</small>
            </div>
            <button className="icon-btn" onClick={() => setProviderModal(resetForm())}>{ICONS.x}</button>
          </div>
          <div className="provider-steps">
            <div className="steps-heading">SETUP STEPS</div>
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
              Model ID {option?.modelHint ? <em style={{ color: '#84909e' }}>({option.modelHint})</em> : null}
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
            <button className="text-btn" onClick={() => void testProvider()}>Check connection</button>
            <button className="outline-btn" onClick={() => setProviderModal(resetForm())}>Cancel</button>
            <button className="primary-btn" onClick={() => void saveProvider()}>Save & set active</button>
          </div>
        </div>
      </div>
    );
  }

  // ---- Workspace ------------------------------------------------------------
  function WorkspacePage() {
    return (
      <div className="content-page">
        <div className="page-heading">
          <div>
            <span className="eyebrow">LOCAL PROJECT</span>
            <h1>Workspace</h1>
            <p>File browsing is read-only in 0.1.0; agent write proposals are reviewed before anything lands, and every accepted write leaves a checkpoint.</p>
          </div>
          <div className="heading-actions">
            <button className="outline-btn" onClick={() => void refreshWorkspaceState()}>Refresh</button>
            <button className="primary-btn" onClick={() => void selectProject()}>Choose folder…</button>
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
          <span>FILES</span>
          <span>{projectEntries.get('.')?.length ?? 0} top-level entries</span>
        </div>
        {projectRoot && projectEntries.get('.') && (
          <div className="integration-list">
            {[...projectEntries.entries()].map(([dir, entries]) => (
              <div key={dir} style={{ marginLeft: dir === '.' ? 0 : 14 }}>
                <div className="section-title" style={{ marginBottom: 6 }}>
                  <span style={{ letterSpacing: 0, color: '#8a95a4', fontFamily: 'Inter, sans-serif', fontSize: 10 }}>{dir}/{(expandedDirs.has(dir) || dir === '.') && dir !== '.' ? '' : ''}</span>
                  <span onClick={() => toggleDir(dir)} style={{ cursor: 'pointer' }}>{expandedDirs.has(dir) || dir === '.' ? '' : ''}</span>
                </div>
                {entries.map((entry) => (
                  <div className="integration-card" key={entry.path} style={{ maxWidth: 560 }}>
                    {entry.type === 'dir' ? (
                      <button className="integration-top" onClick={() => void toggleDir(entry.path)}>
                        <span className="integration-icon">{ICONS.folder}</span>
                        <span className="integration-copy">
                          <b>{entry.name}/</b>
                          <small>Directory — click to expand</small>
                        </span>
                        <span className="state-pill">{projectEntries.has(entry.path) ? `${projectEntries.get(entry.path)!.length} entries` : 'open'}</span>
                        <svg className="rotate" style={{ color: '#7e8997', transform: expandedDirs.has(entry.path) ? 'rotate(180deg)' : 'none' }} width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="m4 6 4 4 4-4" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" /></svg>
                      </button>
                    ) : (
                      <button className="integration-top" onClick={() => void openFile(entry.path)}>
                        <span className="integration-icon">{ICONS.file}</span>
                        <span className="integration-copy">
                          <b>{entry.name}</b>
                          <small>Click to preview contents</small>
                        </span>
                        <span className="state-pill">{ICONS.file}</span>
                      </button>
                    )}
                  </div>
                ))}
              </div>
            ))}
            {projectEntries.get('.')!.length === 0 && (
              <div className="empty-state">
                <span className="empty-icon">{ICONS.folder}</span>
                <b>Nothing here</b>
                <p>Hidden files and build folders (node_modules, dist, .git) are ignored.</p>
              </div>
            )}
          </div>
        )}

        <div className="section-title" style={{ marginTop: 28 }}>
          <span>CHECKPOINTS</span>
          <span>restore on click</span>
        </div>
        <div className="integration-list">
          {checkpoints.map((file) => (
            <div className="integration-card" key={file}>
              <button className="integration-top" onClick={() => void restoreCheckpoint(file)}>
                <span className="integration-icon">{ICONS.shield}</span>
                <span className="integration-copy">
                  <b>{file}</b>
                  <small>Recorded before an accepted write</small>
                </span>
                <span className="state-pill">restore</span>
              </button>
            </div>
          ))}
          {checkpoints.length === 0 && (
            <div className="empty-state">
              <span className="empty-icon">{ICONS.shield}</span>
              <b>No checkpoints yet</b>
              <p>Each approved agent write records the previous file state here first.</p>
            </div>
          )}
        </div>
      </div>
    );
  }

  // ---- Integrations ------------------------------------------------------------
  function IntegrationsPage() {
    const groups: Array<{ title: string; category: string }> = [
      { title: 'PROVIDER CLIs', category: 'provider-cli' },
      { title: 'ROBLOX STUDIO', category: 'studio' },
      { title: 'ASSET & PROJECT TOOLS', category: 'asset' },
    ];
    return (
      <div className="content-page">
        <div className="page-heading">
          <div>
            <span className="eyebrow">OPTIONAL CAPABILITIES</span>
            <h1>Integrations</h1>
            <p>Detect, test, install or skip. No capability below is required — the chat workspace works with only a model connection.</p>
          </div>
          <div className="heading-actions">
            <button className="outline-btn" onClick={() => void refreshAll()}>Re-detect</button>
          </div>
        </div>

        <div className="integration-summary">
          <div>
            <span className={mcpState.running ? 'status-dot on' : 'status-dot'} />
            MCP server: {mcpState.running ? 'connected' : 'not connected'} · {mcpState.tools.length} tools
          </div>
          <p>One active stdio server at a time (0.1.0 limit). Multi-server lifecycle and scoped approvals are roadmap work.</p>
        </div>

        <div className="section-title">
          <span>MCP STDIO SERVER</span>
          <span>approval-gated</span>
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
            <button className="primary-btn" disabled={mcpState.running} onClick={() => void connectMcp()}>Connect</button>
            {mcpState.running && <button className="outline-btn" onClick={() => void disconnectMcp()}>Stop server</button>}
          </div>
          {mcpState.tools.length > 0 && (
            <div className="provider-steps">
              <div className="steps-heading">DISCOVERED TOOLS</div>
              {mcpState.tools.slice(0, 12).map((tool) => (
                <div className="step-line" key={tool.name}>
                  <span>{ICONS.file}</span>
                  <p><code>{tool.name}</code> — {tool.description.slice(0, 120)}</p>
                </div>
              ))}
            </div>
          )}
        </div>

        {groups.map((group) => (
          <div key={group.title} style={{ marginTop: 30 }}>
            <div className="section-title">
              <span>{group.title}</span>
              <span>local detection</span>
            </div>
            <div className="integration-list">
              {capabilities.filter((capability) => capability.category === group.category).map((capability) => (
                <div className={`integration-card ${expandedCards.has(capability.id) ? 'expanded' : ''}`} key={capability.id}>
                  <button className="integration-top" onClick={() => setExpandedCards((prev) => { const next = new Set(prev); if (next.has(capability.id)) { next.delete(capability.id); } else { next.add(capability.id); } return next; })}>
                    <span className="integration-icon">{ICONS.spark}</span>
                    <span className="integration-copy">
                      <b>{capability.name}</b>
                      <small>{capability.detail}</small>
                    </span>
                    <span className={`state-pill ${capability.state}`}>{capability.state}</span>
                    <svg className={expandedCards.has(capability.id) ? 'rotate' : ''} style={{ color: '#7e8997' }} width="13" height="13" viewBox="0 0 16 16" fill="none"><path d="m4 6 4 4 4-4" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" /></svg>
                  </button>
                  <div className="integration-detail">
                    <p>Official install page: <a style={{ color: 'var(--accent)' }} href={capability.installUrl ?? '#'} target="_blank" rel="noreferrer">{capability.installUrl}</a>.</p>
                    <div className="integration-meta">
                      <span>CATEGORY · {capability.category}</span>
                      <span>STATUS · {capability.state.toUpperCase()}</span>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </div>
        ))}

        <div className="footnote">
          {ICONS.shield}
          <span>Open-Ro never auto-installs extensions. Setup guides, permissions and uninstall paths are documented in docs/EXTENSIONS.md.</span>
        </div>
      </div>
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
