export type Person = { id: string; name: string; admin: boolean };

export type About = { version: string; repository: string; public_url: string | null };

export type Session = { person: Person; about: About };

// In server mode the browser is known by its session cookie, not by a key.
export async function serverApi<T>(path: string, post?: object): Promise<T> {
  const response = await fetch(`/api${path}`, {
    method: post ? "POST" : "GET",
    headers: { "content-type": "application/json" },
    body: post ? JSON.stringify(post) : undefined,
    credentials: "same-origin",
  }).catch(() => {
    throw new Error("The server did not answer.");
  });
  const data = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new Error(data.error ?? "Something went wrong.");
  }
  return data as T;
}
