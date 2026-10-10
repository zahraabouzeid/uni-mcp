"use client";

import { type FormEvent, useState } from "react";
import { api, errorMessage, type Tool, type ToolRun } from "@/lib/api";
import { ActionCell, Button, Cell, CodeBlock, DetailRow, ErrorText, Field } from "./ui";

export const TOOL_COLUMNS = ["Tool", "Service", "Description", ""];

export function ToolRow({ tool }: { tool: Tool }) {
  const [open, setOpen] = useState(false);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<ToolRun | null>(null);

  async function run(args: Record<string, string>) {
    setRunning(true);
    try {
      setResult(await api<ToolRun>("/tools/try", { name: tool.name, arguments: args }));
    } catch (e) {
      setResult({ ok: false, output: errorMessage(e) });
    }
    setRunning(false);
  }

  function toggle() {
    setOpen(!open);
    if (!open && tool.params.length === 0 && !result) run({});
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const args: Record<string, string> = {};
    for (const param of tool.params) {
      const value = String(form.get(param.name) ?? "").trim();
      // Empty optional fields are left out, otherwise "" would count as a value.
      if (value || param.required) args[param.name] = value;
    }
    run(args);
  }

  return (
    <>
      <tr className={tool.available ? "" : "opacity-45"}>
        <Cell className="whitespace-nowrap align-top font-mono text-[13px]">{tool.name}</Cell>
        <Cell className="whitespace-nowrap align-top">{tool.service}</Cell>
        <Cell className="text-muted">{tool.description}</Cell>
        <ActionCell>
          <Button
            onClick={toggle}
            disabled={!tool.available}
            title={tool.available ? undefined : `Connect ${tool.service} first`}
          >
            {open ? "Close" : "Try"}
          </Button>
        </ActionCell>
      </tr>

      {open && tool.available && (
        <DetailRow columns={TOOL_COLUMNS.length}>
          <form onSubmit={submit} className="flex flex-wrap items-end gap-3">
            {tool.params.map((param) => (
              <Field
                key={param.name}
                name={param.name}
                label={param.required ? param.name : `${param.name} (optional)`}
                title={param.description}
                required={param.required}
              />
            ))}
            <Button type="submit" variant="primary" disabled={running}>
              {running ? "Running" : "Run"}
            </Button>
          </form>
          {result && (
            <div className="mt-4">
              {result.ok ? (
                <CodeBlock code={result.output} className="max-h-96" />
              ) : (
                <ErrorText>{result.output}</ErrorText>
              )}
            </div>
          )}
        </DetailRow>
      )}
    </>
  );
}
