import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App, { compatibleModelsForAgent, messageFrom } from "./App";

const nativeMocks = vi.hoisted(() => ({
  getLauncherSession: vi.fn(),
  beginDeviceAuthorization: vi.fn(),
  completeDeviceAuthorization: vi.fn(),
  installAgent: vi.fn(),
  listenForAgentLifecycleProgress: vi.fn(),
}));

vi.mock("./lib/native", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./lib/native")>();
  return {
    ...actual,
    getLauncherSession: nativeMocks.getLauncherSession,
    beginDeviceAuthorization: nativeMocks.beginDeviceAuthorization,
    completeDeviceAuthorization: nativeMocks.completeDeviceAuthorization,
    installAgent: nativeMocks.installAgent,
    listenForAgentLifecycleProgress:
      nativeMocks.listenForAgentLifecycleProgress,
  };
});

const deviceCode = {
  deviceCode: "device-code",
  userCode: "ACCLY-DEV",
  verificationUri: "https://auth.accly.net/device",
  verificationUriComplete: "https://auth.accly.net/device?user_code=ACCLY-DEV",
  expiresIn: 900,
  interval: 5,
};

describe("Accly Launcher", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    vi.spyOn(window, "open").mockImplementation(() => null);
    nativeMocks.getLauncherSession.mockReset().mockResolvedValue(null);
    nativeMocks.beginDeviceAuthorization
      .mockReset()
      .mockResolvedValue(deviceCode);
    nativeMocks.completeDeviceAuthorization.mockReset().mockResolvedValue({
      status: "completed",
      session: { expiresAt: null },
    });
    nativeMocks.installAgent.mockReset();
    nativeMocks.listenForAgentLifecycleProgress
      .mockReset()
      .mockResolvedValue(() => {});
  });

  it("moves from device login to the agent configuration workspace", async () => {
    nativeMocks.getLauncherSession
      .mockResolvedValueOnce(null)
      .mockResolvedValue({ expiresAt: null });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Continue" }));

    expect(
      await screen.findByRole("heading", {
        name: "Your agents, ready for Accly.",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Configure" }),
    ).toBeInTheDocument();
  });

  it("preserves native command errors for the sign-in screen", () => {
    expect(messageFrom("Device authorization is unavailable.")).toBe(
      "Device authorization is unavailable.",
    );
  });

  it("shows a secure-session state while Keychain access is pending", async () => {
    let resolveSession: ((value: null) => void) | undefined;
    nativeMocks.getLauncherSession.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveSession = resolve;
        }),
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>,
    );

    expect(
      await screen.findByText("Checking secure session."),
    ).toBeInTheDocument();
    await act(async () => {
      resolveSession!(null);
    });
    expect(
      await screen.findByRole("button", { name: "Continue" }),
    ).toBeInTheDocument();
  });

  it("lets the user retry a Keychain session read failure", async () => {
    nativeMocks.getLauncherSession
      .mockRejectedValueOnce(new Error("Keychain access was denied."))
      .mockResolvedValueOnce(null);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>,
    );

    expect(
      await screen.findByRole("heading", {
        name: "Secure session unavailable.",
      }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(
      await screen.findByRole("button", { name: "Continue" }),
    ).toBeInTheDocument();
    expect(nativeMocks.getLauncherSession).toHaveBeenCalledTimes(2);
  });

  it("offers Gateway models for the Max plan tiers", () => {
    expect(
      compatibleModelsForAgent("codex", [
        "basic",
        "advanced",
        "thinking",
        "beta",
      ]).map((model) => model.id),
    ).toEqual(["gpt-5-4-mini", "gpt-5-4", "gpt-5-4-pro"]);
    expect(
      compatibleModelsForAgent("claude-code", ["advanced", "thinking"]).map(
        (model) => model.id,
      ),
    ).toEqual(["claude-sonnet-4-6", "claude-sonnet-4-6-thinking"]);
    expect(
      compatibleModelsForAgent("gemini-cli", ["basic", "advanced"]).map(
        (model) => model.id,
      ),
    ).toEqual([
      "gemini-3.1-flash-lite",
      "gemini-3.5-flash",
      "gemini-3.1-pro-preview",
    ]);
  });

  it("keeps polling after a nonterminal device authorization response", async () => {
    nativeMocks.completeDeviceAuthorization.mockResolvedValue({
      status: "pending",
      retryAfterSeconds: 60,
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Continue" }));

    await waitFor(() => {
      expect(nativeMocks.completeDeviceAuthorization).toHaveBeenCalledWith(
        expect.objectContaining(deviceCode),
      );
    });
    expect(screen.getByText("Verification code")).toBeInTheDocument();
    expect(screen.queryByText("Authorization pending")).not.toBeInTheDocument();
  });

  it("starts only one device authorization when Continue is clicked repeatedly", async () => {
    let resolveDeviceCode: (value: typeof deviceCode) => void;
    nativeMocks.beginDeviceAuthorization.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveDeviceCode = resolve;
        }),
    );
    nativeMocks.completeDeviceAuthorization.mockResolvedValue({
      status: "pending",
      retryAfterSeconds: 60,
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>,
    );

    const continueButton = await screen.findByRole("button", {
      name: "Continue",
    });
    fireEvent.click(continueButton);
    fireEvent.click(continueButton);

    expect(nativeMocks.beginDeviceAuthorization).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveDeviceCode!(deviceCode);
    });

    expect(await screen.findByText("Verification code")).toBeInTheDocument();
  });

  it("shows native agent lifecycle progress while installation is running", async () => {
    let emitProgress:
      | ((progress: {
          agentId: "gemini-cli";
          action: "install";
          phase: "installing";
          message: string;
        }) => void)
      | undefined;
    let resolveInstall: ((value: unknown) => void) | undefined;
    nativeMocks.getLauncherSession.mockResolvedValue({ expiresAt: null });
    nativeMocks.listenForAgentLifecycleProgress.mockImplementation(
      async (handler) => {
        emitProgress = handler;
        return () => {};
      },
    );
    nativeMocks.installAgent.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveInstall = resolve;
        }),
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>,
    );

    await waitFor(() => {
      expect(nativeMocks.listenForAgentLifecycleProgress).toHaveBeenCalledTimes(
        1,
      );
    });
    fireEvent.click(
      (await screen.findAllByRole("button", { name: "Install" }))[0],
    );

    await waitFor(() => {
      expect(nativeMocks.installAgent).toHaveBeenCalledWith("gemini-cli");
    });
    act(() => {
      emitProgress?.({
        agentId: "gemini-cli",
        action: "install",
        phase: "installing",
        message: "Installing the supported package.",
      });
    });

    expect(
      await screen.findByText("Installing the supported package."),
    ).toBeInTheDocument();
    await act(async () => {
      resolveInstall?.({
        agent: {},
        action: "install",
        message: "Gemini CLI was installed. Scan results were refreshed.",
      });
    });
  });

  it("lets the user discard a pending device code and start over", async () => {
    nativeMocks.completeDeviceAuthorization.mockResolvedValue({
      status: "pending",
      retryAfterSeconds: 60,
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Continue" }));
    expect(await screen.findByText("Verification code")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Start over" }));

    expect(
      await screen.findByRole("button", { name: "Continue" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Verification code")).not.toBeInTheDocument();
  });

  it("clears a terminal device authorization error so Continue is available again", async () => {
    nativeMocks.completeDeviceAuthorization.mockRejectedValue(
      new Error("Access denied"),
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Continue" }));

    expect(await screen.findByText("Access denied")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Continue" }),
    ).toBeInTheDocument();
  });

  it("offers a new code after Auth reports that the device code expired", async () => {
    nativeMocks.completeDeviceAuthorization.mockResolvedValue({
      status: "expired",
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Continue" }));

    expect(
      await screen.findByText(
        "This verification code expired. Generate a new code to continue.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Generate new code" }),
    ).toBeInTheDocument();
  });
});
