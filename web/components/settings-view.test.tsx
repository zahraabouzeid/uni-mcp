import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { api } from "@/lib/api";
import { SettingsView } from "./settings-view";

vi.mock("@/lib/api", async (original) => ({
  ...(await original<typeof import("@/lib/api")>()),
  api: vi.fn(),
}));

afterEach(() => {
  cleanup();
  vi.mocked(api).mockReset();
});

test("stored instructions are shown and count as saved", () => {
  render(<SettingsView saved="Answer in German." limit={2000} onSaved={() => {}} />);
  expect(screen.getByRole<HTMLTextAreaElement>("textbox").value).toBe("Answer in German.");
  expect(screen.getByRole("button", { name: "Save" })).toHaveProperty("disabled", true);
  expect(screen.getByText("17 / 2000")).toBeTruthy();
});

test("changed instructions are sent and the page is told to reload", async () => {
  vi.mocked(api).mockResolvedValue({ ok: true });
  const onSaved = vi.fn();
  const user = userEvent.setup();
  render(<SettingsView saved="" limit={2000} onSaved={onSaved} />);

  await user.type(screen.getByRole("textbox"), "Keep answers short.");
  await user.click(screen.getByRole("button", { name: "Save" }));

  expect(api).toHaveBeenCalledWith("/custom-instructions", {
    custom_instructions: "Keep answers short.",
  });
  expect(onSaved).toHaveBeenCalled();
  expect(screen.getByText("Saved. Takes effect after a restart.")).toBeTruthy();
});

test("instructions that could not be stored show why", async () => {
  vi.mocked(api).mockRejectedValue(
    new Error("custom instructions can be at most 2000 characters long"),
  );
  const user = userEvent.setup();
  render(<SettingsView saved="" limit={2000} onSaved={() => {}} />);

  await user.type(screen.getByRole("textbox"), "x");
  await user.click(screen.getByRole("button", { name: "Save" }));

  expect((await screen.findByRole("alert")).textContent).toContain("at most 2000");
});
