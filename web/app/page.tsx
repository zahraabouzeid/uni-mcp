"use client";

import { useCallback, useEffect, useState } from "react";
import { SERVICE_COLUMNS, ServiceRow } from "@/components/service-row";
import { SetupView } from "@/components/setup-view";
import { TOOL_COLUMNS, ToolRow } from "@/components/tool-row";
import { ActionCell, Button, Cell, ErrorText, Table } from "@/components/ui";
import { api, errorMessage, type SetupState } from "@/lib/api";

const VIEWS = ["services", "tools", "setup"] as const;
type View = (typeof VIEWS)[number];

const NEUTRAL = "#1f1f23";
// unimcp stops when these requests stop coming, i.e. when the tab is closed.
const REFRESH_MS = 10_000;

export default function Console() {
  const [state, setState] = useState<SetupState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>(() => {
    const hash = typeof window === "undefined" ? "" : window.location.hash.slice(1);
    return VIEWS.find((v) => v === hash) ?? "services";
  });

  const load = useCallback(
    () =>
      api<SetupState>("/state").then(
        (loaded) => {
          setState(loaded);
          setError(null);
        },
        (e) => setError(errorMessage(e)),
      ),
    [],
  );

  useEffect(() => {
    load();
    const timer = setInterval(load, REFRESH_MS);
    return () => clearInterval(timer);
  }, [load]);

  async function chooseUniversity(id: string) {
    try {
      await api("/university", { id });
    } catch (e) {
      setError(errorMessage(e));
    }
    load();
  }

  if (!state) {
    return error ? <p className="px-10 py-8">{error}</p> : null;
  }

  const university = state.universities.find((u) => u.id === state.selected);
  const connected = state.services.filter((s) => s.logged_in).length;
  const nav: { view: View; label: string; count?: string }[] = [
    { view: "services", label: "Services", count: `${connected}/${state.services.length}` },
    { view: "tools", label: "Tools", count: String(state.tools.length) },
    { view: "setup", label: "Setup" },
  ];

  return (
    <div className="flex h-screen overflow-hidden">
      <aside
        className="flex w-60 shrink-0 flex-col overflow-y-auto text-white"
        style={{ backgroundColor: university?.color ?? NEUTRAL }}
      >
        <p className="flex h-14 shrink-0 items-center bg-black/25 px-4 text-lg tracking-tight">
          {/* The icon as a mask, so it takes the text colour. */}
          <span
            aria-hidden
            className="mr-2 size-7 bg-current [mask:url(/icon.svg)_center/contain_no-repeat]"
          />
          <span className="font-light">uni</span>
          <span className="font-bold">MCP</span>
        </p>

        {university && (
          <nav className="mt-4 grid gap-0.5 px-3">
            {nav.map((item) => (
              <button
                key={item.view}
                type="button"
                aria-current={view === item.view ? "page" : undefined}
                onClick={() => {
                  window.location.hash = item.view;
                  setView(item.view);
                }}
                className={`flex h-8 items-center justify-between rounded-md px-2 text-left ${
                  view === item.view
                    ? "bg-white/15 font-medium"
                    : "text-white/75 hover:bg-white/10 hover:text-white"
                }`}
              >
                {item.label}
                {item.count && <span className="text-xs tabular-nums">{item.count}</span>}
              </button>
            ))}
          </nav>
        )}

        <div className="mt-auto px-4 py-4 text-xs text-white/60">
          <p className="mt-1">Version {state.version}</p>
          <p className="flex items-center gap-1">
            Made with
            <svg aria-label="love" role="img" viewBox="0 0 16 16" className="size-3 fill-white">
              <path d="M8 14s-5.5-3.4-5.5-7.4A3 3 0 0 1 8 5a3 3 0 0 1 5.5 1.6C13.5 10.6 8 14 8 14z" />
            </svg>
            by Zahraa Bou Zeid
          </p>
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center justify-between gap-4 border-b border-line px-10">
          <h1 className="text-lg font-semibold">
            {university ? nav.find((item) => item.view === view)?.label : "University"}
          </h1>
          <label>
            <span className="sr-only">University</span>
            <select
              value={state.selected ?? ""}
              onChange={(event) => chooseUniversity(event.target.value)}
              className="h-8 max-w-64 cursor-pointer rounded-md border border-line bg-canvas px-2 outline-none focus:border-ink"
            >
              {!state.selected && (
                <option value="" disabled>
                  Select university
                </option>
              )}
              {state.universities.map((u) => (
                <option key={u.id} value={u.id}>
                  {u.name}
                </option>
              ))}
            </select>
          </label>
        </header>

        <main className="flex-1 overflow-auto px-10 py-8">
          <div className="max-w-4xl">
            {error && (
              <div className="mb-6">
                <ErrorText>{error}</ErrorText>
              </div>
            )}

            {!university ? (
              <>
                <Table>
                  {state.universities.map((u) => (
                    <tr key={u.id}>
                      <Cell className="font-medium">{u.name}</Cell>
                      <ActionCell>
                        <Button variant="primary" onClick={() => chooseUniversity(u.id)}>
                          Select
                        </Button>
                      </ActionCell>
                    </tr>
                  ))}
                </Table>
              </>
            ) : view === "services" ? (
              <>
                <Table columns={SERVICE_COLUMNS}>
                  {state.services.map((service) => (
                    <ServiceRow
                      key={`${university.id}:${service.id}`}
                      service={service}
                      browserSignIn={state.browser_sign_in}
                      onChanged={load}
                    />
                  ))}
                </Table>
              </>
            ) : view === "tools" ? (
              <>
                <Table columns={TOOL_COLUMNS}>
                  {state.tools.map((tool) => (
                    <ToolRow key={tool.name} tool={tool} />
                  ))}
                </Table>
              </>
            ) : (
              state.executable && <SetupView executable={state.executable} />
            )}
          </div>
        </main>
      </div>
    </div>
  );
}
