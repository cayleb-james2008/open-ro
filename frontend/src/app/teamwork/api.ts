import { call, enc } from "../ship/api";

export type RunStatus = "running" | "completed" | "stopped" | "failed";
export type AgentWorkState = "working" | "waiting" | "stopped";
export type TaskStatus = "pending" | "in_progress" | "awaiting_verification" | "changes_requested" | "done" | "failed" | "cancelled";

export type QuotaState = {
  active: boolean;
  reset_in?: string | null;
  resume_at_ms?: number | null;
  message?: string | null;
  retries?: number;
};

export type StudioLease = {
  holder_agent: string;
  task_id?: string | null;
  operation: "sync" | "playtest";
  acquired_at_ms: number;
};

export type RunSummary = {
  id: string;
  goal: string;
  project_id?: string | null;
  status: RunStatus;
  model_id: string;
  provider?: "antigravity" | "openai";
  max_concurrency: number;
  active_model_calls?: number;
  board_seq?: number;
  studio_lease?: StudioLease | null;
  quota?: QuotaState;
  created_at_ms?: number;
  updated_at_ms?: number;
};

export type RunDetail = RunSummary & {
  project_dir?: string;
  storage_dir?: string;
};

export type AgentState = {
  name: string;
  role: string;
  state: AgentWorkState;
  killed?: boolean;
  chat_id?: string;
  model_id?: string;
  current_task_id?: string | null;
  cursor_seq?: number;
  turns: number;
  trace_count?: number;
  last_post_seq?: number | null;
  last_error?: string | null;
  updated_at_ms?: number;
};

export type VerifyReport = {
  passed: boolean;
  verifier: string;
  playtest_passed?: boolean | null;
  console_errors?: number;
  notes?: string;
  at_ms?: number;
};

export type Task = {
  id: string;
  title: string;
  description?: string;
  kind?: string;
  owner: string;
  verifier?: string;
  status: TaskStatus;
  claimed_files?: string[];
  depends_on?: string[];
  result_summary?: string | null;
  verify_report?: VerifyReport | null;
  commit_sha?: string | null;
  created_at_ms?: number;
  updated_at_ms?: number;
};

export type FileClaim = { path: string; task_id: string; owner: string; claimed_at_ms?: number };

export type BoardPost = {
  seq: number;
  ts_ms?: number;
  time?: string;
  author: string;
  kind: string;
  text: string;
  mentions?: string[];
  is_done?: boolean;
  task_id?: string | null;
};

export type TraceEntry = { seq: number; ts_ms?: number; turn?: number; kind: string; summary: string; data?: unknown };
export type ChatMessage = { role: string; content: string };

export type TeamworkSnapshot = {
  run: RunDetail;
  roster: AgentState[];
  tasks: Task[];
  claims: FileClaim[];
  board: BoardPost[];
};

export type ProjectContractSummary = {
  has_contract: boolean;
  active?: boolean;
  total: number;
  passed_count: number;
  failing_ids?: string[];
};

export const teamUrl = (id: string, tail = "") => `/api/teamwork/${enc(id)}${tail}`;
export const getProjectContract = (projectId: string) =>
  call<ProjectContractSummary>(`/api/projects/${enc(projectId)}/contract`);
export const listRuns = (projectId?: string | null) =>
  call<{ runs: RunSummary[] }>(projectId ? `/api/teamwork?project_id=${enc(projectId)}` : "/api/teamwork");
export const getRun = (id: string) => call<TeamworkSnapshot>(teamUrl(id));
export const createRun = (body: { goal: string; project_id?: string; roles?: string[]; max_concurrency?: number; model_id?: string }) =>
  call<{ run: RunSummary }>("/api/teamwork", body);
export const postToBoard = (id: string, text: string) => call<{ message: BoardPost; woken: string[] }>(teamUrl(id, "/post"), { text, author: "human" });
export const stopRun = (id: string) => call<{ run: RunSummary }>(teamUrl(id, "/stop"), {});
export const resumeRun = (id: string) => call<{ run: RunSummary; woken: string[] }>(teamUrl(id, "/resume"), {});
export const killAgent = (id: string, name: string) => call<{ agent: AgentState }>(teamUrl(id, `/agents/${enc(name)}/kill`), {});
export const restartAgent = (id: string, name: string, reset = false) =>
  call<{ agent: AgentState }>(teamUrl(id, `/agents/${enc(name)}/restart`), { reset });
export const getAgentTrace = (id: string, name: string) =>
  call<{ agent: AgentState; trace: TraceEntry[]; messages: ChatMessage[] }>(teamUrl(id, `/agents/${enc(name)}/trace`));
