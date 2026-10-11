import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { api, type AppTarget } from "@/lib/api";
import { SetupView } from "./setup-view";

vi.mock("@/lib/api", async (original) => ({
  ...(await original<typeof import("@/lib/api")>()),
  api: vi.fn(),
}));

const desktop: AppTarget = { id: "claude-desktop", installed: true, added: false, running: false };

function show(command: string, apps: AppTarget[] = []) {
  const onChanged = vi.fn();
  render(<SetupView command={command} apps={apps} onChanged={onChanged} />);
  return { user: userEvent.setup(), onChanged };
}

afterEach(() => {
  cleanup();
  vi.mocked(api).mockReset();
});

test("every app is given the program under the name uniMCP", async () => {
  const { user } = show("C:\\apps\\unimcp.exe");

  expect(screen.getByText(/"uniMCP": \{/)).toBeTruthy();
  expect(screen.getByText("Settings > Developer > Edit Config, then restart.")).toBeTruthy();

  await user.click(screen.getByRole("tab", { name: "Claude Code" }));
  expect(screen.getByText('claude mcp add uniMCP -- "C:\\apps\\unimcp.exe" serve')).toBeTruthy();

  await user.click(screen.getByRole("tab", { name: "Codex" }));
  expect(screen.getByText('codex mcp add uniMCP -- "C:\\apps\\unimcp.exe" serve')).toBeTruthy();

  await user.click(screen.getByRole("tab", { name: "Gemini CLI" }));
  expect(screen.getByText(/"uniMCP": \{/)).toBeTruthy();
  expect(screen.getByText(/\.gemini folder/)).toBeTruthy();

  await user.click(screen.getByRole("tab", { name: "Other" }));
  expect(screen.getByText('"C:\\apps\\unimcp.exe" serve')).toBeTruthy();
});

test("an installed program is started by its name, without a path", async () => {
  const { user } = show("unimcp");

  expect(screen.getByText(/"command": "unimcp"/)).toBeTruthy();

  await user.click(screen.getByRole("tab", { name: "Claude Code" }));
  expect(screen.getByText("claude mcp add uniMCP -- unimcp serve")).toBeTruthy();

  await user.click(screen.getByRole("tab", { name: "Other" }));
  expect(screen.getByText("unimcp serve")).toBeTruthy();
});

test("an app found on this computer can be added with one click", async () => {
  vi.mocked(api).mockResolvedValue({ ok: true });
  const { user, onChanged } = show("unimcp", [desktop]);
  expect(screen.getByText("Not added")).toBeTruthy();

  await user.click(screen.getByRole("button", { name: "Add to Claude Desktop" }));

  expect(api).toHaveBeenCalledWith("/apps/add", { app: "claude-desktop" });
  expect(onChanged).toHaveBeenCalled();
  expect(screen.getByText("Restart Claude Desktop to apply.")).toBeTruthy();
  expect(screen.getByText(/"command": "unimcp"/)).toBeTruthy();
});

test("an app that is not on this computer only gets the instructions", () => {
  show("unimcp", [{ ...desktop, installed: false }]);
  expect(screen.queryByRole("button", { name: "Add to Claude Desktop" })).toBeNull();
  expect(screen.getByText(/"command": "unimcp"/)).toBeTruthy();
});

test("a running Claude Desktop has to be quit before it can be changed", () => {
  show("unimcp", [{ ...desktop, running: true }]);
  expect(screen.getByText("Quit Claude Desktop first")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Add to Claude Desktop" })).toHaveProperty(
    "disabled",
    true,
  );
});

test("an app that already has uniMCP says so", () => {
  show("unimcp", [{ ...desktop, added: true, running: true }]);
  expect(screen.getByText("Added")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Add to Claude Desktop" })).toHaveProperty(
    "disabled",
    true,
  );
});

test("a failed attempt shows why", async () => {
  vi.mocked(api).mockRejectedValue(new Error("quit Claude Desktop first"));
  const { user } = show("unimcp", [desktop]);

  await user.click(screen.getByRole("button", { name: "Add to Claude Desktop" }));

  expect((await screen.findByRole("alert")).textContent).toBe("quit Claude Desktop first");
});
