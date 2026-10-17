import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { serverApi } from "@/lib/server-api";
import { ConnectView } from "./connect-view";

vi.mock("@/lib/server-api", async (original) => ({
  ...(await original<typeof import("@/lib/server-api")>()),
  serverApi: vi.fn(),
}));

const claude = { id: "c1", name: "Claude", connected: 1_790_000_000, last_used: 1_790_000_000 };

afterEach(() => {
  cleanup();
  vi.mocked(serverApi).mockReset();
});

test("the page shows this server's MCP address and how each app connects to it", async () => {
  vi.mocked(serverApi).mockResolvedValue({ apps: [] });
  const user = userEvent.setup();
  render(<ConnectView />);

  const address = `${window.location.origin}/mcp`;
  expect(screen.getByText(address)).toBeTruthy();
  expect(screen.getByText(/Add custom connector/)).toBeTruthy();

  await user.click(screen.getByRole("tab", { name: "Claude Code" }));
  expect(screen.getByText(`claude mcp add --transport http uniMCP ${address}`)).toBeTruthy();
  expect(await screen.findByText("No app is connected to your account yet.")).toBeTruthy();
});

test("a connected app can be disconnected", async () => {
  vi.mocked(serverApi).mockImplementation(async (path) =>
    path === "/oauth/connected" ? { apps: [claude] } : { ok: true },
  );
  const user = userEvent.setup();
  render(<ConnectView />);

  expect(await screen.findByText("Claude", { selector: "td" })).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "Disconnect" }));
  expect(serverApi).toHaveBeenCalledWith("/oauth/disconnect", { id: "c1" });
});
