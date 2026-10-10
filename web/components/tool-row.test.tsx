import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { api, type Tool } from "@/lib/api";
import { ToolRow } from "./tool-row";

vi.mock("@/lib/api", async (original) => ({
  ...(await original<typeof import("@/lib/api")>()),
  api: vi.fn(),
}));

const tool: Tool = {
  name: "list_courses",
  description: "List the courses.",
  params: [],
  service: "Moodle",
  available: true,
};

function show(overrides: Partial<Tool> = {}) {
  render(
    <table>
      <tbody>
        <ToolRow tool={{ ...tool, ...overrides }} />
      </tbody>
    </table>,
  );
  return userEvent.setup();
}

afterEach(() => {
  cleanup();
  vi.mocked(api).mockReset();
});

test("a tool whose service is not connected cannot be tried", () => {
  show({ available: false });
  expect(screen.getByRole("button", { name: "Try" })).toHaveProperty("disabled", true);
});

test("a tool without parameters runs as soon as it is opened", async () => {
  vi.mocked(api).mockResolvedValue({ ok: true, output: '{"courses": []}' });
  const user = show();

  await user.click(screen.getByRole("button", { name: "Try" }));

  expect(api).toHaveBeenCalledWith("/tools/try", { name: "list_courses", arguments: {} });
  expect(await screen.findByText('{"courses": []}')).toBeTruthy();
});

test("an optional parameter that is left empty is not sent", async () => {
  vi.mocked(api).mockResolvedValue({ ok: true, output: "[]" });
  const user = show({
    name: "list_events",
    params: [
      { name: "from", description: "First day.", required: false },
      { name: "search", description: "Title text.", required: false },
    ],
  });

  await user.click(screen.getByRole("button", { name: "Try" }));
  expect(api).not.toHaveBeenCalled();

  await user.type(screen.getByLabelText("search (optional)"), "Mechanik");
  await user.click(screen.getByRole("button", { name: "Run" }));

  expect(api).toHaveBeenCalledWith("/tools/try", {
    name: "list_events",
    arguments: { search: "Mechanik" },
  });
});

test("a failing tool shows its message", async () => {
  vi.mocked(api).mockResolvedValue({ ok: false, output: "not logged in to moodle" });
  const user = show();

  await user.click(screen.getByRole("button", { name: "Try" }));

  expect((await screen.findByRole("alert")).textContent).toBe("not logged in to moodle");
});
