import { invoke } from "@tauri-apps/api/core";
import type {
  AccountSnapshot,
  AgentConfiguration,
  AgentDetection,
  AgentLifecycleResult,
  AgentLifecycleProgress,
  ApiKeyCreateOptions,
  ApiKeyRecord,
  CompatibleModel,
  ConfigureResult,
  CreatedApiKey,
  DeviceCode,
  DeviceAuthorizationPoll,
  LauncherSession,
  ModelCatalogRecord,
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
    planName: "Max",
    paid: true,
    suspended: false,
    allowedTiers: ["basic", "advanced", "thinking", "beta"],
  },
  usage: {
    used: 174_260,
    daily: 500_000,
    remaining: 325_740,
    percentUsed: 35,
  },
  keys: [
    {
      name: "Launcher key",
      prefix: "sk-accly-7H2P",
      groupType: "universal",
      allowedTiers: ["basic", "advanced", "thinking", "beta"],
      allowedModelIds: [],
      creditLimitUsd: null,
      creditUsedUsd: 0,
      expiresAt: null,
      disabledReason: null,
      isActive: true,
      lastUsedAt: null,
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
    version: "0.99.0",
    installSource: "npm",
    executablePath: "~/.npm-global/bin/codex",
    installationCount: 1,
    canInstall: false,
    canUpdate: true,
    canRepair: false,
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
    version: "2.1.0",
    installSource: "npm",
    executablePath: "~/.npm-global/bin/claude",
    installationCount: 1,
    canInstall: false,
    canUpdate: true,
    canRepair: false,
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
    version: null,
    installSource: null,
    executablePath: null,
    installationCount: 0,
    canInstall: true,
    canUpdate: false,
    canRepair: false,
  },
  {
    id: "opencode",
    name: "OpenCode",
    state: "missing",
    installed: false,
    configurable: true,
    configPath: "~/.config/opencode/opencode.json[c]",
    detail: "Not installed",
    icon: "/opencode.svg",
    version: null,
    installSource: null,
    executablePath: null,
    installationCount: 0,
    canInstall: true,
    canUpdate: false,
    canRepair: false,
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
    version: null,
    installSource: null,
    executablePath: null,
    installationCount: 0,
    canInstall: false,
    canUpdate: false,
    canRepair: false,
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
    version: null,
    installSource: null,
    executablePath: null,
    installationCount: 0,
    canInstall: false,
    canUpdate: false,
    canRepair: false,
  },
];

export const compatibleModels: CompatibleModel[] = [
  {
    id: "gpt-5-4-mini",
    label: "GPT-5.4 Mini",
    tier: "basic",
    agents: ["codex", "opencode"],
    protocol: "responses",
  },
  {
    id: "gpt-5-4",
    label: "GPT-5.4",
    tier: "advanced",
    agents: ["codex", "opencode"],
    protocol: "responses",
  },
  {
    id: "gpt-5-4-pro",
    label: "GPT-5.4 Pro",
    tier: "beta",
    agents: ["codex", "opencode"],
    protocol: "responses",
  },
  {
    id: "claude-haiku-4-5-20251001",
    label: "Claude Haiku 4.5",
    tier: "basic",
    agents: ["claude-code", "opencode"],
    protocol: "anthropic",
  },
  {
    id: "claude-sonnet-4-6",
    label: "Claude Sonnet 4.6",
    tier: "advanced",
    agents: ["claude-code", "opencode"],
    protocol: "anthropic",
  },
  {
    id: "claude-sonnet-4-6-thinking",
    label: "Claude Sonnet 4.6 Thinking",
    tier: "thinking",
    agents: ["claude-code", "opencode"],
    protocol: "anthropic",
  },
  {
    id: "gemini-3.1-flash-lite",
    label: "Gemini 3.1 Flash-Lite",
    tier: "basic",
    agents: ["gemini-cli", "opencode"],
    protocol: "google",
  },
  {
    id: "gemini-3.5-flash",
    label: "Gemini 3.5 Flash",
    tier: "advanced",
    agents: ["gemini-cli", "opencode"],
    protocol: "google",
  },
  {
    id: "gemini-3.1-pro-preview",
    label: "Gemini 3.1 Pro Preview",
    tier: "advanced",
    agents: ["gemini-cli", "opencode"],
    protocol: "google",
  },
];

function demoKey(options: ApiKeyCreateOptions): CreatedApiKey {
  const suffix = crypto
    .randomUUID()
    .replace(/-/g, "")
    .slice(0, 8)
    .toUpperCase();
  return {
    name: options.name,
    prefix: `sk-accly-${suffix.slice(0, 4)}`,
    fullKey: `sk-accly-dev-${suffix}`,
    groupType: "universal",
    allowedTiers: ["basic", "advanced", "thinking", "beta"],
    allowedModelIds: options.allowedModelIds,
    creditLimitUsd: options.creditLimitUsd,
    creditUsedUsd: 0,
    expiresAt: options.expiresAt,
    disabledReason: null,
    isActive: true,
    lastUsedAt: null,
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
      verificationUriComplete:
        "https://auth.accly.net/device?user_code=ACCLY-DEV",
      expiresIn: 900,
      interval: 2,
    };
  }

  return invoke<DeviceCode>("begin_device_authorization");
}

export async function completeDeviceAuthorization(
  deviceCode: DeviceCode,
): Promise<DeviceAuthorizationPoll> {
  if (!isTauri()) {
    demoSession = true;
    return {
      status: "completed",
      session: { expiresAt: null },
    };
  }

  return invoke<DeviceAuthorizationPoll>("poll_device_authorization", {
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

function completeDemoAgentLifecycle(
  agentId: AgentDetection["id"],
  action: AgentLifecycleResult["action"],
): AgentLifecycleResult {
  const agent = demoAgents.find(({ id }) => id === agentId);
  if (
    !agent?.configurable ||
    (!agent.canInstall && action === "install") ||
    (!agent.canRepair && action === "repair")
  ) {
    throw new Error("This agent cannot be installed by Accly Launcher.");
  }

  if (action === "install") {
    agent.installed = true;
    agent.state = "installed";
    agent.detail = "Installed, ready to configure";
    agent.version = "latest";
    agent.installSource = "npm";
    agent.executablePath = `~/.npm-global/bin/${agent.id === "claude-code" ? "claude" : agent.id === "gemini-cli" ? "gemini" : agent.id}`;
    agent.installationCount = 1;
    agent.canInstall = false;
    agent.canUpdate = true;
    agent.canRepair = false;
  }

  if (action === "repair") {
    agent.state = "installed";
    agent.detail = "Repaired, ready to configure";
    agent.canRepair = false;
    agent.canUpdate = true;
  }

  return {
    agent: { ...agent },
    action,
    message: `${agent.name} ${action} completed. Scan results were refreshed.`,
  };
}

export async function installAgent(
  agentId: AgentDetection["id"],
): Promise<AgentLifecycleResult> {
  if (!isTauri()) return completeDemoAgentLifecycle(agentId, "install");
  return invoke<AgentLifecycleResult>("install_agent", { agentId });
}

export async function updateAgent(
  agentId: AgentDetection["id"],
): Promise<AgentLifecycleResult> {
  if (!isTauri()) return completeDemoAgentLifecycle(agentId, "update");
  return invoke<AgentLifecycleResult>("update_agent", { agentId });
}

export async function repairAgent(
  agentId: AgentDetection["id"],
): Promise<AgentLifecycleResult> {
  if (!isTauri()) return completeDemoAgentLifecycle(agentId, "repair");
  return invoke<AgentLifecycleResult>("repair_agent", { agentId });
}

export async function listenForAgentLifecycleProgress(
  onProgress: (progress: AgentLifecycleProgress) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<AgentLifecycleProgress>("agent-lifecycle", (event) => {
    onProgress(event.payload);
  });
}

export async function createApiKey(
  options: ApiKeyCreateOptions,
): Promise<CreatedApiKey> {
  if (!isTauri()) {
    const key = demoKey(options);
    demoKeys = [key, ...demoKeys];
    return key;
  }
  return invoke<CreatedApiKey>("create_api_key", { options });
}

export async function getModelCatalog(): Promise<ModelCatalogRecord[]> {
  if (!isTauri()) {
    return compatibleModels.map((model) => ({
      id: model.id,
      name: model.label,
      providerFamily: model.protocol,
      modelType: "generation",
      apiOnly: false,
      tier: model.tier,
    }));
  }
  return invoke<ModelCatalogRecord[]>("get_model_catalog");
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
    const replacement = demoKey({
      name: previous.name,
      allowedModelIds: previous.allowedModelIds,
      creditLimitUsd: previous.creditLimitUsd,
      expiresAt: previous.expiresAt,
    });
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
