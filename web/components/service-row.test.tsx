import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { api, type Service } from "@/lib/api";
import { ServiceRow } from "./service-row";

vi.mock("@/lib/api", async (original) => ({
  ...(await original<typeof import("@/lib/api")>()),
  api: vi.fn(),
}));

const service: Service = {
  id: "cloud",
  name: "Cloud",
  kind: "webdav",
  logged_in: false,
  checking: false,
  expired: false,
  error: null,
};

function show(overrides: Partial<Service> = {}, browserSignIn = true) {
  const onChanged = vi.fn();
  render(
    <table>
      <tbody>
        <ServiceRow
          service={{ ...service, ...overrides }}
          browserSignIn={browserSignIn}
          onChanged={onChanged}
        />
      </tbody>
    </table>,
  );
  return { user: userEvent.setup(), onChanged };
}

afterEach(() => {
  cleanup();
  vi.mocked(api).mockReset();
});

test("a connected service can be disconnected", async () => {
  vi.mocked(api).mockResolvedValue({ ok: true });
  const { user, onChanged } = show({ logged_in: true });
  expect(screen.getByText("Connected")).toBeTruthy();

  await user.click(screen.getByRole("button", { name: "Disconnect" }));

  expect(api).toHaveBeenCalledWith("/logout", { service: "cloud" });
  expect(onChanged).toHaveBeenCalled();
});

test("a login that is still being checked does not count as connected yet", () => {
  show({ logged_in: true, checking: true });
  expect(screen.getByText("Checking")).toBeTruthy();
  expect(screen.queryByText("Connected")).toBeNull();
});

test("an expired login says so and offers to connect again", async () => {
  const { user } = show({ logged_in: true, expired: true });
  expect(screen.getByText("Sign-in expired")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Disconnect" })).toBeNull();

  await user.click(screen.getByRole("button", { name: "Reconnect" }));

  expect(screen.getByLabelText("App password")).toBeTruthy();
});

test("cloud storage is connected with a username and an app password", async () => {
  vi.mocked(api).mockResolvedValue({ ok: true });
  const { user, onChanged } = show();

  await user.click(screen.getByRole("button", { name: "Connect" }));
  await user.type(screen.getByLabelText("Username"), "ab123456");
  await user.type(screen.getByLabelText("App password"), "secret");
  await user.click(screen.getByRole("button", { name: "Connect" }));

  expect(api).toHaveBeenCalledWith("/login/webdav", { user: "ab123456", password: "secret" });
  expect(onChanged).toHaveBeenCalled();
});

test("a calendar is connected with its link", async () => {
  vi.mocked(api).mockResolvedValue({ ok: true });
  const { user } = show({ id: "calendar", name: "Calendar", kind: "calendar" });

  await user.click(screen.getByRole("button", { name: "Connect" }));
  await user.type(screen.getByLabelText("iCal link"), "https://campus.example/ical");
  await user.click(screen.getByRole("button", { name: "Connect" }));

  expect(api).toHaveBeenCalledWith("/login/calendar", { link: "https://campus.example/ical" });
});

test("a rejected login shows the reason and stores nothing", async () => {
  vi.mocked(api).mockRejectedValue(new Error("Cloud did not accept this login"));
  const { user, onChanged } = show();

  await user.click(screen.getByRole("button", { name: "Connect" }));
  await user.type(screen.getByLabelText("Username"), "ab123456");
  await user.type(screen.getByLabelText("App password"), "wrong");
  await user.click(screen.getByRole("button", { name: "Connect" }));

  expect((await screen.findByRole("alert")).textContent).toBe("Cloud did not accept this login");
  expect(onChanged).not.toHaveBeenCalled();
});

test("Moodle starts a browser sign-in where that is supported", async () => {
  vi.mocked(api).mockResolvedValue({ url: "https://moodle.example", opened: true });
  const { user } = show({ id: "moodle", name: "Moodle", kind: "moodle" });

  await user.click(screen.getByRole("button", { name: "Connect" }));

  expect(api).toHaveBeenCalledWith("/login/moodle", {});
  expect(await screen.findByText("Signing in")).toBeTruthy();
});

test("Moodle points to the terminal where it is not", async () => {
  const { user } = show({ id: "moodle", name: "Moodle", kind: "moodle" }, false);

  await user.click(screen.getByRole("button", { name: "Connect" }));

  expect(api).not.toHaveBeenCalled();
  expect(screen.getByRole("alert").textContent).toContain("unimcp login moodle");
});
