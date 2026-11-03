import { describe, expect, it } from 'vitest';
import { getProviderOption, providerOptions } from '../electron/providerCatalog';

describe('provider catalog', () => {
  it('includes direct APIs, official CLI sign-in, local inference and custom endpoints', () => {
    const kinds = providerOptions.map((item) => item.kind);
    expect(kinds).toContain('openai-api');
    expect(kinds).toContain('codex-cli');
    expect(kinds).toContain('anthropic-api');
    expect(kinds).toContain('claude-code-cli');
    expect(kinds).toContain('openrouter-api');
    expect(kinds).toContain('opencode-cli');
    expect(kinds).toContain('ollama');
    expect(kinds).toContain('custom-openai');
  });

  it('provides setup instructions and cost notes for every provider', () => {
    for (const option of providerOptions) {
      expect(option.setup.length).toBeGreaterThan(0);
      expect(option.url.startsWith('https://')).toBe(true);
      expect(option.costNote.length).toBeGreaterThan(10);
    }
  });

  it('rejects unknown provider identifiers', () => {
    expect(() => getProviderOption('other' as never)).toThrow('Unknown provider kind');
  });
});
