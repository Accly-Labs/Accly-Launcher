import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App, { messageFrom } from "./App";

const nativeMocks = vi.hoisted(() => ({
  getLauncherSession: vi.fn(),
  beginDeviceAuthorization: vi.fn(),
  completeDeviceAuthorization: vi.fn(),
}));

vi.mock("./lib/native", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./lib/native")>();
  return {
    ...actual,
    getLauncherSession: nativeMocks.getLauncherSession,
    beginDeviceAuthorization: nativeMocks.beginDeviceAuthorization,
    completeDeviceAuthorization: nativeMocks.completeDeviceAuthorization,
  };
});

const deviceCode = {
  deviceCode: "device-code",
  userCode: "ACCLY-DEV",
  verificationUri: "https://auth.accly.net/device",
  verificationUriComplete: "https://auth.accly.net/device?user_code=ACCLY-DEV",
  expiresIn: 1800,
  interval: 5,
};

describe("Accly Launcher", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    vi.spyOn(window, "open").mockImplementation(() => null);
    nativeMocks.getLauncherSession.mockReset().mockResolvedValue(null);
    nativeMocks.beginDeviceAuthorization.mockResolvedValue(deviceCode);
    nativeMocks.completeDeviceAuthorization.mockResolvedValue({
      status: "completed",
      session: { expiresAt: null },
    });
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
        deviceCode,
      );
    });
    expect(screen.getByText("Verification code")).toBeInTheDocument();
    expect(screen.queryByText("Authorization pending")).not.toBeInTheDocument();
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
});
