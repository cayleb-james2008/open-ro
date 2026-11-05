import { describe, expect, it } from 'vitest';
import {
  containsSecret,
  redactLogArgs,
  redactObject,
  redactSecrets,
  SECRET_PATTERNS,
} from '../electron/redaction';

describe('Secret Redaction Engine (electron/redaction.ts)', () => {
  it('has comprehensive secret patterns defined', () => {
    expect(SECRET_PATTERNS.length).toBeGreaterThanOrEqual(8);
  });

  describe('redactSecrets string scrubbing', () => {
    it('redacts OpenAI API keys cleanly', () => {
      const key = ['sk', 'proj', 'abc12345678901234567890xyz'].join('-');
      const raw = `Configured key ${key} in env.`;
      const scrubbed = redactSecrets(raw);
      expect(scrubbed).not.toContain('proj');
      expect(scrubbed).toContain('[REDACTED:OPENAI_KEY]');
    });

    it('redacts Anthropic API keys cleanly', () => {
      const key = ['sk', 'ant', 'api03-abcdef12345678901234567890xyz'].join('-');
      const raw = `Using Anthropic client with ${key} for chat.`;
      const scrubbed = redactSecrets(raw);
      expect(scrubbed).not.toContain('api03');
      expect(scrubbed).toContain('[REDACTED:ANTHROPIC_KEY]');
    });

    it('redacts OpenRouter API keys cleanly', () => {
      const key = ['sk', 'or', 'v1', 'abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789'].join('-');
      const raw = `Authorization: Bearer ${key}`;
      const scrubbed = redactSecrets(raw);
      expect(scrubbed).not.toContain('v1-abcdef');
      expect(scrubbed).toContain('[REDACTED:OPENROUTER_KEY]');
    });

    it('redacts Google Gemini API keys cleanly', () => {
      const key = 'AIza' + 'SyA12345678901234567890123456789012';
      const raw = `Endpoint url with key=${key} in params.`;
      const scrubbed = redactSecrets(raw);
      expect(scrubbed).not.toContain('SyA12345');
      expect(scrubbed).toContain('[REDACTED:GEMINI_KEY]');
    });

    it('redacts Roblox .ROBLOSECURITY cookies cleanly', () => {
      const cookie =
        ['_|WARNING:', 'DO-NOT-SHARE-THIS.--Sharing-this-will-allow-someone-to-log-in.|_123456789'].join('');
      const raw = `Cookie header: ${cookie}; Path=/`;
      const scrubbed = redactSecrets(raw);
      expect(scrubbed).not.toContain('DO-NOT-SHARE-THIS');
      expect(scrubbed).toContain('[REDACTED:ROBLOSECURITY]');
    });

    it('redacts JSON Web Tokens (JWT) cleanly', () => {
      const jwt =
        'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4iLCJpYXQiOjE1MTYyMzkwMjJ9.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c';
      const raw = `Bearer ${jwt} token attached`;
      const scrubbed = redactSecrets(raw);
      expect(scrubbed).not.toContain('eyJhbGci');
      expect(scrubbed).toContain('[REDACTED:JWT]');
    });

    it('redacts PEM private key blocks cleanly', () => {
      const header = '-----BEGIN ' + 'RSA PRIVATE KEY-----';
      const footer = '-----END ' + 'RSA PRIVATE KEY-----';
      const pem = `${header}\nMIIEowIBAAKCAQEA0Y1234567890abcdef\n${footer}`;
      const raw = `Certificate error with key:\n${pem}\nTraceback finished.`;
      const scrubbed = redactSecrets(raw);
      expect(scrubbed).not.toContain('MIIEowIBAAKCAQEA');
      expect(scrubbed).toContain('[REDACTED:PRIVATE_KEY]');
    });

    it('redacts basic authentication passwords in URLs', () => {
      const url = 'mongodb://app_user:superSecretPassword123@cluster0.mongodb.net/roblox';
      const scrubbed = redactSecrets(url);
      expect(scrubbed).not.toContain('superSecretPassword123');
      expect(scrubbed).toBe('mongodb://app_user:[REDACTED]@cluster0.mongodb.net/roblox');
    });

    it('metamorphic invariant: benign string without secrets is unchanged', () => {
      const benign = 'Roblox place opened at Workspace.Baseplate with 42 parts.';
      expect(redactSecrets(benign)).toBe(benign);
    });

    it('metamorphic invariant: redactSecrets is idempotent', () => {
      const key = ['sk', 'proj', '12345678901234567890123456'].join('-');
      const raw = `API ${key} and token eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3OCJ9.abc1234567890_xyz`;
      const pass1 = redactSecrets(raw);
      const pass2 = redactSecrets(pass1);
      expect(pass2).toBe(pass1);
    });
  });

  describe('containsSecret detection', () => {
    it('returns true when a secret pattern is present', () => {
      const gKey = 'AIza' + 'SyA12345678901234567890123456789012';
      const aKey = ['sk', 'ant', 'api03-abcdef12345678901234567890'].join('-');
      expect(containsSecret(`key=${gKey}`)).toBe(true);
      expect(containsSecret(aKey)).toBe(true);
    });

    it('returns false for clean, ordinary strings', () => {
      expect(containsSecret('local Players = game:GetService("Players")')).toBe(false);
      expect(containsSecret('https://roblox.com/games/123456/OpenRo')).toBe(false);
      expect(containsSecret('')).toBe(false);
    });
  });

  describe('redactObject deep sanitization', () => {
    it('redacts sensitive values inside nested structures and arrays', () => {
      const oKey = ['sk', 'proj', 'abcdef12345678901234567890'].join('-');
      const gKey = 'AIza' + 'SyA12345678901234567890123456789012';
      const payload = {
        session: 'sess-123',
        auth: {
          apiKey: oKey,
          headers: [`Authorization: Bearer ${gKey}`],
        },
        credentials: {
          password: 'plainPasswordHere',
        },
        metadata: {
          tags: ['roblox', 'scripting'],
        },
      };

      const sanitized = redactObject(payload);
      expect(sanitized.session).toBe('sess-123');
      expect(sanitized.auth.apiKey).toBe('[REDACTED:OPENAI_KEY]');
      expect(sanitized.credentials.password).toBe('[REDACTED]');
      expect(sanitized.auth.headers[0]).toContain('[REDACTED:GEMINI_KEY]');
      expect(sanitized.metadata.tags).toEqual(['roblox', 'scripting']);
    });

    it('handles circular object references without stack overflow', () => {
      const parent: Record<string, unknown> = { name: 'parent' };
      const child: Record<string, unknown> = { name: 'child', parent };
      parent.child = child;

      const sanitized = redactObject(parent);
      expect(sanitized.name).toBe('parent');
      expect((sanitized.child as Record<string, unknown>).name).toBe('child');
    });
  });

  describe('redactLogArgs helper', () => {
    it('scrubs multiple arguments cleanly', () => {
      const gKey = 'AIza' + 'SyA12345678901234567890123456789012';
      const args = redactLogArgs('Error with token:', gKey, { pass: '123' });
      expect(args[0]).toBe('Error with token:');
      expect(args[1]).toBe('[REDACTED:GEMINI_KEY]');
      expect(args[2]).toEqual({ pass: '[REDACTED]' });
    });
  });
});
