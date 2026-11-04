/**
 * Project acceptance definitions. Pure module: the main process reads
 * `.openro/acceptance.json` and passes its text here.
 */

export type AcceptanceCheckType = 'assertion' | 'state' | 'file';

/** The subset of a playtest report needed to check the declared assertion/state sets. */
export interface AcceptanceReportCheck {
  description: string;
  expected: string;
}

export interface AcceptanceReportStateCheck {
  instancePath: string;
  property: string;
  expected: string;
}

export interface AcceptanceReport {
  assertions: AcceptanceReportCheck[];
  stateChecks: AcceptanceReportStateCheck[];
}

export interface AcceptanceAssertion {
  id: string;
  scenario: string;
  type: 'assertion';
  description: string;
  expected: string;
}

export interface AcceptanceStateCheck {
  id: string;
  scenario: string;
  type: 'state';
  instancePath: string;
  property: string;
  expected: string;
}

export interface AcceptanceFileCheck {
  id: string;
  scenario: string;
  type: 'file';
  path: string;
  contains?: string;
  expected?: string;
}

export type AcceptanceCheck = AcceptanceAssertion | AcceptanceStateCheck | AcceptanceFileCheck;

export interface AcceptanceDefinition {
  version: 1;
  checks: AcceptanceCheck[];
  screenshots: string[];
  /** Optional project-authored hint included only in the repair prompt. */
  repairHint?: string;
}

export interface ScenarioAcceptance {
  assertions: AcceptanceAssertion[];
  stateChecks: AcceptanceStateCheck[];
  fileChecks: AcceptanceFileCheck[];
  screenshots: string[];
}

/** The ledger stores a 200-character scenario key; definitions use the same key. */
export const ACCEPTANCE_SCENARIO_MAX = 200;

const MAX_CHECKS = 50;
const MAX_SCREENSHOTS = 10;
const MAX_TEXT = 500;
const MAX_FILE_BYTES = 256_000;
const MAX_TOTAL_TEXT = 12_000;
const MAX_RENDERED_TEXT = 15_000;

/** Mirrors pathSafety.ts; duplicated so this module stays renderer-safe. */
const IGNORED_SEGMENTS = new Set(['.git', 'node_modules', 'dist', 'dist-electron', 'release', 'coverage']);
const SECRET_SEGMENT = /^(?:\.env(?:\..*)?|.*\.(?:pem|key|p12|pfx)|id_rsa|id_ed25519|credentials\.json)$/i;

function requiredText(value: unknown, field: string, max = MAX_TEXT): string {
  if (typeof value !== 'string' || !value.trim()) {
    throw new Error(`Acceptance ${field} must be a non-empty string.`);
  }
  const text = value.trim();
  if (text.length > max) {
    throw new Error(`Acceptance ${field} is longer than ${max} characters.`);
  }
  return text;
}

function safeRelativePath(value: unknown, field: string): string {
  const path = requiredText(value, field, 300).replace(/\\/g, '/');
  if (path.startsWith('/') || /^[A-Za-z]:\//.test(path) || path.split('/').includes('..')) {
    throw new Error(`Acceptance ${field} must be a relative path inside the project.`);
  }
  for (const part of path.split('/')) {
    if (IGNORED_SEGMENTS.has(part)) {
      throw new Error(`Acceptance ${field} may not use the ignored folder ${part}.`);
    }
    if (SECRET_SEGMENT.test(part)) {
      throw new Error(`Acceptance ${field} may not point at a secret file.`);
    }
  }
  return path;
}

function optionalText(value: unknown, field: string, max = MAX_TEXT): string | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (typeof value !== 'string' || value.length > max) {
    throw new Error(`Acceptance ${field} must be a string of at most ${max} characters.`);
  }
  return value;
}

export function parseAcceptanceDefinition(raw: string): AcceptanceDefinition {
  if (raw.length > MAX_FILE_BYTES) {
    throw new Error('Acceptance file is larger than 256 KB.');
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw.replace(/^\uFEFF/, ''));
  } catch {
    throw new Error('Acceptance file is not valid JSON.');
  }
  if (!parsed || typeof parsed !== 'object') {
    throw new Error('Acceptance file must contain an object.');
  }
  const record = parsed as Record<string, unknown>;
  if (record.version !== 1) {
    throw new Error('Acceptance version must be 1.');
  }
  if (!Array.isArray(record.checks) || record.checks.length === 0 || record.checks.length > MAX_CHECKS) {
    throw new Error(`Acceptance checks must be a list of 1-${MAX_CHECKS}.`);
  }
  const screenshots = record.screenshots === undefined ? [] : record.screenshots;
  if (!Array.isArray(screenshots) || screenshots.length > MAX_SCREENSHOTS) {
    throw new Error(`Acceptance screenshots must be a list of at most ${MAX_SCREENSHOTS}.`);
  }
  const ids = new Set<string>();
  const checks = record.checks.map((value, index): AcceptanceCheck => {
    if (!value || typeof value !== 'object') {
      throw new Error(`Acceptance check ${index + 1} must be an object.`);
    }
    const check = value as Record<string, unknown>;
    const id = requiredText(check.id, 'check id', 100);
    if (ids.has(id)) {
      throw new Error(`Duplicate acceptance check id: ${id}.`);
    }
    ids.add(id);
    const scenario = requiredText(check.scenario, 'scenario', ACCEPTANCE_SCENARIO_MAX);
    if (check.type === 'assertion') {
      return { id, scenario, type: 'assertion', description: requiredText(check.description, 'description'), expected: requiredText(check.expected, 'expected value') };
    }
    if (check.type === 'state') {
      return {
        id,
        scenario,
        type: 'state',
        instancePath: requiredText(check.instancePath, 'instancePath', 300),
        property: requiredText(check.property, 'property', 300),
        expected: requiredText(check.expected, 'expected value'),
      };
    }
    if (check.type === 'file') {
      const path = safeRelativePath(check.path, 'file path');
      const contains = check.contains === undefined ? undefined : requiredText(check.contains, 'contains', 1_000);
      const exact = optionalText(check.expected, 'expected', 2_000);
      if (contains === undefined && exact === undefined) {
        throw new Error(`Acceptance file check ${id} needs contains or expected.`);
      }
      return { id, scenario, type: 'file', path, ...(contains !== undefined ? { contains } : {}), ...(exact !== undefined ? { expected: exact } : {}) };
    }
    throw new Error(`Acceptance check ${id} has an unknown type.`);
  });
  const repairHint = record.repairHint === undefined ? undefined : requiredText(record.repairHint, 'repairHint', 1_000);
  const screenshotPaths = screenshots.map((value) => safeRelativePath(value, 'screenshot'));
  const totalText = checks.reduce((sum, check) => {
    if (check.type === 'assertion') {
      return sum + check.id.length + check.scenario.length + check.description.length + check.expected.length;
    }
    if (check.type === 'state') {
      return sum + check.id.length + check.scenario.length + check.instancePath.length + check.property.length + check.expected.length;
    }
    return sum + check.id.length + check.scenario.length + check.path.length + (check.contains?.length ?? 0) + (check.expected?.length ?? 0);
  }, 0) + screenshotPaths.reduce((sum, path) => sum + path.length, 0) + (repairHint?.length ?? 0);
  if (totalText > MAX_TOTAL_TEXT) {
    throw new Error(`Acceptance text is larger than ${MAX_TOTAL_TEXT} characters.`);
  }
  const renderedText = checks.reduce((sum, check) => {
    if (check.type === 'assertion') {
      return sum + JSON.stringify(check.description).length + JSON.stringify(check.expected).length + check.scenario.length + check.id.length;
    }
    if (check.type === 'state') {
      return sum + JSON.stringify(check.instancePath).length + JSON.stringify(check.property).length + JSON.stringify(check.expected).length + check.scenario.length + check.id.length;
    }
    return sum + JSON.stringify(check.path).length + (check.contains !== undefined ? JSON.stringify(check.contains).length : 0) + (check.expected !== undefined ? JSON.stringify(check.expected).length : 0) + check.scenario.length + check.id.length;
  }, 0) + screenshotPaths.reduce((sum, path) => sum + JSON.stringify(path).length, 0) + (repairHint ? JSON.stringify(repairHint).length : 0);
  if (renderedText > MAX_RENDERED_TEXT) {
    throw new Error(`Acceptance instruction is larger than ${MAX_RENDERED_TEXT} characters.`);
  }
  return { version: 1, checks, screenshots: screenshotPaths, ...(repairHint ? { repairHint } : {}) };
}

export function acceptanceForScenario(definition: AcceptanceDefinition, scenario: string): ScenarioAcceptance | null {
  const wanted = scenario.slice(0, ACCEPTANCE_SCENARIO_MAX);
  const scoped = definition.checks.filter((check) => check.scenario === wanted);
  if (scoped.length === 0) {
    return null;
  }
  return {
    assertions: scoped.filter((check): check is AcceptanceAssertion => check.type === 'assertion'),
    stateChecks: scoped.filter((check): check is AcceptanceStateCheck => check.type === 'state'),
    fileChecks: scoped.filter((check): check is AcceptanceFileCheck => check.type === 'file'),
    screenshots: [...definition.screenshots],
  };
}

/**
 * Run file checks for one scenario. The reader returns the file text, or null
 * when the file is missing or unreadable. A null result means every check
 * passed.
 */
export async function verifyAcceptanceFiles(
  definition: AcceptanceDefinition,
  scenario: string,
  read: (path: string) => Promise<string | null>,
): Promise<string | null> {
  const scoped = acceptanceForScenario(definition, scenario);
  if (!scoped) {
    return null;
  }
  for (const check of scoped.fileChecks) {
    const content = await read(check.path);
    if (content === null) {
      return `acceptance file ${check.path} is missing or unreadable`;
    }
    if (check.contains !== undefined && !content.includes(check.contains)) {
      return `acceptance file ${check.path} does not contain ${JSON.stringify(check.contains)}`;
    }
    if (check.expected !== undefined && content !== check.expected) {
      return `acceptance file ${check.path} does not equal the expected content`;
    }
  }
  return null;
}

export interface AcceptanceVerdict {
  status: 'PASSED' | 'FAILED' | 'NO_EVIDENCE';
  reason: string;
  failingCheck?: string;
}

function setKey(parts: string[]): string {
  return parts.map((part) => part.trim()).join('\u0000');
}

function describeSetKey(key: string): string {
  return JSON.stringify(key.split('\u0000').join(' / '));
}

function describeList(items: string[]): string {
  return items.length <= 3 ? items.join(', ') : `${items.slice(0, 3).join(', ')} (+${items.length - 3} more)`;
}

function compareSet(kind: 'assertion' | 'state', expectedParts: string[][], actualParts: string[][]): string | null {
  const expected = new Map<string, number>();
  const actual = new Map<string, number>();
  for (const parts of expectedParts) {
    const key = setKey(parts);
    expected.set(key, (expected.get(key) ?? 0) + 1);
  }
  for (const parts of actualParts) {
    const key = setKey(parts);
    actual.set(key, (actual.get(key) ?? 0) + 1);
  }
  const missing: string[] = [];
  const extra: string[] = [];
  for (const [key, count] of expected) {
    for (let index = actual.get(key) ?? 0; index < count; index += 1) {
      missing.push(describeSetKey(key));
    }
  }
  for (const [key, count] of actual) {
    for (let index = expected.get(key) ?? 0; index < count; index += 1) {
      extra.push(describeSetKey(key));
    }
  }
  if (missing.length === 0 && extra.length === 0) {
    return null;
  }
  const parts: string[] = [];
  if (missing.length > 0) {
    parts.push(`missing ${describeList(missing)}`);
  }
  if (extra.length > 0) {
    parts.push(`unexpected ${describeList(extra)}`);
  }
  return `${kind} set does not match (${parts.join('; ')})`;
}

/**
 * The declared assertion/state multiset must match exactly. A definition with
 * no assertion (or state) checks does not constrain that part of the report.
 */
export function verifyAcceptanceChecks(definition: AcceptanceDefinition, scenario: string, report: AcceptanceReport): string | null {
  const scoped = acceptanceForScenario(definition, scenario);
  if (!scoped) {
    return null;
  }
  if (scoped.assertions.length > 0) {
    const failure = compareSet(
      'assertion',
      scoped.assertions.map((check) => [check.description, check.expected]),
      report.assertions.map((check) => [check.description, check.expected]),
    );
    if (failure) {
      return failure;
    }
  }
  if (scoped.stateChecks.length > 0) {
    return compareSet(
      'state',
      scoped.stateChecks.map((check) => [check.instancePath, check.property, check.expected]),
      report.stateChecks.map((check) => [check.instancePath, check.property, check.expected]),
    );
  }
  return null;
}

/**
 * Apply acceptance definitions to a graded verdict. A malformed definition or
 * an unmet file check can only make a verdict stricter; it never turns a
 * failure into a pass. The first failing check is preserved when one exists.
 */
export async function applyAcceptanceChecks<T extends AcceptanceVerdict>(
  verdict: T,
  definition: AcceptanceDefinition | null,
  definitionError: string | null,
  scenario: string,
  read: (path: string) => Promise<string | null>,
  report?: AcceptanceReport | null,
): Promise<T> {
  if (definitionError) {
    const invalid = `Acceptance definition is invalid: ${definitionError}`;
    const reason = verdict.status === 'PASSED' ? invalid : `${verdict.reason} ${invalid}`.trim();
    return { ...verdict, status: 'FAILED', reason, failingCheck: verdict.failingCheck ?? 'acceptance definition' } as T;
  }
  if (!definition) {
    return verdict;
  }
  const setFailure = report ? verifyAcceptanceChecks(definition, scenario, report) : null;
  const fileFailure = await verifyAcceptanceFiles(definition, scenario, read);
  if (!setFailure && !fileFailure) {
    return verdict;
  }
  const details: string[] = [];
  if (setFailure) {
    details.push(`Acceptance assertion/state set failed: ${setFailure}.`);
  }
  if (fileFailure) {
    details.push(`Acceptance file check failed: ${fileFailure}.`);
  }
  const detail = details.join(' ');
  const primary = setFailure ? `acceptance set: ${setFailure}` : (fileFailure as string);
  if (verdict.status === 'PASSED') {
    return { ...verdict, status: 'FAILED', reason: detail, failingCheck: primary } as T;
  }
  return {
    ...verdict,
    status: 'FAILED',
    reason: `${verdict.reason} ${detail}`.trim(),
    failingCheck: verdict.failingCheck ?? primary,
  } as T;
}

export function acceptanceText(raw: string, strict: boolean): AcceptanceDefinition | null {
  try {
    return parseAcceptanceDefinition(raw);
  } catch (error) {
    if (strict) {
      throw error;
    }
    return null;
  }
}

export type AcceptanceReadOutcome =
  | { kind: 'missing' }
  | { kind: 'error'; error: unknown }
  | { kind: 'text'; raw: string };

export function acceptanceFromRead(outcome: AcceptanceReadOutcome, strict: boolean): AcceptanceDefinition | null {
  if (outcome.kind === 'missing') {
    return null;
  }
  if (outcome.kind === 'error') {
    if (strict) {
      throw outcome.error;
    }
    return null;
  }
  return acceptanceText(outcome.raw, strict);
}

export function acceptanceInstruction(definition: AcceptanceDefinition, scenario: string): string {
  const displayScenario = scenario.slice(0, ACCEPTANCE_SCENARIO_MAX);
  const scoped = acceptanceForScenario(definition, displayScenario);
  if (!scoped) {
    return '';
  }
  const checks = [
    ...scoped.assertions.map((check) => `- assertion ${JSON.stringify(check.description)}: expected ${JSON.stringify(check.expected)}`),
    ...scoped.stateChecks.map((check) => `- state ${check.instancePath}.${check.property}: expected ${JSON.stringify(check.expected)}`),
    ...scoped.fileChecks.map((check) => {
      const parts: string[] = [];
      if (check.contains !== undefined) {
        parts.push(`contains ${JSON.stringify(check.contains)}`);
      }
      if (check.expected !== undefined) {
        parts.push(`equals ${JSON.stringify(check.expected)}`);
      }
      return `- file ${check.path} ${parts.join(' and ')}`;
    }),
  ];
  const evidence = scoped.screenshots.length > 0 ? `\nEvidence screenshot path(s): ${scoped.screenshots.join(', ')}` : '';
  return [
    `Acceptance definition for ${JSON.stringify(displayScenario)}:`,
    definition.repairHint ? `Repair hint: ${definition.repairHint}` : '',
    ...checks,
    evidence,
    `When re-running, call report_playtest with scenario ${JSON.stringify(displayScenario)}, these checks with actual values from the run, and the listed screenshot path(s). Do not claim PASSED until the gate returns PASSED.`,
  ].filter(Boolean).join('\n');
}
