import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test } from "vitest";
import { SetupView } from "./setup-view";

afterEach(cleanup);

test("every app is given the program under the name unimcp", async () => {
  render(<SetupView executable={"C:\\apps\\unimcp.exe"} />);
  const user = userEvent.setup();

  expect(screen.getByText('claude mcp add unimcp -- "C:\\apps\\unimcp.exe" serve')).toBeTruthy();
  expect(screen.getByText(/"unimcp": \{/)).toBeTruthy();

  await user.click(screen.getByRole("tab", { name: "Codex" }));
  expect(screen.getByText('codex mcp add unimcp -- "C:\\apps\\unimcp.exe" serve')).toBeTruthy();

  await user.click(screen.getByRole("tab", { name: "Other" }));
  expect(screen.getByText('"C:\\apps\\unimcp.exe" serve')).toBeTruthy();
});
