/**
 * Comprehensive secret redaction engine for Open-Ro.
 * Protects logs, diagnostic bundles, tool execution traces, and crash reports
 * from inadvertently leaking API keys, Roblox security tokens, JWTs, and private keys.
 */

export interface SecretPattern {
  name: string;
  pattern: RegExp;
  replacement: string;
}

export const SECRET_PATTERNS: SecretPattern[] = [
  {
    name: 'PRIVATE_KEY',
    pattern: /-----BEGIN\s+[A-Z\s]+PRIVATE\s+KEY-----[\s\S]*?-----END\s+[A-Z\s]+PRIVATE\s+KEY-----/g,
    replacement: '[REDACTED:PRIVATE_KEY]',
  },
  {
    name: 'ROBLOSECURITY',
    pattern: /_\|WARNING:?-?DO-NOT-SHARE-THIS[^\s;"]+/gi,
    replacement: '[REDACTED:ROBLOSECURITY]',
  },
  {
    name: 'OPENROUTER_KEY',
    pattern: /\bsk-or-v1-[a-zA-Z0-9_-]{20,}\b/g,
    replacement: '[REDACTED:OPENROUTER_KEY]',
  },
  {
    name: 'ANTHROPIC_KEY',
    pattern: /\bsk-ant-[a-zA-Z0-9_-]{20,}\b/g,
    replacement: '[REDACTED:ANTHROPIC_KEY]',
  },
  {
    name: 'OPENAI_KEY',
    pattern: /\bsk-(?:proj-)?[a-zA-Z0-9]{20,}\b/g,
    replacement: '[REDACTED:OPENAI_KEY]',
  },
  {
    name: 'GEMINI_KEY',
    pattern: /\bAIza[0-9A-Za-z_-]{35}\b/g,
    replacement: '[REDACTED:GEMINI_KEY]',
  },
  {
    name: 'GITHUB_PAT',
    pattern: /\bgh[pousr]_[A-Za-z0-9_]{36,}\b/g,
    replacement: '[REDACTED:GITHUB_PAT]',
  },
  {
    name: 'JWT_TOKEN',
    pattern: /\beyJ[a-zA-Z0-9_-]{10,}\.eyJ[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}\b/g,
    replacement: '[REDACTED:JWT]',
  },
  {
    name: 'URL_AUTH',
    pattern: /(:\/\/)([^:@\s]+):([^@\s]+)@/g,
    replacement: '$1$2:[REDACTED]@',
  },
];

/**
 * Scans a string and replaces all matching secret patterns with redaction markers.
 */
export function redactSecrets(input: string): string {
  if (!input || typeof input !== 'string') {
    return input;
  }
  let result = input;
  for (const { pattern, replacement } of SECRET_PATTERNS) {
    pattern.lastIndex = 0;
    result = result.replace(pattern, replacement);
  }
  return result;
}

/**
 * Checks if a given text contains any recognizable secrets.
 */
export function containsSecret(input: string): boolean {
  if (!input || typeof input !== 'string') {
    return false;
  }
  for (const { pattern } of SECRET_PATTERNS) {
    pattern.lastIndex = 0;
    if (pattern.test(input)) {
      return true;
    }
  }
  return false;
}

/**
 * Recursively redacts sensitive values inside objects, arrays, and primitive fields.
 */
export function redactObject<T>(value: T, depth = 0, seen = new WeakSet<object>()): T {
  if (depth > 12) {
    return value;
  }
  if (value === null || value === undefined) {
    return value;
  }
  if (typeof value === 'string') {
    return redactSecrets(value) as unknown as T;
  }
  if (typeof value !== 'object') {
    return value;
  }

  if (seen.has(value as object)) {
    return value;
  }
  seen.add(value as object);

  if (Array.isArray(value)) {
    return value.map((item) => redactObject(item, depth + 1, seen)) as unknown as T;
  }

  const redactedRecord: Record<string, unknown> = {};
  for (const [key, val] of Object.entries(value)) {
    const lowerKey = key.toLowerCase();
    const isSensitiveKey =
      lowerKey.includes('secret') ||
      lowerKey.includes('pass') ||
      lowerKey.includes('apikey') ||
      lowerKey.includes('credential') ||
      lowerKey.includes('auth') ||
      lowerKey.includes('token');

    if (typeof val === 'string' && val.length > 0) {
      const scrubbed = redactSecrets(val);
      if (scrubbed !== val) {
        redactedRecord[key] = scrubbed;
      } else if (isSensitiveKey) {
        redactedRecord[key] = '[REDACTED]';
      } else {
        redactedRecord[key] = val;
      }
    } else {
      redactedRecord[key] = redactObject(val, depth + 1, seen);
    }
  }
  return redactedRecord as T;
}

/**
 * Redacts an array of arbitrary log arguments for safe console or file emission.
 */
export function redactLogArgs(...args: unknown[]): unknown[] {
  return args.map((arg) => (typeof arg === 'string' ? redactSecrets(arg) : redactObject(arg)));
}
