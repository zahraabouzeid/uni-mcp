"use client";

import { type FormEvent, useState } from "react";
import { api, errorMessage, type Tool, type ToolRun } from "@/lib/api";
import { ActionCell, Button, Cell, Choice, CodeBlock, DetailRow, ErrorText, Field } from "./ui";

export const TOOL_COLUMNS = ["Tool", "Service", "Description", ""];

export function ToolRow({ tool }: { tool: Tool }) {
  const [open, setOpen] = useState(false);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<ToolRun | null>(null);
  const missing = tool.service ? `Connect ${tool.service} first` : "Not offered by this university";

  async function run(args: Record<string, string>) {
    setRunning(true);
    setResult(null);
    try {
      setResult(await api<ToolRun>("/tools/try", { name: tool.name, arguments: args }));
    } catch (e) {
      setResult({ ok: false, output: errorMessage(e), images: [] });
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
            title={tool.available ? undefined : missing}
          >
            {open ? "Close" : "Try"}
          </Button>
        </ActionCell>
      </tr>

      {open && tool.available && (
        <DetailRow columns={TOOL_COLUMNS.length}>
          <form onSubmit={submit} className="flex flex-wrap items-end gap-3">
            {tool.params.map((param) => {
              const field = {
                name: param.name,
                label: param.required ? param.name : `${param.name} (optional)`,
                title: param.description,
                required: param.required,
              };
              return param.options.length > 0 ? (
                <Choice
                  key={param.name}
                  options={param.options}
                  defaultValue={param.default ?? undefined}
                  {...field}
                />
              ) : (
                <Field key={param.name} {...field} />
              );
            })}
            <Button type="submit" variant="primary" disabled={running}>
              {running ? "Running" : "Run"}
            </Button>
          </form>
          {result && (
            <div className="mt-4">
              {result.ok ? (
                <>
                  <CodeBlock code={result.output} className="max-h-96" />
                  {result.images.length > 0 && <Pictures images={result.images} />}
                </>
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

function Pictures({ images }: { images: ToolRun["images"] }) {
  const [shown, setShown] = useState(0);
  const index = Math.min(shown, images.length - 1);
  const image = images[index];
  const label = image.page ? `Page ${image.page}` : "Picture";

  return (
    <div className="mt-3 overflow-hidden rounded-md border border-line bg-canvas">
      <div className="flex items-center justify-between gap-3 border-b border-line px-3 py-2">
        <span className="text-muted">
          {label}
          {images.length > 1 && ` (${index + 1} of ${images.length})`}
        </span>
        {images.length > 1 && (
          <span className="flex gap-2">
            <Button disabled={index === 0} onClick={() => setShown(index - 1)}>
              Previous
            </Button>
            <Button disabled={index === images.length - 1} onClick={() => setShown(index + 1)}>
              Next
            </Button>
          </span>
        )}
      </div>
      {/* A data: URL from the backend; next/image has nothing to optimise. */}
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img src={image.url} alt={label} className="mx-auto max-h-[70vh] max-w-full" />
    </div>
  );
}
