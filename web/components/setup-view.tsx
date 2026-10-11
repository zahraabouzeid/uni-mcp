"use client";

import { useState } from "react";
import { api, type AppTarget, errorMessage } from "@/lib/api";
import { Button, CodeBlock, ErrorText, Status } from "./ui";

type Step = {
  title: string;
  hint?: string;
  snippet: (command: string) => string;
};

// The name MCP clients list the server under.
const SERVER = "uniMCP";

// A bare name, as after the installer, needs no quotes; a path may hold spaces.
const forShell = (command: string) => (/[\s\\/]/.test(command) ? `"${command}"` : command);

const APPS: { name: string; id?: AppTarget["id"]; steps: Step[] }[] = [
  {
    name: "Claude Desktop",
    id: "claude-desktop",
    steps: [
      {
        title: "Config file",
        hint: "Settings > Developer > Edit Config, then restart.",
        snippet: (command) =>
          JSON.stringify({ mcpServers: { [SERVER]: { command, args: ["serve"] } } }, null, 2),
      },
    ],
  },
  {
    name: "Claude Code",
    id: "claude-code",
    steps: [
      {
        title: "Terminal",
        snippet: (command) => `claude mcp add ${SERVER} -- ${forShell(command)} serve`,
      },
    ],
  },
  {
    name: "Codex",
    id: "codex",
    steps: [
      {
        title: "Terminal",
        snippet: (command) => `codex mcp add ${SERVER} -- ${forShell(command)} serve`,
      },
    ],
  },
  {
    name: "Other",
    steps: [
      {
        title: "Command",
        snippet: (command) => `${forShell(command)} serve`,
      },
    ],
  },
];

type Props = { command: string; apps: AppTarget[]; onChanged: () => void };

export function SetupView({ command, apps, onChanged }: Props) {
  const [selected, setSelected] = useState(0);
  const app = APPS[selected];
  const target = apps.find((a) => a.id === app.id && a.installed);

  return (
    <>
      <div role="tablist" className="mb-6 flex flex-wrap gap-x-5 gap-y-2 border-b border-line">
        {APPS.map((a, index) => (
          <button
            key={a.name}
            role="tab"
            type="button"
            aria-selected={index === selected}
            onClick={() => setSelected(index)}
            className={`-mb-px whitespace-nowrap border-b-2 pb-2 font-medium ${
              index === selected
                ? "border-ink text-ink"
                : "border-transparent text-muted hover:text-ink"
            }`}
          >
            {a.name}
          </button>
        ))}
      </div>

      {target && <AddTo key={target.id} name={app.name} target={target} onChanged={onChanged} />}

      <div className="grid gap-8">
        {app.steps.map((step) => (
          <Snippet key={`${app.name}:${step.title}`} step={step} code={step.snippet(command)} />
        ))}
      </div>
    </>
  );
}

function AddTo(props: { name: string; target: AppTarget; onChanged: () => void }) {
  const { name, target, onChanged } = props;
  const [busy, setBusy] = useState(false);
  const [justAdded, setJustAdded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const blocked = target.running && !target.added;

  async function add() {
    setBusy(true);
    setError(null);
    try {
      await api("/apps/add", { app: target.id });
      setJustAdded(true);
      onChanged();
    } catch (e) {
      setError(errorMessage(e));
    }
    setBusy(false);
  }

  return (
    <div className="mb-8 grid gap-2">
      <div className="flex items-center justify-between gap-4 rounded-md border border-line px-4 py-3">
        <Status ok={target.added}>
          {target.added ? "Added" : blocked ? `Quit ${name} first` : "Not added"}
        </Status>
        <span className="flex items-center gap-3">
          {justAdded && <span className="text-muted">Restart {name} to apply.</span>}
          <Button variant="primary" disabled={busy || blocked || target.added} onClick={add}>
            Add to {name}
          </Button>
        </span>
      </div>
      {error && <ErrorText>{error}</ErrorText>}
    </div>
  );
}

function Snippet({ step, code }: { step: Step; code: string }) {
  const [copied, setCopied] = useState(false);

  async function copy() {
    await navigator.clipboard.writeText(code);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  return (
    <div>
      <div className="mb-2 flex items-center justify-between gap-4">
        <p>
          <span className="font-medium">{step.title}</span>
          {step.hint && <span className="ml-3 text-muted">{step.hint}</span>}
        </p>
        <Button onClick={copy}>{copied ? "Copied" : "Copy"}</Button>
      </div>
      <CodeBlock code={code} className="whitespace-pre-wrap break-all bg-panel" />
    </div>
  );
}
