import { invoke } from "@tauri-apps/api/core";
import type {
  AccountSnapshot,
  AgentConfiguration,
  AgentDetection,
  ApiKeyGroup,
  ApiKeyRecord,
  CompatibleModel,
  ConfigureResult,
  CreatedApiKey,
  DeviceCode,
  LauncherSession,
} from "./types";

type UpdateStatus = {
  available: boolean;
  version: string | null;
  url: string | null;
};

const isTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const now = new Date().toISOString();

const demoSnapshot: AccountSnapshot = {
  plan: {
    planName: "Pro",
    paid: true,
    suspended: false,
    allowedTiers: ["starter", "pro"],
  },
  usage: {
    used: 174_260,
    daily: 500_000,
    remaining: 325_740,
    percentUsed: 35,
  },
  keys: [
    {
      prefix: "sk-accly-7H2P",
      groupType: "universal",
      allowedTiers: ["starter", "pro"],
      createdAt: now,
    },
  ],
};

let demoSession = false;
let demoKeys = [...demoSnapshot.keys];

const demoAgents: AgentDetection[] = [
  {
    id: "codex",
    name: "Codex",
    state: "ready",
    installed: true,
    configurable: true,
    configPath: "~/.codex/config.toml",
    detail: "Ready to connect",
    icon: "/codex-color.svg",
  },
  {
    id: "claude-code",
    name: "Claude Code",
    state: "installed",
    installed: true,
    configurable: true,
    configPath: "~/.claude/settings.json",
    detail: "Detected",
    icon: "/claudecode-color.svg",
  },
  {
    id: "gemini-cli",
    name: "Gemini CLI",
    state: "missing",
    installed: false,
    configurable: true,
    configPath: "~/.gemini/.env",
    detail: "Not installed",
    icon: "",
  },
  {
    id: "opencode",
    name: "OpenCode",
    state: "missing",
    installed: false,
    configurable: true,
    configPath: "~/.config/opencode/opencode.json",
    detail: "Not installed",
    icon: "/opencode.svg",
  },
  {
    id: "cursor",
    name: "Cursor",
    state: "unsupported",
    installed: true,
    configurable: false,
    configPath: "~/Library/Application Support/Cursor/User/settings.json",
    detail: "No safe gateway config detected",
    icon: "/cursor.svg",
  },
  {
    id: "windsurf",
    name: "Windsurf",
    state: "unsupported",
    installed: true,
    configurable: false,
    configPath: "~/Library/Application Support/Windsurf/User/settings.json",
    detail: "No safe gateway config detected",
    icon: "/windsurf.svg",
  },
];

export const compatibleModels: CompatibleModel[] = [
  {
    id: "gpt-5.4-codex",
    label: "GPT-5.4 Codex",
    tier: "pro",
    agents: ["codex", "opencode"],
    protocol: "responses",
  },
  {
    id: "claude-opus-4-6",
    label: "Claude Opus 4.6",
    tier: "pro",
    agents: ["claude-code", "opencode"],
    protocol: "anthropic",
  },
  {
    id: "gemini-3.1-pro",
    label: "Gemini 3.1 Pro",
    tier: "pro",
    agents: ["gemini-cli", "opencode"],
    protocol: "google",
  },
  {
    id: "gpt-5.4-mini",
    label: "GPT-5.4 Mini",
    tier: "starter",
    agents: ["codex", "opencode"],
    protocol: "responses",
  },
];

function demoKey(groupType: ApiKeyGroup): CreatedApiKey {
  const suffix = crypto
    .randomUUID()
    .replace(/-/g, "")
    .slice(0, 8)
    .toUpperCase();
  return {
    prefix: `sk-accly-${suffix.slice(0, 4)}`,
    fullKey: `sk-accly-dev-${suffix}`,
    groupType,
    allowedTiers: ["starter", "pro"],
    createdAt: new Date().toISOString(),
  };
}

export async function getLauncherSession(): Promise<LauncherSession | null> {
  if (!isTauri()) return demoSession ? { expiresAt: null } : null;
  return invoke<LauncherSession | null>("get_launcher_session");
}

export async function beginDeviceAuthorization(): Promise<DeviceCode> {
  if (!isTauri()) {
    return {
      deviceCode: "local-development-code",
      userCode: "ACCLY-DEV",
      verificationUri: "https://auth.accly.net/device",
      verificationUriComplete: "https://auth.accly.net/device?code=ACCLY-DEV",
      expiresIn: 1800,
      interval: 2,
    };
  }

  return invoke<DeviceCode>("begin_device_authorization");
}

export async function completeDeviceAuthorization(
  deviceCode: DeviceCode,
): Promise<LauncherSession | null> {
  if (!isTauri()) {
    demoSession = true;
    return { expiresAt: null };
  }

  return invoke<LauncherSession | null>("poll_device_authorization", {
    deviceCode,
  });
}

export async function signOut(): Promise<void> {
  if (!isTauri()) {
    demoSession = false;
    return;
  }
  await invoke("clear_launcher_session");
}

export async function getAccountSnapshot(): Promise<AccountSnapshot> {
  if (!isTauri()) return { ...demoSnapshot, keys: [...demoKeys] };
  return invoke<AccountSnapshot>("get_account_snapshot");
}

export async function listAgents(): Promise<AgentDetection[]> {
  if (!isTauri()) return demoAgents;
  return invoke<AgentDetection[]>("detect_agents");
}

export async function configureAgent(
  config: AgentConfiguration,
): Promise<ConfigureResult> {
  if (!isTauri()) {
    const agent = demoAgents.find(({ id }) => id === config.agentId);
    if (!agent?.configurable || !agent.configPath) {
      throw new Error(
        "This agent does not expose a safe configuration target.",
      );
    }
    agent.state = "ready";
    agent.detail = "Ready to connect";
    return {
      agentId: config.agentId,
      configPath: agent.configPath,
      backupPath: `~/Library/Application Support/Accly Launcher/backups/${config.agentId}`,
      message: `${agent.name} is connected to Accly.`,
    };
  }
  return invoke<ConfigureResult>("configure_agent", { config });
}

export async function createApiKey(
  groupType: ApiKeyGroup,
): Promise<CreatedApiKey> {
  if (!isTauri()) {
    const key = demoKey(groupType);
    demoKeys = [key, ...demoKeys];
    return key;
  }
  return invoke<CreatedApiKey>("create_api_key", { groupType });
}

export async function deleteApiKey(prefix: string): Promise<void> {
  if (!isTauri()) {
    demoKeys = demoKeys.filter((key) => key.prefix !== prefix);
    return;
  }
  await invoke("delete_api_key", { prefix });
}

export async function regenerateApiKey(prefix: string): Promise<CreatedApiKey> {
  if (!isTauri()) {
    const previous = demoKeys.find((key) => key.prefix === prefix);
    if (!previous) throw new Error("The API key no longer exists.");
    const replacement = demoKey(previous.groupType);
    demoKeys = [
      replacement,
      ...demoKeys.filter((key) => key.prefix !== prefix),
    ];
    return replacement;
  }
  return invoke<CreatedApiKey>("regenerate_api_key", { prefix });
}

export async function checkForUpdate(): Promise<UpdateStatus> {
  if (!isTauri()) return { available: false, version: null, url: null };
  return invoke<UpdateStatus>("check_for_update");
}

export type { ApiKeyRecord, UpdateStatus };
