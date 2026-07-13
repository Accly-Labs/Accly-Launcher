import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

describe("Accly Launcher", () => {
  beforeEach(() => {
    vi.spyOn(window, "open").mockImplementation(() => null);
  });

  it("moves from device login to the agent configuration workspace", async () => {
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
});
