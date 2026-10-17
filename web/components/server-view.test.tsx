import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { serverApi } from "@/lib/server-api";
import { ServerView } from "./server-view";

vi.mock("@/lib/server-api", async (original) => ({
  ...(await original<typeof import("@/lib/server-api")>()),
  serverApi: vi.fn(),
}));

const off = { autostart: { on: false, moment: "boot", note: null } };
const on = { autostart: { on: true, moment: "boot", note: null } };

afterEach(() => {
  cleanup();
  vi.mocked(serverApi).mockReset();
});

test("starting with the computer can be turned on", async () => {
  vi.mocked(serverApi).mockImplementation(async (path) => (path === "/server" ? off : on));
  const user = userEvent.setup();
  render(<ServerView />);
  expect(await screen.findByText("Off")).toBeTruthy();

  await user.click(screen.getByRole("button", { name: "Turn on" }));

  expect(serverApi).toHaveBeenCalledWith("/server/autostart", { on: true });
  expect(await screen.findByText("On")).toBeTruthy();
  expect(screen.getByText("Starts when this computer starts.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Turn off" })).toBeTruthy();
});

test("a hint from the system replaces the usual line", async () => {
  const hint = "To start it at boot, run once: sudo loginctl enable-linger $USER";
  vi.mocked(serverApi).mockResolvedValue({ autostart: { on: true, moment: "boot", note: hint } });
  render(<ServerView />);

  expect(await screen.findByText(hint)).toBeTruthy();
  expect(screen.queryByText("Starts when this computer starts.")).toBeNull();
});

test("a system without this option says so and offers no button", async () => {
  vi.mocked(serverApi).mockResolvedValue({ autostart: { on: false, moment: null, note: null } });
  render(<ServerView />);

  expect(await screen.findByText("Not available on this system")).toBeTruthy();
  expect(screen.queryByRole("button")).toBeNull();
});
