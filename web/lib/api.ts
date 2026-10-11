export type University = { id: string; name: string; color: string | null };

export type Service = {
  id: string;
  name: string;
  kind: "moodle" | "webdav" | "calendar";
  logged_in: boolean;
  checking: boolean;
  expired: boolean;
  error: string | null;
};

export type ToolParam = {
  name: string;
  description: string;
  required: boolean;
  options: string[];
  default: string | null;
};

export type Tool = {
  name: string;
  description: string;
  params: ToolParam[];
  service: string | null;
  available: boolean;
};

export type ToolRun = {
  ok: boolean;
  output: string;
  images: { page: number | null; url: string }[];
};

export type AppTarget = {
  id: "claude-desktop" | "claude-code" | "codex" | "gemini-cli";
  installed: boolean;
  added: boolean;
  running: boolean;
};

export type DashboardState = {
  universities: University[];
  selected: string | null;
  services: Service[];
  tools: Tool[];
  custom_instructions: string;
  custom_instructions_limit: number;
  command: string | null;
  apps: AppTarget[];
  version: string;
  repository: string;
  browser_sign_in: boolean;
  hosted: boolean;
};

export type MoodleProgress = {
  state: "waiting" | "done" | "failed";
  message: string | null;
};

// The server only answers requests that carry the key from the page address.
export async function api<T>(path: string, post?: object): Promise<T> {
  const key = new URLSearchParams(window.location.search).get("key") ?? "";
  const response = await fetch(`/api${path}`, {
    method: post ? "POST" : "GET",
    headers: { "x-unimcp-key": key, "content-type": "application/json" },
    body: post ? JSON.stringify(post) : undefined,
  }).catch(() => {
    throw new Error("uniMCP is not running. Start it again to continue.");
  });
  if (response.status === 403) {
    throw new Error("This link is no longer valid. Start uniMCP again to open the dashboard.");
  }
  const data = await response.json();
  if (!response.ok) {
    throw new Error(data.error ?? "Something went wrong.");
  }
  return data as T;
}

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : "Something went wrong.";
}

// Lets unimcp stop right after the tab closes. A beacon cannot carry headers,
// so the key goes in the address.
export function announceClosing() {
  const key = new URLSearchParams(window.location.search).get("key") ?? "";
  navigator.sendBeacon(`/api/closing?key=${encodeURIComponent(key)}`);
}
