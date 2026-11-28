import { isRemoteRuntime } from "@/lib/remoteRuntime";
import { invoke } from "@tauri-apps/api/core";

const REMOTE_KEY_PREFIX = "__TERAX_REMOTE_KEY__:";

function remotePlaceholder(account: string): string {
  return `${REMOTE_KEY_PREFIX}${account}`;
}
import {
  getProvider,
  KEYRING_SERVICE,
  PROVIDERS,
  providerSupportsKey,
  type CustomEndpoint,
  type ProviderId,
} from "../config";

export type ProviderKeys = Record<ProviderId, string | null>;
export type CustomEndpointKeys = Record<string, string | null>;

export const EMPTY_PROVIDER_KEYS: ProviderKeys = {
  openai: null,
  anthropic: null,
  google: null,
  xai: null,
  cerebras: null,
  groq: null,
  deepseek: null,
  mistral: null,
  openrouter: null,
  "openai-compatible": null,
  lmstudio: null,
  mlx: null,
  ollama: null,
};

export async function getKey(provider: ProviderId): Promise<string | null> {
  if (!providerSupportsKey(provider)) return null;
  const account = getProvider(provider).keyringAccount;
  if (isRemoteRuntime()) {
    try {
      const [has] = await invoke<boolean[]>("secrets_has", {
        service: KEYRING_SERVICE,
        accounts: [account],
      });
      return has ? remotePlaceholder(account) : null;
    } catch {
      return null;
    }
  }
  try {
    const v = await invoke<string | null>("secrets_get", {
      service: KEYRING_SERVICE,
      account,
    });
    return v && v.length > 0 ? v : null;
  } catch {
    return null;
  }
}

export async function setKey(provider: ProviderId, key: string): Promise<void> {
  if (!providerSupportsKey(provider)) {
    throw new Error(`${provider} does not use an API key`);
  }
  const trimmed = key.trim();
  if (!trimmed) throw new Error("API key is empty");
  await invoke("secrets_set", {
    service: KEYRING_SERVICE,
    account: getProvider(provider).keyringAccount,
    password: trimmed,
  });
}

async function deleteKey(account: string): Promise<void> {
  await invoke("secrets_delete", { service: KEYRING_SERVICE, account });
}

export async function clearKey(provider: ProviderId): Promise<void> {
  if (!providerSupportsKey(provider)) return;
  await deleteKey(getProvider(provider).keyringAccount);
}

export async function getAllKeys(): Promise<ProviderKeys> {
  const out = { ...EMPTY_PROVIDER_KEYS };
  const need = PROVIDERS.filter((p) => providerSupportsKey(p.id));
  if (isRemoteRuntime()) {
    try {
      const results = await invoke<boolean[]>("secrets_has", {
        service: KEYRING_SERVICE,
        accounts: need.map((p) => p.keyringAccount),
      });
      need.forEach((p, i) => {
        out[p.id] = results[i] ? remotePlaceholder(p.keyringAccount) : null;
      });
    } catch {
      // Never fall back to secrets_get in remote mode.
    }
    return out;
  }
  try {
    const results = await invoke<(string | null)[]>("secrets_get_all", {
      service: KEYRING_SERVICE,
      accounts: need.map((p) => p.keyringAccount),
    });
    need.forEach((p, i) => {
      const v = results[i];
      out[p.id] = v && v.length > 0 ? v : null;
    });
    return out;
  } catch {
    const entries = await Promise.all(
      need.map(async (p) => [p.id, await getKey(p.id)] as const),
    );
    for (const [id, v] of entries) out[id] = v;
    return out;
  }
}

export function hasAnyKey(keys: ProviderKeys): boolean {
  return PROVIDERS.some((p) => providerSupportsKey(p.id) && !!keys[p.id]);
}

function compatKeyringAccount(endpointId: string): string {
  return `compat-${endpointId}-api-key`;
}

export async function getCustomEndpointKey(
  endpointId: string,
): Promise<string | null> {
  const account = compatKeyringAccount(endpointId);
  if (isRemoteRuntime()) {
    try {
      const [has] = await invoke<boolean[]>("secrets_has", {
        service: KEYRING_SERVICE,
        accounts: [account],
      });
      return has ? remotePlaceholder(account) : null;
    } catch {
      return null;
    }
  }
  try {
    const v = await invoke<string | null>("secrets_get", {
      service: KEYRING_SERVICE,
      account,
    });
    return v && v.length > 0 ? v : null;
  } catch {
    return null;
  }
}

export async function setCustomEndpointKey(
  endpointId: string,
  key: string,
): Promise<void> {
  const trimmed = key.trim();
  if (!trimmed) throw new Error("API key is empty");
  await invoke("secrets_set", {
    service: KEYRING_SERVICE,
    account: compatKeyringAccount(endpointId),
    password: trimmed,
  });
}

export async function clearCustomEndpointKey(
  endpointId: string,
): Promise<void> {
  await deleteKey(compatKeyringAccount(endpointId));
}

export async function getAllCustomEndpointKeys(
  endpoints: readonly CustomEndpoint[],
): Promise<CustomEndpointKeys> {
  if (endpoints.length === 0) return {};
  const out: CustomEndpointKeys = {};
  const accounts = endpoints.map((e) => compatKeyringAccount(e.id));
  if (isRemoteRuntime()) {
    try {
      const results = await invoke<boolean[]>("secrets_has", {
        service: KEYRING_SERVICE,
        accounts,
      });
      endpoints.forEach((e, i) => {
        out[e.id] = results[i] ? remotePlaceholder(accounts[i]) : null;
      });
    } catch {
      // Never fall back to secrets_get in remote mode.
    }
    return out;
  }
  try {
    const results = await invoke<(string | null)[]>("secrets_get_all", {
      service: KEYRING_SERVICE,
      accounts,
    });
    endpoints.forEach((e, i) => {
      const v = results[i];
      out[e.id] = v && v.length > 0 ? v : null;
    });
  } catch {
    for (const e of endpoints) {
      out[e.id] = await getCustomEndpointKey(e.id);
    }
  }
  return out;
}
