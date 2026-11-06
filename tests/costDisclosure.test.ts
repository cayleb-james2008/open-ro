import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  calculateTurnCost,
  estimateTokensFromText,
  getDataFlowDisclosure,
} from '../electron/agent';

describe('Cost Disclosure & Data-Flow Egress Governance', () => {
  describe('Data-Flow Egress Classification', () => {
    it('categorizes Ollama and local models as LOCAL_ONLY with zero data egress', () => {
      const disclosure = getDataFlowDisclosure({ kind: 'ollama', baseUrl: 'http://127.0.0.1:11434' });
      expect(disclosure.category).toBe('LOCAL_ONLY');
      expect(disclosure.egressPayloads).toHaveLength(0);
      expect(disclosure.localOnlyPayloads).toContain('user prompts');
      expect(disclosure.localOnlyPayloads).toContain('project files and metadata');
      expect(disclosure.summary).toContain('100% on-device local execution');
    });

    it('categorizes subscription CLIs as SUBSCRIPTION_CLI with local subprocess execution', () => {
      const disclosure = getDataFlowDisclosure({ kind: 'codex-cli' });
      expect(disclosure.category).toBe('SUBSCRIPTION_CLI');
      expect(disclosure.destination).toContain('codex');
      expect(disclosure.egressPayloads).toEqual(['user prompt string']);
      expect(disclosure.localOnlyPayloads).toContain('unrelated environment credentials');
    });

    it('categorizes OpenAI and Anthropic as REMOTE_INFERENCE with explicit payload disclosure', () => {
      const disclosure = getDataFlowDisclosure({
        kind: 'openai-api',
        model: 'gpt-4o',
        baseUrl: 'https://api.openai.com',
      });
      expect(disclosure.category).toBe('REMOTE_INFERENCE');
      expect(disclosure.destination).toBe('https://api.openai.com');
      expect(disclosure.egressPayloads).toContain('conversation messages');
      expect(disclosure.egressPayloads).toContain('project index summary (names and sizes only)');
      expect(disclosure.localOnlyPayloads).toContain('system credentials and safeStorage keyring');
      expect(disclosure.localOnlyPayloads).toContain('unapproved staged file writes and commands');
    });
  });

  describe('Token & Cost Accounting Calculations', () => {
    it('rates local inference at $0.00 (Local Free)', () => {
      const cost = calculateTurnCost({ kind: 'ollama', model: 'qwen2.5:0.5b' }, 10_000, 5_000);
      expect(cost.isFreeOrLocal).toBe(true);
      expect(cost.estimatedCostUsd).toBe(0);
      expect(cost.costLabel).toContain('($0.00 / Local Free)');
      expect(cost.totalTokens).toBe(15_000);
    });

    it('rates subscription CLI turns as Subscription Covered', () => {
      const cost = calculateTurnCost({ kind: 'claude-code-cli' }, 4_000, 2_000);
      expect(cost.isFreeOrLocal).toBe(true);
      expect(cost.estimatedCostUsd).toBe(0);
      expect(cost.costLabel).toBe('Subscription Covered');
    });

    it('computes accurate USD pricing for OpenAI gpt-4o turns', () => {
      // gpt-4o: $2.50 prompt, $10.00 completion per million tokens
      // 100,000 prompt = $0.25; 50,000 completion = $0.50; total = $0.75
      const cost = calculateTurnCost(
        { kind: 'openai-api', model: 'gpt-4o' },
        100_000,
        50_000,
      );
      expect(cost.isFreeOrLocal).toBe(false);
      expect(cost.totalTokens).toBe(150_000);
      expect(cost.estimatedCostUsd).toBe(0.75);
      expect(cost.costLabel).toContain('$0.7500');
    });

    it('computes accurate USD pricing for Anthropic claude-3-5-sonnet turns', () => {
      // claude-3-5-sonnet: $3.00 prompt, $15.00 completion per million tokens
      // 10,000 prompt = $0.03; 2,000 completion = $0.03; total = $0.06
      const cost = calculateTurnCost(
        { kind: 'anthropic-api', model: 'claude-3-5-sonnet-20241022' },
        10_000,
        2_000,
      );
      expect(cost.isFreeOrLocal).toBe(false);
      expect(cost.totalTokens).toBe(12_000);
      expect(cost.estimatedCostUsd).toBe(0.06);
    });

    it('approximates token count from text using 4-character heuristics', () => {
      expect(estimateTokensFromText('')).toBe(0);
      expect(estimateTokensFromText('abcd')).toBe(1);
      expect(estimateTokensFromText('Hello, world! This is a test message.')).toBe(10);
    });
  });

  describe('User-Facing Disclosure UI in App.tsx', () => {
    const app = readFileSync('src/App.tsx', 'utf8').replace(/\r\n/g, '\n');

    it('renders cost disclosure tags on assistant message author headers', () => {
      expect(app).toContain('(message as { costLabel?: string }).costLabel');
      expect(app).toContain('title="Token consumption & estimated cost"');
    });

    it('renders data-flow disclosure indicators in composer hints', () => {
      expect(app).toContain("catalogByKind.get(activeProvider.kind)?.connection === 'local'");
      expect(app).toContain('🔒 Local Free');
      expect(app).toContain('⚡ CLI Subscription');
      expect(app).toContain('🌐 Cloud API');
      expect(app).toContain('100% on-device local execution. Zero data egress.');
    });
  });
});
