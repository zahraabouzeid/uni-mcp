import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { serverApi } from "@/lib/server-api";
import { Consent, readRequest } from "./consent";

vi.mock("@/lib/server-api", async (original) => ({
  ...(await original<typeof import("@/lib/server-api")>()),
  serverApi: vi.fn(),
}));

const about = { version: "1.0.0", repository: "https://example.org", public_url: null };
const member = { person: { id: "b", name: "Sam", admin: false }, about };
const admin = { person: { id: "a", name: "Zahraa", admin: true }, about };

const request = readRequest(
  "?response_type=code&client_id=app1&redirect_uri=https%3A%2F%2Fclaude.ai%2Fcb" +
    "&code_challenge=c&code_challenge_method=S256&state=s1",
);

function server(session: object | null, app: string | null = "Claude") {
  vi.mocked(serverApi).mockImplementation(async (path) => {
    if (path.startsWith("/oauth/app")) {
      if (!app) throw new Error("unknown app");
      return { name: app };
    }
    if (path === "/account/me") {
      if (!session) throw new Error("log in first");
      return session;
    }
    if (path === "/oauth/answer") return { redirect: "https://claude.ai/cb?code=x" };
    return {};
  });
}

afterEach(() => {
  cleanup();
  vi.mocked(serverApi).mockReset();
  vi.unstubAllGlobals();
});

test("a link without what OAuth needs is not a request", () => {
  expect(readRequest("?client_id=app1&redirect_uri=x&code_challenge=c")).toBeNull();
  expect(readRequest("?response_type=code&client_id=app1&redirect_uri=x")).toBeNull();
  expect(request?.state).toBe("s1");
});

test("an incomplete link and an unknown app cannot connect", async () => {
  render(<Consent request={null} />);
  expect(screen.getByText("Cannot connect")).toBeTruthy();
  cleanup();

  server(member, null);
  render(<Consent request={request} />);
  expect(await screen.findByText(/does not know the app/)).toBeTruthy();
});

test("someone logged out logs in first, and is told what for", async () => {
  server(null);
  render(<Consent request={request} />);
  expect(await screen.findByText("to connect Claude to your account")).toBeTruthy();
  expect(screen.getByLabelText("Username")).toBeTruthy();
});

test("a member sees who asks and what it may read, and allowing sends them back", async () => {
  server(member);
  const assign = vi.fn();
  vi.stubGlobal("location", { ...window.location, assign });
  const user = userEvent.setup();
  render(<Consent request={request} />);

  expect(await screen.findByText("Connect Claude")).toBeTruthy();
  expect(screen.getByText("Sam")).toBeTruthy();
  expect(screen.getByText("Your timetable")).toBeTruthy();

  await user.click(screen.getByRole("button", { name: "Allow" }));
  expect(serverApi).toHaveBeenCalledWith("/oauth/answer", {
    client_id: "app1",
    redirect_uri: "https://claude.ai/cb",
    code_challenge: "c",
    code_challenge_method: "S256",
    state: "s1",
    allow: true,
  });
  expect(assign).toHaveBeenCalledWith("https://claude.ai/cb?code=x");
});

test("the admin cannot connect apps", async () => {
  server(admin);
  render(<Consent request={request} />);
  expect(await screen.findByText(/admin account cannot connect apps/)).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Allow" })).toBeNull();
});
