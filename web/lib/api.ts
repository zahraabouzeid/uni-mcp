export type University = { id: string; name: string; color: string | null };

export type Service = {
  id: string;
  name: string;
  kind: "moodle" | "webdav" | "calendar";
  logged_in: boolean;
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

export type DashboardState = {
  universities: University[];
  selected: string | null;
  services: Service[];
  tools: Tool[];
  executable: string | null;
  version: string;
  repository: string;
  browser_sign_in: boolean;
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
    throw new Error("unimcp is not running. Start it again in a terminal.");
  });
  if (response.status === 403) {
    throw new Error("Open the address that unimcp printed in the terminal.");
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
