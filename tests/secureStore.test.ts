import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  createSecureStore,
  type SafeStorageBackend,
} from '../electron/secureStore';

describe('Secure credential store (electron/secureStore.ts)', () => {
  let tempDir: string;

  beforeEach(() => {
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'open-ro-secure-store-test-'));
  });

  afterEach(() => {
    fs.rmSync(tempDir, { recursive: true, force: true });
  });

  // Reversible mock cipher for testing without requiring real OS desktop keyrings
  const createMockBackend = (available = true): SafeStorageBackend => ({
    isEncryptionAvailable: () => available,
    encryptString: (plainText: string) => Buffer.from(`cipher:${plainText}`, 'utf8'),
    decryptString: (encrypted: Buffer) => {
      const str = encrypted.toString('utf8');
      if (!str.startsWith('cipher:')) {
        throw new Error('Decryption failure: invalid ciphertext payload');
      }
      return str.slice(7);
    },
  });

  it('reports secureStorageAvailable accurately based on backend capability', () => {
    const readyStore = createSecureStore({ userDataDir: tempDir, backend: createMockBackend(true) });
    expect(readyStore.secureStorageAvailable()).toBe(true);

    const unreadyStore = createSecureStore({ userDataDir: tempDir, backend: createMockBackend(false) });
    expect(unreadyStore.secureStorageAvailable()).toBe(false);

    const throwingStore = createSecureStore({
      userDataDir: tempDir,
      backend: {
        isEncryptionAvailable: () => {
          throw new Error('Keyring daemon uncontactable');
        },
        encryptString: () => Buffer.from(''),
        decryptString: () => '',
      },
    });
    expect(throwingStore.secureStorageAvailable()).toBe(false);
  });

  it('refuses to persist secrets when secure storage is unavailable (Gate D adherence)', () => {
    const insecureStore = createSecureStore({ userDataDir: tempDir, backend: createMockBackend(false) });
    const storeFilePath = insecureStore.storePath();

    expect(() => {
      insecureStore.saveCredential('provider:openai-api', 'sk-super-secret-key-12345');
    }).toThrow(/Secure storage unavailable; refusing to save credentials in plaintext/);

    // Verify zero plaintext files written to disk
    expect(fs.existsSync(storeFilePath)).toBe(false);
  });

  it('encrypts and round-trips credentials without writing plaintext to disk', () => {
    const store = createSecureStore({ userDataDir: tempDir, backend: createMockBackend(true) });
    const secret = 'sk-live-actual-openai-token-xyz789';

    store.saveCredential('provider:openai-api', secret);

    // Inspect the raw file content on disk directly
    const rawOnDisk = fs.readFileSync(store.storePath(), 'utf8');
    expect(rawOnDisk).not.toContain(secret);
    const parsed = JSON.parse(rawOnDisk) as Record<string, string>;
    const base64Cipher = parsed['provider:openai-api'];
    expect(base64Cipher).toBeDefined();
    expect(Buffer.from(base64Cipher, 'base64').toString('utf8')).toContain('cipher:');

    // Load and decrypt
    const loaded = store.loadCredential('provider:openai-api');
    expect(loaded).toBe(secret);
  });

  it('manages multiple service credentials independently and deletes cleanly', () => {
    const store = createSecureStore({ userDataDir: tempDir, backend: createMockBackend(true) });

    store.saveCredential('provider:openai-api', 'token-openai');
    store.saveCredential('provider:anthropic-api', 'token-anthropic');

    expect(store.listCredentialServices().sort()).toEqual([
      'provider:anthropic-api',
      'provider:openai-api',
    ]);

    expect(store.loadCredential('provider:openai-api')).toBe('token-openai');
    expect(store.loadCredential('provider:anthropic-api')).toBe('token-anthropic');

    store.deleteCredential('provider:openai-api');

    expect(store.listCredentialServices()).toEqual(['provider:anthropic-api']);
    expect(store.loadCredential('provider:openai-api')).toBeUndefined();
    expect(store.loadCredential('provider:anthropic-api')).toBe('token-anthropic');
  });

  it('handles corrupt JSON or invalid ciphertext gracefully without throwing', () => {
    const store = createSecureStore({ userDataDir: tempDir, backend: createMockBackend(true) });
    const storePath = store.storePath();

    // 1. Uninitialized or missing file
    expect(store.loadCredential('provider:non-existent')).toBeUndefined();
    expect(store.listCredentialServices()).toEqual([]);

    // 2. Corrupt JSON on disk
    fs.mkdirSync(tempDir, { recursive: true });
    fs.writeFileSync(storePath, '{ corrupt json garbage', 'utf8');
    expect(store.loadCredential('provider:openai-api')).toBeUndefined();
    expect(store.listCredentialServices()).toEqual([]);

    // 3. Tampered ciphertext payload
    fs.writeFileSync(storePath, JSON.stringify({ 'provider:openai-api': Buffer.from('tampered').toString('base64') }), 'utf8');
    expect(store.loadCredential('provider:openai-api')).toBeUndefined();
  });
});
