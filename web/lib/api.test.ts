import { afterEach, expect, test, vi } from "vitest";
import { api } from "./api";

function backendAnswers(status: number, body: unknown) {
  const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify(body), { status }));
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

afterEach(() => vi.unstubAllGlobals());

test("sends the key from the page address", async () => {
  window.history.replaceState(null, "", "/?key=abc123");
  const fetch = backendAnswers(200, { ok: true });

  await api("/state");

  const [url, request] = fetch.mock.calls[0];
  expect(url).toBe("/api/state");
  expect(request.method).toBe("GET");
  expect(request.headers["x-unimcp-key"]).toBe("abc123");
});

test("posts a body as JSON", async () => {
  const fetch = backendAnswers(200, { ok: true });

  await api("/logout", { service: "moodle" });

  const [, request] = fetch.mock.calls[0];
  expect(request.method).toBe("POST");
  expect(JSON.parse(request.body)).toEqual({ service: "moodle" });
});

test("returns what the backend answered", async () => {
  backendAnswers(200, { version: "1.2.3" });
  await expect(api("/state")).resolves.toEqual({ version: "1.2.3" });
});

test("explains a missing or wrong key", async () => {
  backendAnswers(403, null);
  await expect(api("/state")).rejects.toThrow(/printed in the terminal/);
});

test("passes on the backend's error message", async () => {
  backendAnswers(400, { error: "choose a university first" });
  await expect(api("/state")).rejects.toThrow("choose a university first");
});

test("says so when unimcp is not running", async () => {
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("Failed to fetch")));
  await expect(api("/state")).rejects.toThrow(/not running/);
});
