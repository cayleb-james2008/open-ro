/**
 * Provider definitions. Identity (vendor/endpoint), connection method and model
 * metadata stay separate fields so routing policy can evolve without changing
 * the catalog contract (docs/ARCHITECTURE.md §3).
 */
export type ProviderKind =
  | 'openai-api'
  | 'codex-cli'
  | 'anthropic-api'
  | 'claude-code-cli'
  | 'openrouter-api'
  | 'opencode-cli'
  | 'ollama'
  | 'custom-openai';

export interface ProviderOption {
  kind: ProviderKind;
  /** Connection method family. */
  connection: 'api-key' | 'cli' | 'local';
  /** Human display name. */
  name: string;
  /** One-tile setup description shown in the Models screen. */
  setup: string;
  /** Official setup / signup URL (docs/AUTH.md routes). */
  url: string;
  /** Cost boundary note, shown before connecting. */
  costNote: string;
  /** Placeholder for the model field, when the user must type an exact model ID. */
  modelHint: string;
  /** API kinds talk /v1/chat/completions (or /v1/messages for Anthropic). */
  api?: 'openai-chat' | 'anthropic-messages';
  /** Default endpoint; overridden by the user for custom-openai / ollama. */
  defaultBaseUrl?: string;
  /** True when the user must supply a base URL. */
  needsBaseUrl?: boolean;
  /** True when no credential is stored at all (local runtime). */
  keyless?: boolean;
  /** True when an API key is accepted but not required (custom endpoints). */
  keyOptional?: boolean;
  /** CLI executable handed the official sign-in flow. */
  cliCommand?: string;
}

export const providerOptions: ProviderOption[] = [
  {
    kind: 'openai-api',
    connection: 'api-key',
    name: 'OpenAI API key',
    setup: 'Create an API key at platform.openai.com and paste it here.',
    url: 'https://platform.openai.com/api-keys',
    costNote: 'Pay-as-you-go, billed by OpenAI Platform. Separate from any ChatGPT subscription.',
    modelHint: 'e.g. gpt-5.2',
    api: 'openai-chat',
    defaultBaseUrl: 'https://api.openai.com/v1',
  },
  {
    kind: 'codex-cli',
    connection: 'cli',
    name: 'ChatGPT subscription (Codex CLI)',
    setup: 'Install Codex CLI, run `codex login` in a terminal, then check the connection here.',
    url: 'https://chatgpt.com/codex',
    costNote: 'Uses your ChatGPT subscription; sign-in and limits are handled by Codex itself.',
    modelHint: 'leave empty to use the CLI default model',
    cliCommand: 'codex',
  },
  {
    kind: 'anthropic-api',
    connection: 'api-key',
    name: 'Anthropic API key',
    setup: 'Create a key in the Anthropic Console and paste it here.',
    url: 'https://console.anthropic.com/settings/keys',
    costNote: 'Pay-as-you-go, billed through the Anthropic Console.',
    modelHint: 'e.g. claude-sonnet-4-6',
    api: 'anthropic-messages',
    defaultBaseUrl: 'https://api.anthropic.com',
  },
  {
    kind: 'claude-code-cli',
    connection: 'cli',
    name: 'Claude subscription (Claude Code CLI)',
    setup: 'Install Claude Code, complete sign-in inside Claude Code, then check the connection.',
    url: 'https://code.claude.com/docs/en/setup',
    costNote: 'Uses your Claude subscription entirely inside the official Claude Code CLI.',
    modelHint: 'leave empty to use the CLI default model',
    cliCommand: 'claude',
  },
  {
    kind: 'openrouter-api',
    connection: 'api-key',
    name: 'OpenRouter',
    setup: 'Create a key at openrouter.ai/keys and paste it, then enter an exact model ID.',
    url: 'https://openrouter.ai/keys',
    costNote: 'Billing and provider fallbacks are managed by OpenRouter.',
    modelHint: 'e.g. openai/gpt-5.2 or anthropic/claude-sonnet-4-6',
    api: 'openai-chat',
    defaultBaseUrl: 'https://openrouter.ai/api/v1',
  },
  {
    kind: 'opencode-cli',
    connection: 'cli',
    name: 'OpenCode CLI',
    setup: 'Install OpenCode, run `opencode auth login`, then check the connection here.',
    url: 'https://opencode.ai/download',
    costNote: 'Provider account and plan are handled by OpenCode and its provider.',
    modelHint: 'leave empty to use the CLI default model',
    cliCommand: 'opencode',
  },
  {
    kind: 'ollama',
    connection: 'local',
    name: 'Ollama (local)',
    setup: 'Install Ollama, pull a model, e.g. `ollama pull qwen2.5-coder`.',
    url: 'https://ollama.com/download',
    costNote: 'Runs on your machine; no cloud key required and no data leaves your device.',
    modelHint: 'e.g. qwen2.5-coder',
    api: 'openai-chat',
    defaultBaseUrl: 'http://127.0.0.1:11434/v1',
    keyless: true,
  },
  {
    kind: 'custom-openai',
    connection: 'api-key',
    name: 'Custom OpenAI-compatible endpoint',
    setup: 'Point Open-Ro at any service exposing /v1/chat/completions.',
    url: 'https://openrouter.ai/models',
    costNote: 'Depends entirely on the service you select; check its own terms.',
    modelHint: 'exact model ID served by the endpoint',
    api: 'openai-chat',
    needsBaseUrl: true,
    keyOptional: true,
  },
];

const PROVIDER_KINDS = new Set<ProviderKind>(providerOptions.map((option) => option.kind));

export function isProviderKind(value: string): value is ProviderKind {
  return PROVIDER_KINDS.has(value as ProviderKind);
}

export function getProviderOption(kind: ProviderKind): ProviderOption {
  const found = providerOptions.find((option) => option.kind === kind);
  if (!found) {
    throw new Error(`Unknown provider kind: ${String(kind)}`);
  }
  return found;
}
