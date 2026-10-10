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
      { name: "from", description: "First day.", required: false, options: [], default: null },
      { name: "search", description: "Title text.", required: false, options: [], default: null },
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
      {
        name: "path",
        description: "Path of the file.",
        required: true,
        options: [],
        default: null,
      },
      {
        name: "view",
        description: "Text or pages.",
        required: false,
        options: ["text", "pages"],
        default: "text",
      },
    ],
  });

  await user.click(screen.getByRole("button", { name: "Try" }));
  await user.type(screen.getByLabelText("path"), "Uni/slides.pdf");
  const view = screen.getByLabelText<HTMLSelectElement>("view (optional)");
  expect(view.value).toBe("text");
  expect(view.options.length).toBe(2);
  await user.selectOptions(view, "pages");
  await user.click(screen.getByRole("button", { name: "Run" }));

  expect(api).toHaveBeenCalledWith("/tools/try", {
    name: "read_file",
    arguments: { path: "Uni/slides.pdf", view: "pages" },
  });
  const picture = await screen.findByAltText("Page 2");
  expect(picture.getAttribute("src")).toBe("data:image/png;base64,AAAA");
});

test("several page pictures are shown one at a time", async () => {
  vi.mocked(api).mockResolvedValue({
    ok: true,
    output: "",
    images: [
      { page: 4, url: "data:image/png;base64,AAAA" },
      { page: 5, url: "data:image/png;base64,BBBB" },
    ],
  });
  const user = show();

  await user.click(screen.getByRole("button", { name: "Try" }));

  expect(await screen.findByAltText("Page 4")).toBeTruthy();
  expect(screen.queryByAltText("Page 5")).toBeNull();
  expect(screen.getByText(/1 of 2/)).toBeTruthy();
  expect(screen.getByRole("button", { name: "Previous" })).toHaveProperty("disabled", true);

  await user.click(screen.getByRole("button", { name: "Next" }));

  expect(screen.getByAltText("Page 5").getAttribute("src")).toBe("data:image/png;base64,BBBB");
  expect(screen.getByText(/2 of 2/)).toBeTruthy();
  expect(screen.getByRole("button", { name: "Next" })).toHaveProperty("disabled", true);
});

test("running a tool again clears the previous result first", async () => {
  vi.mocked(api).mockResolvedValueOnce({ ok: true, output: "first answer", images: [] });
  const user = show({
    params: [{ name: "path", description: "Path.", required: false, options: [], default: null }],
  });

  await user.click(screen.getByRole("button", { name: "Try" }));
  await user.click(screen.getByRole("button", { name: "Run" }));
  expect(await screen.findByText("first answer")).toBeTruthy();

  let answer: (run: unknown) => void = () => {};
  vi.mocked(api).mockReturnValueOnce(new Promise((resolve) => (answer = resolve)));
  await user.click(screen.getByRole("button", { name: "Run" }));

  expect(screen.queryByText("first answer")).toBeNull();
  answer({ ok: true, output: "second answer", images: [] });
  expect(await screen.findByText("second answer")).toBeTruthy();
});
