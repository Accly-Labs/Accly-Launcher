export type AgentId =
  "codex" | "claude-code" | "gemini-cli" | "opencode" | "cursor" | "windsurf";

export type AgentState =
  "ready" | "installed" | "missing" | "unsupported" | "broken";

export interface AgentDetection {
  id: AgentId;
  name: string;
  state: AgentState;
  installed: boolean;
  configurable: boolean;
  configPath: string | null;
  detail: string;
  icon: string;
}

export interface AgentConfiguration {
  agentId: AgentId;
  endpoint: string;
  apiKey: string;
  model: string;
}

export interface ConfigureResult {
  agentId: AgentId;
  configPath: string;
  backupPath: string;
  message: string;
}

export interface DeviceCode {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  verificationUriComplete: string | null;
  expiresIn: number;
  interval: number;
}

export interface LauncherSession {
  expiresAt: string | null;
}

export interface PlanAccess {
  planName: string;
  paid: boolean;
  suspended: boolean;
  allowedTiers: string[];
}

export interface UsageSummary {
  used: number;
  daily: number;
  remaining: number;
  percentUsed: number;
}

export interface ApiKeyRecord {
  prefix: string;
  groupType: ApiKeyGroup;
  allowedTiers: string[];
  createdAt: string;
}

export type ApiKeyGroup = "anthropic" | "openai" | "google" | "universal";

export interface CreatedApiKey extends ApiKeyRecord {
  fullKey: string;
}

export interface AccountSnapshot {
  plan: PlanAccess;
  usage: UsageSummary;
  keys: ApiKeyRecord[];
}

export interface CompatibleModel {
  id: string;
  label: string;
  tier: string;
  agents: AgentId[];
  protocol: "responses" | "anthropic" | "google" | "openai";
}

export interface AgentAdapter {
  detect(): Promise<AgentDetection>;
  configure(config: AgentConfiguration): Promise<ConfigureResult>;
  validate(): Promise<{ valid: boolean; detail: string }>;
  backup(): Promise<{ path: string }>;
  restore(handle: { path: string }): Promise<void>;
}
