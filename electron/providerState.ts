import { getProviderOption, isProviderKind, type ProviderKind, type ProviderOption } from './providerCatalog';
import type { StoredProviderView } from './types';

export interface ProviderRecord {
  kind: ProviderKind;
  model: string;
  baseUrl?: string;
}

export interface ProviderAppState {
  activeKind?: ProviderKind;
  providers: ProviderRecord[];
  /** Absolute folder the user selected. Absent means no project, never the process cwd. */
  projectRoot?: string;
}

export interface ProviderSaveInput {
  model: string;
  baseUrl?: string;
  apiKey?: string;
  existingKey: boolean;
}

export function parseProviderState(raw: unknown): ProviderAppState {
  if (!raw || typeof raw !== 'object') {
    return { providers: [] };
  }
  const value = raw as Record<string, unknown>;
  const providers: ProviderRecord[] = [];
  const push = (kind: string, model: unknown, baseUrl: unknown) => {
    if (!isProviderKind(kind) || providers.some((item) => item.kind === kind)) {
      return;
    }
    providers.push({
      kind,
      model: typeof model === 'string' ? model : '',
      baseUrl: typeof baseUrl === 'string' && baseUrl.trim() ? baseUrl : undefined,
    });
  };
  if (Array.isArray(value.providers)) {
    for (const entry of value.providers) {
      if (!entry || typeof entry !== 'object') {
        continue;
      }
      const record = entry as { kind?: unknown; model?: unknown; baseUrl?: unknown };
      if (typeof record.kind === 'string') {
        push(record.kind, record.model, record.baseUrl);
      }
    }
  }
  let activeKind = typeof value.activeKind === 'string' && isProviderKind(value.activeKind) ? value.activeKind : undefined;
  const legacy = value.active;
  if (legacy && typeof legacy === 'object') {
    const active = legacy as { kind?: unknown; model?: unknown; baseUrl?: unknown };
    if (typeof active.kind === 'string') {
      push(active.kind, active.model, active.baseUrl);
      if (!activeKind && isProviderKind(active.kind)) {
        activeKind = active.kind;
      }
    }
  }
  if (activeKind && !providers.some((item) => item.kind === activeKind)) {
    activeKind = undefined;
  }
  const projectRoot = storedProjectRoot(value.projectRoot);
  return projectRoot ? { activeKind, providers, projectRoot } : { activeKind, providers };
}

function storedProjectRoot(value: unknown): string | undefined {
  if (typeof value !== 'string') {
    return undefined;
  }
  const trimmed = value.trim();
  if (!trimmed || trimmed === '.') {
    return undefined;
  }
  return trimmed;
}

export function rememberProjectRoot(state: ProviderAppState, root: string): ProviderAppState {
  const projectRoot = storedProjectRoot(root);
  if (!projectRoot) {
    const { projectRoot: _ignored, ...rest } = state;
    return rest;
  }
  return { ...state, projectRoot };
}

export function upsertProvider(state: ProviderAppState, record: ProviderRecord): ProviderAppState {
  const providers = state.providers.filter((item) => item.kind !== record.kind);
  providers.push({ kind: record.kind, model: record.model, baseUrl: record.baseUrl });
  return { providers, activeKind: record.kind, projectRoot: state.projectRoot };
}

export function activateProvider(state: ProviderAppState, kind: ProviderKind): ProviderAppState {
  if (!state.providers.some((item) => item.kind === kind)) {
    throw new Error('Save this provider before setting it active.');
  }
  return { ...state, activeKind: kind };
}

export function removeProvider(state: ProviderAppState, kind: ProviderKind): ProviderAppState {
  return {
    providers: state.providers.filter((item) => item.kind !== kind),
    activeKind: state.activeKind === kind ? undefined : state.activeKind,
    projectRoot: state.projectRoot,
  };
}

export function validateProviderSave(option: ProviderOption, input: ProviderSaveInput): { model: string; baseUrl?: string; apiKey?: string } {
  const model = input.model.trim();
  const baseUrl = input.baseUrl?.trim() || undefined;
  const apiKey = input.apiKey?.trim() || undefined;
  if (option.needsBaseUrl && !baseUrl) {
    throw new Error('This provider requires a base URL.');
  }
  const keyRequired = option.connection === 'api-key' && !option.keyless && !option.keyOptional;
  if (keyRequired && !apiKey && !input.existingKey) {
    throw new Error('An API key is required for this provider.');
  }
  if (option.connection !== 'cli' && !model) {
    throw new Error('Enter the model ID this connection should use.');
  }
  return { model, baseUrl, apiKey };
}

export function listProviderViews(
  state: ProviderAppState,
  options: { credentialServices: string[]; secure: boolean },
): StoredProviderView[] {
  const kinds = new Set<ProviderKind>(state.providers.map((item) => item.kind));
  for (const service of options.credentialServices) {
    if (!service.startsWith('provider:')) {
      continue;
    }
    const kind = service.slice('provider:'.length);
    if (isProviderKind(kind)) {
      kinds.add(kind);
    }
  }
  return [...kinds].map((kind) => {
    const record = state.providers.find((item) => item.kind === kind);
    let keyless = false;
    try {
      keyless = getProviderOption(kind).keyless === true;
    } catch {
      keyless = false;
    }
    return {
      kind,
      model: record?.model ?? '',
      baseUrl: record?.baseUrl,
      hasKey: keyless || options.credentialServices.includes(`provider:${kind}`),
      active: state.activeKind === kind,
      secure: options.secure,
    };
  });
}
