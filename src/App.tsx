import { useCallback, useEffect, useMemo, useState } from "react";
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
  getLauncherSession,
  listAgents,
  regenerateApiKey,
  signOut,
} from "./lib/native";
import type {
  AgentDetection,
  ApiKeyGroup,
  ApiKeyRecord,
  CompatibleModel,
  CreatedApiKey,
  DeviceCode,
} from "./lib/types";
import { cn, formatDate, formatUsage } from "./lib/utils";

type Notice = { tone: "success" | "error"; message: string } | null;

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
  error,
  starting,
  onStart,
}: {
  deviceCode: DeviceCode | null;
  error: string | null;
  starting: boolean;
  onStart: () => Promise<void>;
}) {
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
            </div>
            <div className="signin-actions">
              <Button variant="primary" onClick={openVerification}>
                Open browser <ArrowUpRight size={15} />
              </Button>
              <LoaderCircle
                className="animate-spin text-[#8e6cff]"
                size={18}
                aria-label="Waiting for approval"
              />
            </div>
          </>
        ) : (
          <div className="signin-actions">
            <Button
              variant="primary"
              onClick={() => void onStart()}
              disabled={starting}
            >
              {starting ? (
                <LoaderCircle className="animate-spin" size={15} />
              ) : null}
              Continue
            </Button>
          </div>
        )}

        {error ? <p className="error-copy mt-5">{error}</p> : null}
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
  const [group, setGroup] = useState<ApiKeyGroup>("universal");
  const [created, setCreated] = useState<CreatedApiKey | null>(null);

  const createMutation = useMutation({
    mutationFn: createApiKey,
    onSuccess: async (key) => {
      setCreated(key);
      await onChanged();
    },
  });

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
        if (!nextOpen) setCreated(null);
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
            <label className="field-label" htmlFor="key-group">
              Key access
            </label>
            <select
              id="key-group"
              className="select-field"
              value={group}
              onChange={(event) => setGroup(event.target.value as ApiKeyGroup)}
            >
              <option value="universal">Universal</option>
              <option value="openai">OpenAI-compatible</option>
              <option value="anthropic">Anthropic</option>
              <option value="google">Google</option>
            </select>
          </div>

          <div>
            {keys.length ? (
              keys.map((key) => (
                <div className="key-row" key={key.prefix}>
                  <div className="min-w-0">
                    <p className="key-prefix">{key.prefix}</p>
                    <p className="key-meta">
                      {key.groupType} · {formatDate(key.createdAt)}
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
            disabled={createMutation.isPending}
            onClick={() =>
              void createMutation
                .mutateAsync(group)
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

function keyGroupForModel(model: CompatibleModel): ApiKeyGroup {
  if (model.protocol === "anthropic") return "anthropic";
  if (model.protocol === "google") return "google";
  if (model.protocol === "openai" || model.protocol === "responses")
    return "openai";
  return "universal";
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
  const models = useMemo(
    () =>
      compatibleModels.filter(
        (model) =>
          model.agents.includes(agent?.id ?? "codex") &&
          allowedTiers.includes(model.tier),
      ),
    [agent?.id, allowedTiers],
  );
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
                  .mutateAsync(keyGroupForModel(selectedModel))
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
  const [deviceCode, setDeviceCode] = useState<DeviceCode | null>(null);
  const [loginError, setLoginError] = useState<string | null>(null);
  const [startingLogin, setStartingLogin] = useState(false);
  const [keysOpen, setKeysOpen] = useState(false);
  const [configuringAgent, setConfiguringAgent] =
    useState<AgentDetection | null>(null);
  const [notice, setNotice] = useState<Notice>(null);

  const sessionQuery = useQuery({
    queryKey: ["launcher-session"],
    queryFn: getLauncherSession,
    staleTime: Infinity,
  });
  const signedIn = Boolean(sessionQuery.data);

  const accountQuery = useQuery({
    queryKey: ["account"],
    queryFn: getAccountSnapshot,
    enabled: signedIn,
  });
  const agentsQuery = useQuery({
    queryKey: ["agents"],
    queryFn: listAgents,
    enabled: signedIn,
  });
  const updateQuery = useQuery({
    queryKey: ["update"],
    queryFn: checkForUpdate,
    enabled: signedIn,
    refetchInterval: 4 * 60 * 60 * 1000,
  });

  const startLogin = useCallback(async () => {
    setStartingLogin(true);
    setLoginError(null);
    try {
      const code = await beginDeviceAuthorization();
      await openExternal(code.verificationUriComplete ?? code.verificationUri);
      setDeviceCode(code);
    } catch (error) {
      setLoginError(messageFrom(error));
    } finally {
      setStartingLogin(false);
    }
  }, []);

  useEffect(() => {
    if (!deviceCode) return;

    let cancelled = false;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
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

        if (!cancelled) {
          timeout = setTimeout(
            poll,
            Math.max(result.retryAfterSeconds, 2) * 1000,
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
      if (timeout) clearTimeout(timeout);
    };
  }, [deviceCode, queryClient]);

  useEffect(() => {
    if (!notice) return;
    const timeout = setTimeout(() => setNotice(null), 3500);
    return () => clearTimeout(timeout);
  }, [notice]);

  const handleSignOut = async () => {
    await signOut();
    setDeviceCode(null);
    queryClient.clear();
    await queryClient.invalidateQueries({ queryKey: ["launcher-session"] });
  };

  if (sessionQuery.isPending) {
    return (
      <main className="signin-shell" aria-label="Loading Accly Launcher">
        <LoaderCircle className="animate-spin text-[#8e6cff]" size={22} />
      </main>
    );
  }

  if (!signedIn) {
    return (
      <SignInScreen
        deviceCode={deviceCode}
        error={loginError}
        starting={startingLogin}
        onStart={startLogin}
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
  const usageWidth = Math.min(100, Math.max(0, account.usage.percentUsed));

  return (
    <div className="app-shell">
      <aside className="rail">
        <div className="brand">
          <img className="brand-mark" src="/accly-icon.svg" alt="" />
          <span>Accly Launcher</span>
        </div>
        <nav className="step-list" aria-label="Setup progress">
          <span className="step-item is-done">Account</span>
          <span
            className={cn(
              "step-item",
              configuredCount ? "is-done" : "is-current",
            )}
          >
            Agents
          </span>
          <span
            className={cn("step-item", configuredCount ? "is-current" : "")}
          >
            Ready
          </span>
        </nav>
        <div className="rail-bottom">
          <p className="rail-note">macOS · English</p>
        </div>
      </aside>

      <main className="workspace">
        <div className="workspace-inner">
          <header className="topbar">
            <p className="eyebrow">Workspace</p>
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
                <h2 id="agents-title">Detected agents</h2>
                <p>
                  {configuredCount} of {agents.length} ready
                </p>
              </div>
              <Button
                size="compact"
                variant="quiet"
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
                    </div>
                  </div>
                  <div className="agent-state" data-state={agent.state}>
                    <AgentStateIcon state={agent.state} />
                    <span>{agent.detail}</span>
                  </div>
                  <div className="agent-action">
                    {agent.installed && agent.configurable ? (
                      <Button
                        size="compact"
                        variant={
                          agent.state === "ready" ? "secondary" : "primary"
                        }
                        onClick={() => setConfiguringAgent(agent)}
                      >
                        {agent.state === "ready" ? "Change" : "Configure"}
                      </Button>
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
