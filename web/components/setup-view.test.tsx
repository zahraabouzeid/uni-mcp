import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test } from "vitest";
import { SetupView } from "./setup-view";

afterEach(cleanup);

test("every app is given the program under the name uniMCP", async () => {
  render(<SetupView command={"C:\\apps\\unimcp.exe"} />);
  const user = userEvent.setup();

  expect(screen.getByText(/"uniMCP": \{/)).toBeTruthy();
  expect(screen.getByText("Settings > Developer > Edit Config, then restart.")).toBeTruthy();

  await user.click(screen.getByRole("tab", { name: "Claude Code" }));
  expect(screen.getByText('claude mcp add uniMCP -- "C:\\apps\\unimcp.exe" serve')).toBeTruthy();

  await user.click(screen.getByRole("tab", { name: "Codex" }));
  expect(screen.getByText('codex mcp add uniMCP -- "C:\\apps\\unimcp.exe" serve')).toBeTruthy();

  await user.click(screen.getByRole("tab", { name: "Other" }));
  expect(screen.getByText('"C:\\apps\\unimcp.exe" serve')).toBeTruthy();
});

test("an installed program is started by its name, without a path", async () => {
  render(<SetupView command="unimcp" />);
  const user = userEvent.setup();

  expect(screen.getByText(/"command": "unimcp"/)).toBeTruthy();

  await user.click(screen.getByRole("tab", { name: "Claude Code" }));
  expect(screen.getByText("claude mcp add uniMCP -- unimcp serve")).toBeTruthy();

  await user.click(screen.getByRole("tab", { name: "Other" }));
  expect(screen.getByText("unimcp serve")).toBeTruthy();
});
