"use client";

import { useState } from "react";
import { Button, CodeBlock } from "./ui";

type Step = {
  title: string;
  hint?: string;
  snippet: (command: string) => string;
};

// The name MCP clients list the server under.
const SERVER = "uniMCP";

// A bare name, as after the installer, needs no quotes; a path may hold spaces.
const forShell = (command: string) => (/[\s\\/]/.test(command) ? `"${command}"` : command);

const APPS: { name: string; steps: Step[] }[] = [
  {
    name: "Claude Desktop",
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
    steps: [
      {
        title: "Terminal",
        snippet: (command) => `claude mcp add ${SERVER} -- ${forShell(command)} serve`,
      },
    ],
  },
  {
    name: "Codex",
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

export function SetupView({ command }: { command: string }) {
  const [selected, setSelected] = useState(0);
  const app = APPS[selected];

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

      <div className="grid gap-8">
        {app.steps.map((step) => (
          <Snippet key={`${app.name}:${step.title}`} step={step} code={step.snippet(command)} />
        ))}
      </div>
    </>
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
