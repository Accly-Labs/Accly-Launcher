import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AlertCircle,
  ArrowUpRight,
  BadgeCheck,
  Check,
  CircleAlert,
  CircleCheck,
  CircleDashed,
  CircleX,
  Copy,
  Download,
  KeyRound,
  LoaderCircle,
  LogOut,
  RefreshCw,
  RotateCw,
  Settings2,
  Trash2,
} from "lucide-react";
import { Button } from "./components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
} from "./components/ui/dialog";
import {
  beginDeviceAuthorization,
  checkForUpdate,
  compatibleModels,
  completeDeviceAuthorization,
  configureAgent,
  createApiKey,
  deleteApiKey,
  getAccountSnapshot,
  getModelCatalog,
  getLauncherSession,
  listAgents,
  listenForAgentLifecycleProgress,
  repairAgent,
  regenerateApiKey,
  signOut,
  installAgent,
  updateAgent,
} from "./lib/native";
import type {
  AgentDetection,
  AgentId,
  AgentLifecycleProgress,
  ApiKeyCreateOptions,
  ApiKeyRecord,
  CompatibleModel,
  CreatedApiKey,
  DeviceCode,
  ModelCatalogRecord,
} from "./lib/types";
import { formatDate, formatUsage } from "./lib/utils";

type Notice = { tone: "success" | "error"; message: string } | null;

const ACCOUNT_REFRESH_INTERVAL_MS = 5 * 60 * 1000;
const UPDATE_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;
const AGENT_SCAN_INTERVAL_MS = 30 * 1000;
const DEVICE_CODE_EXPIRED_MESSAGE =
  "This verification code expired. Generate a new code to continue.";
const SESSION_EXPIRED_MESSAGE =
  "Your launcher session has expired. Reconnect to continue.";

type PendingDeviceCode = DeviceCode & {
  expiresAt: number;
};

type KeyExpirationMode = "never" | "hour" | "day" | "month" | "custom";

type KeyFormState = {
  name: string;
  allowedModelIds: string[];
  unlimitedCredits: boolean;
  creditLimitUsd: string;
  expirationMode: KeyExpirationMode;
  customExpiresAt: string;
};

const emptyKeyForm = (): KeyFormState => ({
  name: "Launcher key",
  allowedModelIds: [],
  unlimitedCredits: true,
  creditLimitUsd: "",
  expirationMode: "never",
  customExpiresAt: "",
});

function expirationDateFor(form: KeyFormState): string | null {
  if (form.expirationMode === "never") return null;
  if (form.expirationMode === "custom") {
    const date = new Date(form.customExpiresAt);
    if (!form.customExpiresAt || Number.isNaN(date.getTime())) return null;
    return date.toISOString();
  }

  const date = new Date();
  const durations: Record<
    Exclude<KeyExpirationMode, "never" | "custom">,
    number
  > = {
    hour: 60 * 60 * 1000,
    day: 24 * 60 * 60 * 1000,
    month: 30 * 24 * 60 * 60 * 1000,
  };
  date.setTime(date.getTime() + durations[form.expirationMode]);
  return date.toISOString();
}

function localDateTimeInputValue(date: Date): string {
  const timezoneOffset = date.getTimezoneOffset();
  return new Date(date.getTime() - timezoneOffset * 60_000)
    .toISOString()
    .slice(0, 16);
}

function keyOptionsForForm(form: KeyFormState): ApiKeyCreateOptions {
  return {
    name: form.name.trim(),
    allowedModelIds: form.allowedModelIds,
    creditLimitUsd: form.unlimitedCredits ? null : Number(form.creditLimitUsd),
    expiresAt: expirationDateFor(form),
  };
}

const gatewayUrl =
  import.meta.env.VITE_ACCLY_GATEWAY_URL ?? "https://api.accly.net";

export function messageFrom(error: unknown) {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (
    error &&
    typeof error === "object" &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    return error.message;
  }
  return "Something went wrong. Try again.";
}

async function copyToClipboard(value: string) {
  await navigator.clipboard.writeText(value);
}

async function openExternal(url: string) {
  if ("__TAURI_INTERNALS__" in window) {
    await openUrl(url);
    return;
  }
  window.open(url, "_blank", "noopener,noreferrer");
}

function formatRemainingTime(totalSeconds: number) {
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

function useRemainingSeconds(expiresAt: number | null) {
  const [currentTime, setCurrentTime] = useState(() => Date.now());

  useEffect(() => {
    if (!expiresAt) return;

    setCurrentTime(Date.now());
    const interval = window.setInterval(
      () => setCurrentTime(Date.now()),
      1_000,
    );
    return () => window.clearInterval(interval);
  }, [expiresAt]);

  if (!expiresAt) return null;
  return Math.max(0, Math.ceil((expiresAt - currentTime) / 1_000));
}

function AgentIcon({ agent }: { agent: AgentDetection }) {
  if (!agent.icon) {
    return (
      <span className="agent-fallback" aria-hidden="true">
        {agent.name.slice(0, 1)}
      </span>
    );
  }

  return (
    <img
      className="agent-icon"
      src={agent.icon}
      alt=""
      onError={(event) => {
        event.currentTarget.style.display = "none";
      }}
    />
  );
}

function AgentStateIcon({ state }: { state: AgentDetection["state"] }) {
  if (state === "ready") return <CircleCheck size={15} aria-hidden="true" />;
  if (state === "broken") return <CircleAlert size={15} aria-hidden="true" />;
  if (state === "missing") return <CircleDashed size={15} aria-hidden="true" />;
  if (state === "unsupported") return <CircleX size={15} aria-hidden="true" />;
  return <Check size={15} aria-hidden="true" />;
}

function SignInScreen({
  deviceCode,
  deviceCodeExpiresAt,
  error,
  starting,
  onStart,
  onStartOver,
}: {
  deviceCode: DeviceCode | null;
  deviceCodeExpiresAt: number | null;
  error: string | null;
  starting: boolean;
  onStart: () => Promise<void>;
  onStartOver: () => void;
}) {
  const remainingSeconds = useRemainingSeconds(deviceCodeExpiresAt);
  const startLabel =
    error === DEVICE_CODE_EXPIRED_MESSAGE ? "Generate new code" : "Continue";

  const openVerification = () => {
    if (!deviceCode) return;
    void openExternal(
      deviceCode.verificationUriComplete ?? deviceCode.verificationUri,
    );
  };

  return (
    <main className="signin-shell">
      <section className="signin-panel" aria-labelledby="sign-in-title">
        <img className="signin-logo" src="/logo-white.svg" alt="Accly" />
        <h1 id="sign-in-title">Connect your Accly account.</h1>
        <p>Use your browser to approve this launcher.</p>

        {deviceCode ? (
          <>
            <div className="device-code">
              <p className="device-code-label">Verification code</p>
              <p className="device-code-value">{deviceCode.userCode}</p>
              {remainingSeconds !== null ? (
                <p className="device-code-expiry" aria-live="polite">
                  Expires in {formatRemainingTime(remainingSeconds)}
                </p>
              ) : null}
            </div>
            <div className="signin-actions signin-actions--approval">
              <Button
                className="signin-primary-action"
                variant="primary"
                onClick={openVerification}
              >
                Open browser <ArrowUpRight size={15} />
              </Button>
            </div>
            <p className="signin-approval-status" aria-live="polite">
              <CircleDashed size={16} aria-hidden="true" /> Waiting for approval
            </p>
            <div className="signin-reset">
              <Button variant="quiet" size="compact" onClick={onStartOver}>
                <RotateCw size={14} /> Start over
              </Button>
            </div>
          </>
        ) : (
          <div className="signin-actions">
            <Button
              className="signin-primary-action"
              variant="primary"
              onClick={() => void onStart()}
              disabled={starting}
            >
              {starting ? (
                <LoaderCircle className="animate-spin" size={15} />
              ) : null}
              {startLabel}
            </Button>
          </div>
        )}

        {error ? <p className="error-copy mt-5">{error}</p> : null}
      </section>
    </main>
  );
}

function SessionBootstrap() {
  return (
    <main className="signin-shell" aria-label="Checking secure session">
      <section className="signin-panel session-status" aria-live="polite">
        <img className="signin-logo" src="/logo-white.svg" alt="Accly" />
        <KeyRound
          className="session-status-icon"
          size={23}
          aria-hidden="true"
        />
        <h1>Checking secure session.</h1>
        <p>macOS may ask for Keychain access.</p>
      </section>
    </main>
  );
}

function SessionReadError({
  error,
  onRetry,
}: {
  error: unknown;
  onRetry: () => void;
}) {
  return (
    <main className="signin-shell">
      <section className="signin-panel" aria-labelledby="session-error-title">
        <img className="signin-logo" src="/logo-white.svg" alt="Accly" />
        <h1 id="session-error-title">Secure session unavailable.</h1>
        <p className="error-copy">{messageFrom(error)}</p>
        <div className="signin-actions">
          <Button variant="secondary" onClick={onRetry}>
            <RefreshCw size={15} /> Retry
          </Button>
        </div>
      </section>
    </main>
  );
}

function KeyDialog({
  open,
  keys,
  onOpenChange,
  onChanged,
  onNotice,
}: {
  open: boolean;
  keys: ApiKeyRecord[];
  onOpenChange: (open: boolean) => void;
  onChanged: () => Promise<void>;
  onNotice: (notice: Notice) => void;
}) {
  const [form, setForm] = useState<KeyFormState>(emptyKeyForm);
  const [created, setCreated] = useState<CreatedApiKey | null>(null);

  const modelQuery = useQuery({
    queryKey: ["launcher-model-catalog"],
    queryFn: getModelCatalog,
    enabled: open,
    staleTime: 5 * 60 * 1000,
    retry: false,
  });
  const models = useMemo<ModelCatalogRecord[]>(
    () =>
      modelQuery.data?.length
        ? modelQuery.data
        : compatibleModels.map((model) => ({
            id: model.id,
            name: model.label,
            providerFamily: model.protocol,
            modelType: "generation",
            apiOnly: false,
            tier: model.tier,
          })),
    [modelQuery.data],
  );

  const createMutation = useMutation({
    mutationFn: createApiKey,
    onSuccess: async (key) => {
      setCreated(key);
      await onChanged();
    },
  });

  const setFormValue = (patch: Partial<KeyFormState>) =>
    setForm((current) => ({ ...current, ...patch }));
  const toggleModel = (modelId: string) =>
    setForm((current) => ({
      ...current,
      allowedModelIds: current.allowedModelIds.includes(modelId)
        ? current.allowedModelIds.filter((id) => id !== modelId)
        : [...current.allowedModelIds, modelId],
    }));
  const options = keyOptionsForForm(form);
  const invalidLimit =
    !form.unlimitedCredits &&
    (!Number.isFinite(Number(form.creditLimitUsd)) ||
      Number(form.creditLimitUsd) <= 0);
  const invalidExpiration =
    form.expirationMode === "custom" && options.expiresAt === null;
  const canCreate =
    form.name.trim().length >= 2 && !invalidLimit && !invalidExpiration;

  const deleteMutation = useMutation({
    mutationFn: deleteApiKey,
    onSuccess: onChanged,
  });

  const regenerateMutation = useMutation({
    mutationFn: regenerateApiKey,
    onSuccess: async (key) => {
      setCreated(key);
      await onChanged();
    },
  });

  const copyKey = async (value: string) => {
    try {
      await copyToClipboard(value);
      onNotice({ tone: "success", message: "Copied to clipboard." });
    } catch {
      onNotice({ tone: "error", message: "Clipboard access is unavailable." });
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) {
          setCreated(null);
          setForm(emptyKeyForm());
        }
        onOpenChange(nextOpen);
      }}
    >
      <DialogContent>
        <DialogHeader>
          <h2 className="dialog-title">API keys</h2>
          <p className="dialog-subtitle">
            Keys are shown in full only when created or renewed.
          </p>
        </DialogHeader>
        <div className="dialog-body">
          {created ? (
            <div className="secret-reveal">
              <span className="field-label">New key</span>
              <code>{created.fullKey}</code>
              <Button
                className="mt-3"
                size="compact"
                onClick={() => void copyKey(created.fullKey)}
              >
                <Copy size={13} /> Copy
              </Button>
            </div>
          ) : null}

          <div>
            <label className="field-label" htmlFor="key-name">
              Key name
            </label>
            <input
              id="key-name"
              className="select-field"
              type="text"
              maxLength={80}
              value={form.name}
              onChange={(event) => setFormValue({ name: event.target.value })}
            />
          </div>

          <section
            className="key-setting-section"
            aria-labelledby="key-models-title"
          >
            <div className="key-setting-heading">
              <div>
                <h3 id="key-models-title">Model access</h3>
                <p>
                  {form.allowedModelIds.length
                    ? `${form.allowedModelIds.length} selected`
                    : "All models"}
                </p>
              </div>
            </div>
            <div className="key-model-list">
              <label className="key-option">
                <input
                  type="checkbox"
                  checked={form.allowedModelIds.length === 0}
                  onChange={() => setFormValue({ allowedModelIds: [] })}
                />
                <span>All models</span>
              </label>
              {models.map((model) => (
                <label className="key-option" key={model.id}>
                  <input
                    type="checkbox"
                    checked={form.allowedModelIds.includes(model.id)}
                    onChange={() => toggleModel(model.id)}
                  />
                  <span className="key-option-copy">
                    <span>{model.name}</span>
                    <small>
                      {model.providerFamily} · {model.tier}
                    </small>
                  </span>
                </label>
              ))}
            </div>
          </section>

          <section
            className="key-setting-section"
            aria-labelledby="key-credit-title"
          >
            <div className="key-setting-heading">
              <div>
                <h3 id="key-credit-title">Credit spending limit</h3>
                <p>Disable this key automatically at the limit.</p>
              </div>
              <label className="key-toggle">
                <input
                  type="checkbox"
                  checked={form.unlimitedCredits}
                  onChange={(event) =>
                    setFormValue({ unlimitedCredits: event.target.checked })
                  }
                />
                <span>No limit</span>
              </label>
            </div>
            {!form.unlimitedCredits ? (
              <label className="key-inline-input" htmlFor="key-credit-limit">
                <span>$</span>
                <input
                  id="key-credit-limit"
                  className="select-field"
                  type="number"
                  min="0.01"
                  step="0.01"
                  placeholder="Limit in USD"
                  value={form.creditLimitUsd}
                  onChange={(event) =>
                    setFormValue({ creditLimitUsd: event.target.value })
                  }
                />
              </label>
            ) : null}
          </section>

          <section
            className="key-setting-section"
            aria-labelledby="key-expiration-title"
          >
            <div className="key-setting-heading">
              <div>
                <h3 id="key-expiration-title">Expiration</h3>
                <p>
                  {form.expirationMode === "never"
                    ? "Never expires"
                    : "Key stops working automatically"}
                </p>
              </div>
            </div>
            <div
              className="key-expiration-options"
              role="radiogroup"
              aria-label="Expiration"
            >
              {(
                [
                  ["never", "Never"],
                  ["hour", "1 hour"],
                  ["day", "1 day"],
                  ["month", "1 month"],
                  ["custom", "Custom"],
                ] as const
              ).map(([value, label]) => (
                <button
                  key={value}
                  type="button"
                  role="radio"
                  aria-checked={form.expirationMode === value}
                  className={form.expirationMode === value ? "is-selected" : ""}
                  onClick={() => setFormValue({ expirationMode: value })}
                >
                  {label}
                </button>
              ))}
            </div>
            {form.expirationMode === "custom" ? (
              <input
                className="select-field"
                type="datetime-local"
                min={localDateTimeInputValue(new Date(Date.now() + 60_000))}
                value={form.customExpiresAt}
                onChange={(event) =>
                  setFormValue({ customExpiresAt: event.target.value })
                }
              />
            ) : null}
          </section>

          <div>
            {keys.length ? (
              keys.map((key) => (
                <div className="key-row" key={key.prefix}>
                  <div className="min-w-0">
                    <p className="key-prefix">{key.name}</p>
                    <p className="key-meta">
                      {key.prefix} · {formatDate(key.createdAt)}
                    </p>
                    <p className="key-meta">
                      {key.allowedModelIds.length
                        ? `${key.allowedModelIds.length} models`
                        : "All models"}{" "}
                      ·{" "}
                      {key.creditLimitUsd === null
                        ? "No limit"
                        : `$${key.creditUsedUsd.toFixed(2)} / $${key.creditLimitUsd.toFixed(2)}`}{" "}
                      ·{" "}
                      {key.expiresAt
                        ? `Until ${formatDate(key.expiresAt)}`
                        : "Never expires"}
                    </p>
                  </div>
                  <div className="key-actions">
                    <Button
                      size="icon"
                      variant="quiet"
                      title="Renew key"
                      aria-label={`Renew ${key.prefix}`}
                      disabled={regenerateMutation.isPending}
                      onClick={() =>
                        void regenerateMutation
                          .mutateAsync(key.prefix)
                          .catch((error) =>
                            onNotice({
                              tone: "error",
                              message: messageFrom(error),
                            }),
                          )
                      }
                    >
                      <RotateCw size={15} />
                    </Button>
                    <Button
                      size="icon"
                      variant="quiet"
                      title="Revoke key"
                      aria-label={`Revoke ${key.prefix}`}
                      disabled={deleteMutation.isPending}
                      onClick={() =>
                        void deleteMutation
                          .mutateAsync(key.prefix)
                          .catch((error) =>
                            onNotice({
                              tone: "error",
                              message: messageFrom(error),
                            }),
                          )
                      }
                    >
                      <Trash2 size={15} />
                    </Button>
                  </div>
                </div>
              ))
            ) : (
              <p className="muted-empty">No active keys.</p>
            )}
          </div>
        </div>
        <DialogFooter>
          <Button
            variant="primary"
            disabled={createMutation.isPending || !canCreate}
            onClick={() =>
              void createMutation
                .mutateAsync(options)
                .catch((error) =>
                  onNotice({ tone: "error", message: messageFrom(error) }),
                )
            }
          >
            {createMutation.isPending ? (
              <LoaderCircle className="animate-spin" size={15} />
            ) : (
              <KeyRound size={15} />
            )}
            Create key
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function compatibleModelsForAgent(
  agentId: AgentId,
  allowedTiers: readonly string[],
): CompatibleModel[] {
  return compatibleModels.filter(
    (model) =>
      model.agents.includes(agentId) && allowedTiers.includes(model.tier),
  );
}

function AgentConfigurationDialog({
  agent,
  allowedTiers,
  open,
  onOpenChange,
  onConfigured,
  onNotice,
}: {
  agent: AgentDetection | null;
  allowedTiers: string[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfigured: () => Promise<void>;
  onNotice: (notice: Notice) => void;
}) {
  const models = useMemo(() => {
    if (!agent) return [];
    return compatibleModelsForAgent(agent.id, allowedTiers);
  }, [agent, allowedTiers]);
  const [modelId, setModelId] = useState("");
  const [apiKey, setApiKey] = useState<string | null>(null);
  const [result, setResult] = useState<string | null>(null);

  useEffect(() => {
    setModelId(models[0]?.id ?? "");
    setApiKey(null);
    setResult(null);
  }, [agent?.id, models]);

  const selectedModel =
    models.find((model) => model.id === modelId) ?? models[0];

  const keyMutation = useMutation({
    mutationFn: createApiKey,
    onSuccess: (key) => setApiKey(key.fullKey),
  });

  const configureMutation = useMutation({
    mutationFn: async () => {
      if (!agent || !selectedModel || !apiKey) {
        throw new Error(
          "Choose a compatible model and create an API key first.",
        );
      }
      return configureAgent({
        agentId: agent.id,
        endpoint: gatewayUrl,
        apiKey,
        model: selectedModel.id,
      });
    },
    onSuccess: async (configured) => {
      setResult(configured.message);
      await onConfigured();
    },
  });

  if (!agent) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <div className="flex items-center gap-3">
            <AgentIcon agent={agent} />
            <div>
              <h2 className="dialog-title">Configure {agent.name}</h2>
              <p className="dialog-subtitle">
                A backup is created before the configuration is changed.
              </p>
            </div>
          </div>
        </DialogHeader>
        <div className="dialog-body">
          <div>
            <label className="field-label" htmlFor="agent-model">
              Model
            </label>
            <select
              id="agent-model"
              className="select-field"
              value={modelId}
              onChange={(event) => setModelId(event.target.value)}
              disabled={!models.length || configureMutation.isSuccess}
            >
              {models.map((model) => (
                <option key={model.id} value={model.id}>
                  {model.label}
                </option>
              ))}
            </select>
          </div>

          {!models.length ? (
            <p className="error-copy">
              No compatible models are available on this plan.
            </p>
          ) : null}
          {result ? (
            <div className="secret-reveal text-sm text-[#cabaff]">{result}</div>
          ) : null}
          {configureMutation.error ? (
            <p className="error-copy">{messageFrom(configureMutation.error)}</p>
          ) : null}
        </div>
        <DialogFooter>
          {apiKey ? (
            <span className="mr-auto text-xs text-[#ad95ff]">
              API key ready
            </span>
          ) : null}
          {!apiKey ? (
            <Button
              variant="secondary"
              disabled={!selectedModel || keyMutation.isPending}
              onClick={() =>
                selectedModel &&
                void keyMutation
                  .mutateAsync({
                    name: `${agent.name} - ${selectedModel.label}`,
                    allowedModelIds: [selectedModel.id],
                    creditLimitUsd: null,
                    expiresAt: null,
                  })
                  .catch((error) =>
                    onNotice({ tone: "error", message: messageFrom(error) }),
                  )
              }
            >
              {keyMutation.isPending ? (
                <LoaderCircle className="animate-spin" size={15} />
              ) : (
                <KeyRound size={15} />
              )}
              Create key
            </Button>
          ) : (
            <Button
              variant="primary"
              disabled={
                !selectedModel || configureMutation.isPending || Boolean(result)
              }
              onClick={() =>
                void configureMutation.mutateAsync().catch(() => undefined)
              }
            >
              {configureMutation.isPending ? (
                <LoaderCircle className="animate-spin" size={15} />
              ) : (
                <Settings2 size={15} />
              )}
              Configure
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function FreePlan({ onSignOut }: { onSignOut: () => Promise<void> }) {
  return (
    <main className="signin-shell">
      <section className="signin-panel" aria-labelledby="plan-title">
        <img className="signin-logo" src="/logo-white.svg" alt="Accly" />
        <h1 id="plan-title">A paid Accly plan is required.</h1>
        <p>Manage your plan from the Accly web app, then return here.</p>
        <div className="signin-actions">
          <Button variant="secondary" onClick={() => void onSignOut()}>
            <LogOut size={15} /> Sign out
          </Button>
        </div>
      </section>
    </main>
  );
}

function Launcher() {
  const queryClient = useQueryClient();
  const [deviceCode, setDeviceCode] = useState<PendingDeviceCode | null>(null);
  const [loginError, setLoginError] = useState<string | null>(null);
  const [startingLogin, setStartingLogin] = useState(false);
  const loginInFlightRef = useRef(false);
  const [keysOpen, setKeysOpen] = useState(false);
  const [configuringAgent, setConfiguringAgent] =
    useState<AgentDetection | null>(null);
  const [agentLifecycleProgress, setAgentLifecycleProgress] =
    useState<AgentLifecycleProgress | null>(null);
  const [notice, setNotice] = useState<Notice>(null);

  const sessionQuery = useQuery({
    queryKey: ["launcher-session"],
    queryFn: getLauncherSession,
    staleTime: Infinity,
    retry: false,
  });
  const signedIn = Boolean(sessionQuery.data);

  const accountQuery = useQuery({
    queryKey: ["account"],
    queryFn: getAccountSnapshot,
    enabled: signedIn,
    staleTime: ACCOUNT_REFRESH_INTERVAL_MS,
    refetchInterval: ACCOUNT_REFRESH_INTERVAL_MS,
    refetchIntervalInBackground: false,
  });
  const agentsQuery = useQuery({
    queryKey: ["agents"],
    queryFn: listAgents,
    enabled: signedIn,
    staleTime: AGENT_SCAN_INTERVAL_MS,
    refetchInterval: AGENT_SCAN_INTERVAL_MS,
    refetchOnReconnect: false,
    refetchIntervalInBackground: false,
  });
  const updateQuery = useQuery({
    queryKey: ["update"],
    queryFn: checkForUpdate,
    enabled: signedIn,
    staleTime: UPDATE_CHECK_INTERVAL_MS,
    refetchInterval: UPDATE_CHECK_INTERVAL_MS,
    refetchIntervalInBackground: false,
  });

  const agentLifecycleMutation = useMutation({
    mutationFn: async ({
      agentId,
      action,
    }: {
      agentId: AgentDetection["id"];
      action: "install" | "update" | "repair";
    }) => {
      if (action === "install") return installAgent(agentId);
      if (action === "repair") return repairAgent(agentId);
      return updateAgent(agentId);
    },
    onMutate: (variables) => {
      setAgentLifecycleProgress({
        agentId: variables.agentId,
        action: variables.action,
        phase: "checking",
        message: "Checking local installations.",
      });
    },
    onSuccess: async (result) => {
      await queryClient.invalidateQueries({ queryKey: ["agents"] });
      setNotice({ tone: "success", message: result.message });
    },
  });

  const startLogin = useCallback(async () => {
    if (loginInFlightRef.current) return;

    loginInFlightRef.current = true;
    setStartingLogin(true);
    setLoginError(null);
    try {
      const code = await beginDeviceAuthorization();
      setDeviceCode({
        ...code,
        expiresAt: Date.now() + Math.max(code.expiresIn, 1) * 1_000,
      });
      await openExternal(code.verificationUriComplete ?? code.verificationUri);
    } catch (error) {
      setDeviceCode(null);
      setLoginError(messageFrom(error));
    } finally {
      loginInFlightRef.current = false;
      setStartingLogin(false);
    }
  }, []);

  const restartLogin = useCallback(() => {
    setDeviceCode(null);
    setLoginError(null);
  }, []);

  useEffect(() => {
    if (!deviceCode) return;

    let cancelled = false;
    let pollTimeout: number | undefined;
    const expireDeviceCode = () => {
      if (cancelled) return;
      setDeviceCode(null);
      setLoginError(DEVICE_CODE_EXPIRED_MESSAGE);
    };
    const expiryTimeout = window.setTimeout(
      expireDeviceCode,
      Math.max(0, deviceCode.expiresAt - Date.now()),
    );
    const poll = async () => {
      if (Date.now() >= deviceCode.expiresAt) {
        expireDeviceCode();
        return;
      }

      try {
        const result = await completeDeviceAuthorization(deviceCode);
        if (result.status === "completed") {
          if (!cancelled) {
            setDeviceCode(null);
            await queryClient.invalidateQueries({
              queryKey: ["launcher-session"],
            });
          }
          return;
        }

        if (result.status === "expired") {
          expireDeviceCode();
          return;
        }

        if (!cancelled) {
          const remainingMilliseconds = Math.max(
            1,
            deviceCode.expiresAt - Date.now(),
          );
          pollTimeout = window.setTimeout(
            poll,
            Math.min(
              Math.max(result.retryAfterSeconds, 2) * 1_000,
              remainingMilliseconds,
            ),
          );
        }
      } catch (error) {
        if (!cancelled) {
          setDeviceCode(null);
          setLoginError(messageFrom(error));
        }
        return;
      }
    };

    void poll();
    return () => {
      cancelled = true;
      window.clearTimeout(expiryTimeout);
      if (pollTimeout) window.clearTimeout(pollTimeout);
    };
  }, [deviceCode, queryClient]);

  useEffect(() => {
    const expiresAt = Date.parse(sessionQuery.data?.expiresAt ?? "");
    if (!Number.isFinite(expiresAt)) return;

    const timeout = window.setTimeout(
      () => {
        setLoginError(SESSION_EXPIRED_MESSAGE);
        void sessionQuery.refetch();
      },
      Math.max(0, expiresAt - Date.now()),
    );

    return () => window.clearTimeout(timeout);
  }, [sessionQuery.data?.expiresAt, sessionQuery.refetch]);

  const accountSessionExpired =
    accountQuery.error &&
    messageFrom(accountQuery.error) === SESSION_EXPIRED_MESSAGE;

  useEffect(() => {
    if (!accountSessionExpired) return;

    setLoginError(SESSION_EXPIRED_MESSAGE);
    void sessionQuery.refetch();
  }, [accountSessionExpired, sessionQuery.refetch]);

  useEffect(() => {
    if (!notice) return;
    const timeout = setTimeout(() => setNotice(null), 3500);
    return () => clearTimeout(timeout);
  }, [notice]);

  useEffect(() => {
    let disposed = false;
    let unlisten = () => {};

    void listenForAgentLifecycleProgress((progress) => {
      if (!disposed) setAgentLifecycleProgress(progress);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    });

    return () => {
      disposed = true;
      unlisten();
    };
  }, []);

  const handleSignOut = async () => {
    await signOut();
    setDeviceCode(null);
    queryClient.clear();
    await queryClient.invalidateQueries({ queryKey: ["launcher-session"] });
  };

  if (sessionQuery.isPending) {
    return <SessionBootstrap />;
  }

  if (sessionQuery.error) {
    return (
      <SessionReadError
        error={sessionQuery.error}
        onRetry={() => void sessionQuery.refetch()}
      />
    );
  }

  if (!signedIn) {
    return (
      <SignInScreen
        deviceCode={deviceCode}
        deviceCodeExpiresAt={deviceCode?.expiresAt ?? null}
        error={loginError}
        starting={startingLogin}
        onStart={startLogin}
        onStartOver={restartLogin}
      />
    );
  }

  if (accountQuery.isPending || agentsQuery.isPending) {
    return (
      <main className="signin-shell" aria-label="Loading your account">
        <LoaderCircle className="animate-spin text-[#8e6cff]" size={22} />
      </main>
    );
  }

  if (accountSessionExpired) {
    return (
      <SignInScreen
        deviceCode={deviceCode}
        deviceCodeExpiresAt={deviceCode?.expiresAt ?? null}
        error={SESSION_EXPIRED_MESSAGE}
        starting={startingLogin}
        onStart={startLogin}
        onStartOver={restartLogin}
      />
    );
  }

  if (accountQuery.error) {
    return (
      <main className="signin-shell">
        <section className="signin-panel" aria-labelledby="service-title">
          <img className="signin-logo" src="/logo-white.svg" alt="Accly" />
          <h1 id="service-title">Account connection unavailable.</h1>
          <p className="error-copy">{messageFrom(accountQuery.error)}</p>
          <div className="signin-actions">
            <Button
              variant="secondary"
              onClick={() => void accountQuery.refetch()}
            >
              <RefreshCw size={15} /> Retry
            </Button>
            <Button variant="quiet" onClick={() => void handleSignOut()}>
              Sign out
            </Button>
          </div>
        </section>
      </main>
    );
  }

  const account = accountQuery.data;
  if (!account || !account.plan.paid || account.plan.suspended) {
    return <FreePlan onSignOut={handleSignOut} />;
  }

  const agents = agentsQuery.data ?? [];
  const configuredCount = agents.filter(
    (agent) => agent.state === "ready",
  ).length;
  const detectedCount = agents.filter((agent) => agent.installed).length;
  const usageWidth = Math.min(100, Math.max(0, account.usage.percentUsed));

  return (
    <div className="app-shell">
      <main className="workspace">
        <div className="workspace-inner">
          <header className="topbar">
            <div className="workspace-brand">
              <img
                className="workspace-brand-mark"
                src="/accly-icon.svg"
                alt=""
              />
              <span>Accly Launcher</span>
            </div>
            <div className="topbar-actions">
              {updateQuery.data?.available && updateQuery.data.url ? (
                <Button
                  size="compact"
                  variant="secondary"
                  onClick={() => void openExternal(updateQuery.data!.url!)}
                >
                  Update {updateQuery.data.version}
                </Button>
              ) : null}
              <Button
                size="icon"
                variant="quiet"
                title="Manage API keys"
                aria-label="Manage API keys"
                onClick={() => setKeysOpen(true)}
              >
                <KeyRound size={17} />
              </Button>
              <Button
                size="icon"
                variant="quiet"
                title="Sign out"
                aria-label="Sign out"
                onClick={() => void handleSignOut()}
              >
                <LogOut size={17} />
              </Button>
            </div>
          </header>

          <section className="page-heading" aria-labelledby="ready-title">
            <div>
              <h1 id="ready-title">Your agents, ready for Accly.</h1>
              <p>
                Choose an installed tool and connect it with a compatible model.
              </p>
            </div>
            <span className="status-pill">
              <BadgeCheck size={13} /> {configuredCount} connected
            </span>
          </section>

          <section className="account-band" aria-label="Account status">
            <div>
              <p className="account-label">Plan</p>
              <p className="plan-name">
                <BadgeCheck size={19} className="text-[#8e6cff]" />{" "}
                {account.plan.planName}
              </p>
              <p className="plan-detail">
                {account.plan.allowedTiers.join(" · ")}
              </p>
            </div>
            <div>
              <div className="usage-topline">
                <p className="account-label">Today</p>
                <span className="usage-value">
                  {formatUsage(account.usage.remaining)} left
                </span>
              </div>
              <div
                className="usage-bar"
                aria-label={`${account.usage.percentUsed}% of daily allowance used`}
              >
                <div
                  className="usage-fill"
                  style={{ width: `${usageWidth}%` }}
                />
              </div>
              <p className="usage-detail">
                {formatUsage(account.usage.used)} of{" "}
                {formatUsage(account.usage.daily)} request units
              </p>
            </div>
          </section>

          <section className="agent-section" aria-labelledby="agents-title">
            <div className="section-heading">
              <div>
                <h2 id="agents-title">Agents</h2>
                <p>
                  {configuredCount} connected, {detectedCount} detected
                </p>
              </div>
              <Button
                size="compact"
                variant="quiet"
                disabled={agentLifecycleMutation.isPending}
                onClick={() => void agentsQuery.refetch()}
              >
                <RefreshCw size={14} /> Scan again
              </Button>
            </div>
            <div className="agent-list">
              {agents.map((agent) => (
                <article className="agent-row" key={agent.id}>
                  <div className="agent-ident">
                    <AgentIcon agent={agent} />
                    <div className="min-w-0">
                      <h3 className="agent-title">{agent.name}</h3>
                      <p className="agent-path">
                        {agent.configPath ?? "No configuration target"}
                      </p>
                      {agent.version || agent.installSource ? (
                        <p className="agent-meta">
                          {agent.version
                            ? `Version ${agent.version}`
                            : "Installed"}
                          {agent.installSource
                            ? ` · ${agent.installSource}`
                            : ""}
                          {agent.installationCount > 1
                            ? ` · ${agent.installationCount} installs found`
                            : ""}
                        </p>
                      ) : null}
                    </div>
                  </div>
                  <div className="agent-state" data-state={agent.state}>
                    <AgentStateIcon state={agent.state} />
                    <span>{agent.detail}</span>
                  </div>
                  <div
                    className="agent-action"
                    aria-live={
                      agentLifecycleMutation.isPending &&
                      agentLifecycleMutation.variables?.agentId === agent.id
                        ? "polite"
                        : "off"
                    }
                  >
                    {agentLifecycleMutation.isPending &&
                    agentLifecycleMutation.variables?.agentId === agent.id ? (
                      <div className="agent-action-progress">
                        <Button
                          size="compact"
                          variant="secondary"
                          disabled
                          title={agentLifecycleProgress?.message}
                        >
                          <LoaderCircle className="animate-spin" size={15} />
                          {agentLifecycleProgress?.phase === "checking"
                            ? "Checking"
                            : agentLifecycleMutation.variables.action ===
                                "install"
                              ? "Installing"
                              : agentLifecycleMutation.variables.action ===
                                  "repair"
                                ? "Repairing"
                                : "Updating"}
                        </Button>
                        <p className="agent-progress-copy">
                          {agentLifecycleProgress?.message ?? "Working..."}
                        </p>
                      </div>
                    ) : agent.canInstall ? (
                      <Button
                        size="compact"
                        variant="primary"
                        disabled={agentLifecycleMutation.isPending}
                        onClick={() =>
                          void agentLifecycleMutation
                            .mutateAsync({
                              agentId: agent.id,
                              action: "install",
                            })
                            .catch((error) =>
                              setNotice({
                                tone: "error",
                                message: messageFrom(error),
                              }),
                            )
                        }
                      >
                        <Download size={15} /> Install
                      </Button>
                    ) : agent.canRepair ? (
                      <Button
                        size="compact"
                        variant="primary"
                        disabled={agentLifecycleMutation.isPending}
                        onClick={() =>
                          void agentLifecycleMutation
                            .mutateAsync({
                              agentId: agent.id,
                              action: "repair",
                            })
                            .catch((error) =>
                              setNotice({
                                tone: "error",
                                message: messageFrom(error),
                              }),
                            )
                        }
                      >
                        <RotateCw size={15} /> Repair
                      </Button>
                    ) : agent.installed && agent.configurable ? (
                      <div className="agent-action-buttons">
                        {agent.canUpdate ? (
                          <Button
                            size="icon"
                            variant="quiet"
                            disabled={agentLifecycleMutation.isPending}
                            title={`Update ${agent.name}`}
                            aria-label={`Update ${agent.name}`}
                            onClick={() =>
                              void agentLifecycleMutation
                                .mutateAsync({
                                  agentId: agent.id,
                                  action: "update",
                                })
                                .catch((error) =>
                                  setNotice({
                                    tone: "error",
                                    message: messageFrom(error),
                                  }),
                                )
                            }
                          >
                            <RotateCw size={15} />
                          </Button>
                        ) : null}
                        <Button
                          size="compact"
                          variant={
                            agent.state === "ready" ? "secondary" : "primary"
                          }
                          disabled={agentLifecycleMutation.isPending}
                          onClick={() => setConfiguringAgent(agent)}
                        >
                          {agent.state === "ready" ? "Change" : "Configure"}
                        </Button>
                      </div>
                    ) : (
                      <span className="text-xs text-[#6f746c]">
                        {agent.installed ? "Unavailable" : "Not installed"}
                      </span>
                    )}
                  </div>
                </article>
              ))}
            </div>
          </section>
        </div>
      </main>

      <KeyDialog
        open={keysOpen}
        keys={account.keys}
        onOpenChange={setKeysOpen}
        onChanged={() =>
          queryClient.invalidateQueries({ queryKey: ["account"] })
        }
        onNotice={setNotice}
      />
      <AgentConfigurationDialog
        agent={configuringAgent}
        allowedTiers={account.plan.allowedTiers}
        open={Boolean(configuringAgent)}
        onOpenChange={(open) => {
          if (!open) setConfiguringAgent(null);
        }}
        onConfigured={async () => {
          await queryClient.invalidateQueries({ queryKey: ["agents"] });
          await queryClient.invalidateQueries({ queryKey: ["account"] });
          setNotice({
            tone: "success",
            message: "Configuration validated and backed up.",
          });
        }}
        onNotice={setNotice}
      />
      {notice ? (
        <div className="notice" role="status">
          {notice.tone === "success" ? (
            <Check size={16} className="text-[#5fd18c]" />
          ) : (
            <AlertCircle size={16} className="text-[#efad98]" />
          )}
          {notice.message}
        </div>
      ) : null}
    </div>
  );
}

export default function App() {
  return <Launcher />;
}
