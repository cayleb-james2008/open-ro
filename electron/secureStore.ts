import { app, safeStorage } from 'electron';
import * as fs from 'node:fs';
import * as path from 'node:path';

/**
 * Credential storage guarded by Electron safeStorage, per the charter's
 * no-plaintext-fallback rule: if OS secure storage is unavailable we refuse to
 * persist secrets at all rather than writing them in the clear.
 */
export const STORE_FILE = 'credentials.json';

export interface CredentialFile {
  /** base64 blob encrypted by safeStorage, keyed by provider kind. */
  [service: string]: string;
}

export interface SafeStorageBackend {
  isEncryptionAvailable: () => boolean;
  encryptString: (plainText: string) => Buffer;
  decryptString: (encrypted: Buffer) => string;
}

export interface SecureStore {
  storePath: () => string;
  secureStorageAvailable: () => boolean;
  saveCredential: (service: string, secret: string) => void;
  loadCredential: (service: string) => string | undefined;
  deleteCredential: (service: string) => void;
  listCredentialServices: () => string[];
}

export function createSecureStore(options?: {
  userDataDir?: string;
  backend?: SafeStorageBackend;
}): SecureStore {
  const getDir = (): string => options?.userDataDir ?? (typeof app?.getPath === 'function' ? app.getPath('userData') : '');
  const getPath = (): string => path.join(getDir(), STORE_FILE);

  const getBackend = (): SafeStorageBackend | null => {
    if (options?.backend) {
      return options.backend;
    }
    if (typeof safeStorage !== 'undefined' && safeStorage && typeof safeStorage.isEncryptionAvailable === 'function') {
      return safeStorage;
    }
    return null;
  };

  const isAvailable = (): boolean => {
    try {
      const backend = getBackend();
      return backend ? backend.isEncryptionAvailable() : false;
    } catch {
      return false;
    }
  };

  const readStore = (): CredentialFile => {
    try {
      const p = getPath();
      if (!p || !fs.existsSync(p)) {
        return {};
      }
      const raw = fs.readFileSync(p, 'utf8');
      const parsed = JSON.parse(raw) as CredentialFile;
      return typeof parsed === 'object' && parsed !== null ? parsed : {};
    } catch {
      return {};
    }
  };

  const writeStore = (store: CredentialFile): void => {
    const dir = getDir();
    if (!dir) {
      throw new Error('userData directory is not configured for secureStore');
    }
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(getPath(), JSON.stringify(store, null, 2), { encoding: 'utf8', mode: 0o600 });
  };

  return {
    storePath: getPath,
    secureStorageAvailable: isAvailable,
    saveCredential: (service: string, secret: string) => {
      const backend = getBackend();
      if (!backend || !isAvailable()) {
        throw new Error('Secure storage unavailable; refusing to save credentials in plaintext. Install and unlock a desktop keyring, then restart Open-Ro.');
      }
      const store = readStore();
      store[service] = backend.encryptString(secret).toString('base64');
      writeStore(store);
    },
    loadCredential: (service: string) => {
      const entry = readStore()[service];
      const backend = getBackend();
      if (!entry || !backend || !isAvailable()) {
        return undefined;
      }
      try {
        return backend.decryptString(Buffer.from(entry, 'base64'));
      } catch {
        return undefined;
      }
    },
    deleteCredential: (service: string) => {
      const store = readStore();
      delete store[service];
      writeStore(store);
    },
    listCredentialServices: () => Object.keys(readStore()),
  };
}

let defaultStore: SecureStore | null = null;
function getDefaultStore(): SecureStore {
  if (!defaultStore) {
    defaultStore = createSecureStore();
  }
  return defaultStore;
}

export function storePath(): string {
  return getDefaultStore().storePath();
}

export function secureStorageAvailable(): boolean {
  return getDefaultStore().secureStorageAvailable();
}

export function saveCredential(service: string, secret: string): void {
  getDefaultStore().saveCredential(service, secret);
}

export function loadCredential(service: string): string | undefined {
  return getDefaultStore().loadCredential(service);
}

export function deleteCredential(service: string): void {
  getDefaultStore().deleteCredential(service);
}

export function listCredentialServices(): string[] {
  return getDefaultStore().listCredentialServices();
}
