import { app, safeStorage } from 'electron';
import * as fs from 'node:fs';
import * as path from 'node:path';

/**
 * Credential storage guarded by Electron safeStorage, per the charter's
 * no-plaintext-fallback rule: if OS secure storage is unavailable we refuse to
 * persist secrets at all rather than writing them in the clear.
 */
const STORE_FILE = 'credentials.json';

interface CredentialFile {
  /** base64 blob encrypted by safeStorage, keyed by provider kind. */
  [service: string]: string;
}

export function storePath(): string {
  return path.join(app.getPath('userData'), STORE_FILE);
}

export function secureStorageAvailable(): boolean {
  try {
    return safeStorage.isEncryptionAvailable();
  } catch {
    return false;
  }
}

function readStore(): CredentialFile {
  try {
    const raw = fs.readFileSync(storePath(), 'utf8');
    const parsed = JSON.parse(raw) as CredentialFile;
    return typeof parsed === 'object' && parsed !== null ? parsed : {};
  } catch {
    return {};
  }
}

function writeStore(store: CredentialFile): void {
  fs.mkdirSync(app.getPath('userData'), { recursive: true });
  fs.writeFileSync(storePath(), JSON.stringify(store, null, 2), { encoding: 'utf8', mode: 0o600 });
}

export function saveCredential(service: string, secret: string): void {
  if (!secureStorageAvailable()) {
    throw new Error('Secure storage unavailable; refusing to save credentials in plaintext. Install and unlock a desktop keyring, then restart Open-Ro.');
  }
  const store = readStore();
  store[service] = safeStorage.encryptString(secret).toString('base64');
  writeStore(store);
}

export function loadCredential(service: string): string | undefined {
  const entry = readStore()[service];
  if (!entry || !secureStorageAvailable()) {
    return undefined;
  }
  try {
    return safeStorage.decryptString(Buffer.from(entry, 'base64'));
  } catch {
    return undefined;
  }
}

export function deleteCredential(service: string): void {
  const store = readStore();
  delete store[service];
  writeStore(store);
}

export function listCredentialServices(): string[] {
  return Object.keys(readStore());
}
