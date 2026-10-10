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
  vi.mocked(api).mockResolvedValue({ ok: true, output: '{"courses": []}', images: [] });
  const user = show();

  await user.click(screen.getByRole("button", { name: "Try" }));

  expect(api).toHaveBeenCalledWith("/tools/try", { name: "list_courses", arguments: {} });
  expect(await screen.findByText('{"courses": []}')).toBeTruthy();
});

test("an optional parameter that is left empty is not sent", async () => {
  vi.mocked(api).mockResolvedValue({ ok: true, output: "[]", images: [] });
  const user = show({
    name: "list_events",
    params: [
      { name: "from", description: "First day.", required: false, options: [] },
      { name: "search", description: "Title text.", required: false, options: [] },
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
  vi.mocked(api).mockResolvedValue({ ok: false, output: "not logged in to moodle", images: [] });
  const user = show();

  await user.click(screen.getByRole("button", { name: "Try" }));

  expect((await screen.findByRole("alert")).textContent).toBe("not logged in to moodle");
});

test("a parameter with fixed choices is a list, and the page pictures are shown", async () => {
  vi.mocked(api).mockResolvedValue({
    ok: true,
    output: "[page 2]",
    images: [{ page: 2, url: "data:image/png;base64,AAAA" }],
  });
  const user = show({
    name: "read_file",
    params: [
      { name: "path", description: "Path of the file.", required: true, options: [] },
      { name: "view", description: "Text or pages.", required: false, options: ["text", "pages"] },
    ],
  });

  await user.click(screen.getByRole("button", { name: "Try" }));
  await user.type(screen.getByLabelText("path"), "Uni/slides.pdf");
  await user.selectOptions(screen.getByLabelText("view (optional)"), "pages");
  await user.click(screen.getByRole("button", { name: "Run" }));

  expect(api).toHaveBeenCalledWith("/tools/try", {
    name: "read_file",
    arguments: { path: "Uni/slides.pdf", view: "pages" },
  });
  const picture = await screen.findByAltText("Page 2");
  expect(picture.getAttribute("src")).toBe("data:image/png;base64,AAAA");
});
