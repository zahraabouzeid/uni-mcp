"use client";

import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { errorMessage } from "@/lib/api";
import { serverApi } from "@/lib/server-api";
import { Snippet, Tabs } from "./setup-view";
import { ActionCell, Button, Cell, ErrorText, Table } from "./ui";

export type ConnectedApp = { id: string; name: string; connected: number; last_used: number };

type Guide = {
  name: string;
  steps: string[];
  commands?: { title: string; hint?: string; code: (address: string) => string }[];
};

const GUIDES: Guide[] = [
  {
    name: "Claude",
    steps: [
      "In Claude, open Settings > Connectors and choose Add custom connector.",
      "Name it uniMCP and paste the address above.",
      "Choose Connect, log in on the page that opens and allow access.",
    ],
  },
  {
    name: "ChatGPT",
    steps: [
      "In ChatGPT, open Settings > Apps & Connectors > Advanced settings and turn on Developer mode.",
      "Choose Create, name it uniMCP, paste the address above and select OAuth.",
      "Log in on the page that opens and allow access.",
    ],
  },
  {
    name: "Claude Code",
    steps: ["Add the server, then run /mcp in Claude Code to log in and allow access."],
    commands: [
      {
        title: "Terminal",
        code: (address) => `claude mcp add --transport http uniMCP ${address}`,
      },
    ],
  },
  {
    name: "Codex",
    steps: ["Add the server, then log in and allow access in the browser."],
    commands: [
      { title: "Terminal", code: (address) => `codex mcp add uniMCP --url ${address}` },
      { title: "Log in", code: () => "codex mcp login uniMCP" },
    ],
  },
  {
    name: "Other",
    steps: [
      "Add the address above as a remote MCP server in any app that supports OAuth.",
      "The app opens this server's login page; log in and allow access.",
    ],
  },
];

const DATE: Intl.DateTimeFormatOptions = { day: "numeric", month: "short", year: "numeric" };

function day(seconds: number) {
  return new Date(seconds * 1000).toLocaleDateString(undefined, DATE);
}

export function ConnectView() {
  const origin = useSyncExternalStore(
    () => () => {},
    () => window.location.origin,
    () => "",
  );
  const address = `${origin}/mcp`;
  const [selected, setSelected] = useState(0);
  const guide = GUIDES[selected];

  return (
    <div className="grid max-w-3xl gap-10">
      <section className="grid gap-6">
        <Snippet step={{ title: "Server address" }} code={address} />
        <div>
          <Tabs names={GUIDES.map((g) => g.name)} selected={selected} onSelect={setSelected} />
          <ol className="grid list-decimal gap-1.5 pl-5">
            {guide.steps.map((step) => (
              <li key={step}>{step}</li>
            ))}
          </ol>
          {guide.commands && (
            <div className="mt-6 grid gap-6">
              {guide.commands.map((command) => (
                <Snippet
                  key={`${guide.name}:${command.title}`}
                  step={command}
                  code={command.code(address)}
                />
              ))}
            </div>
          )}
        </div>
      </section>
      <ConnectedApps />
    </div>
  );
}

function ConnectedApps() {
  const [apps, setApps] = useState<ConnectedApp[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(
    () =>
      serverApi<{ apps: ConnectedApp[] }>("/oauth/connected").then(
        (loaded) => setApps(loaded.apps),
        (e) => setError(errorMessage(e)),
      ),
    [],
  );

  useEffect(() => {
    load();
  }, [load]);

  async function disconnect(id: string) {
    setError(null);
    try {
      await serverApi("/oauth/disconnect", { id });
    } catch (e) {
      setError(errorMessage(e));
    }
    load();
  }

  return (
    <section className="grid gap-3">
      <h2 className="font-medium">Connected apps</h2>
      {apps && apps.length === 0 && (
        <p className="text-muted">No app is connected to your account yet.</p>
      )}
      {apps && apps.length > 0 && (
        <Table columns={["App", "Connected", "Last used", ""]}>
          {apps.map((app) => (
            <tr key={app.id}>
              <Cell className="font-medium">{app.name}</Cell>
              <Cell className="text-muted">{day(app.connected)}</Cell>
              <Cell className="text-muted">{day(app.last_used)}</Cell>
              <ActionCell>
                <Button onClick={() => disconnect(app.id)}>Disconnect</Button>
              </ActionCell>
            </tr>
          ))}
        </Table>
      )}
      {error && <ErrorText>{error}</ErrorText>}
    </section>
  );
}
