"use client";

import { useCallback, useEffect, useState } from "react";
import { announceClosing, api, errorMessage, type DashboardState } from "@/lib/api";
import type { Person } from "@/lib/server-api";
import { PeopleView } from "./people-view";
import { SERVICE_COLUMNS, ServiceRow } from "./service-row";
import { ServerView } from "./server-view";
import { SettingsView } from "./settings-view";
import { SetupView } from "./setup-view";
import { TOOL_COLUMNS, ToolRow } from "./tool-row";
import { ActionCell, Button, Cell, ErrorText, Table } from "./ui";

const VIEWS = ["services", "tools", "setup", "settings", "people", "server"] as const;
type View = (typeof VIEWS)[number];

const NEUTRAL = "#1f1f23";
// unimcp also stops if these requests stop coming without a goodbye.
const REFRESH_MS = 10_000;
const CHECKING_MS = 1_500;

// On a home server the dashboard belongs to an account.
type Account = { person: Person; onLogOut: () => void };

export function Console({ account }: { account?: Account }) {
  const hosted = account !== undefined;
  const [state, setState] = useState<DashboardState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>(() => {
    const hash = typeof window === "undefined" ? "" : window.location.hash.slice(1);
    return VIEWS.find((v) => v === hash) ?? "services";
  });

  const load = useCallback(
    () =>
      api<DashboardState>("/state").then(
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
    // A server keeps running when the page closes.
    if (!hosted) window.addEventListener("pagehide", announceClosing);
    return () => {
      clearInterval(timer);
      window.removeEventListener("pagehide", announceClosing);
    };
  }, [load, hosted]);

  const checking = state?.services.some((s) => s.checking) ?? false;
  useEffect(() => {
    if (!checking) return;
    const timer = setTimeout(load, CHECKING_MS);
    return () => clearTimeout(timer);
  }, [checking, state, load]);

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
  const connected = state.services.filter((s) => s.logged_in && !s.expired).length;
  type Entry = { view: View; label: string; count?: string };
  // The owner of a server looks after it and has no university of their own there.
  const admin = account?.person.owner ?? false;
  const own: Entry[] = admin
    ? []
    : university
      ? [
          { view: "services", label: "Services", count: `${connected}/${state.services.length}` },
          { view: "tools", label: "Tools", count: String(state.tools.length) },
          ...(state.command ? [{ view: "setup", label: "Setup" } as Entry] : []),
          { view: "settings", label: "Settings" },
        ]
      : [{ view: "services", label: "University" }];
  const forAdmin: Entry[] = admin
    ? [
        { view: "people", label: "People" },
        { view: "server", label: "Settings" },
      ]
    : [];
  const nav = [...own, ...forAdmin];
  const shown = nav.some((item) => item.view === view) ? view : nav[0].view;

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

        {(university || admin) && (
          <nav className="mt-4 grid gap-0.5 px-3">
            {nav.map((item) => (
              <button
                key={item.view}
                type="button"
                aria-current={shown === item.view ? "page" : undefined}
                onClick={() => {
                  window.location.hash = item.view;
                  setView(item.view);
                }}
                className={`flex h-8 items-center justify-between rounded-md px-2 text-left ${
                  shown === item.view
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

        {account && (
          <div className="mt-auto flex items-center justify-between gap-2 px-4 pt-4">
            <span className="truncate text-white/75">{account.person.name}</span>
            <Button onClick={account.onLogOut}>Log out</Button>
          </div>
        )}
        <div
          className={`flex items-end justify-between gap-2 px-4 py-4 text-xs text-white/60 ${
            account ? "" : "mt-auto"
          }`}
        >
          <div>
            <p>v{state.version}</p>
            <p className="flex items-center gap-1">
              Made with
              <svg aria-label="love" role="img" viewBox="0 0 16 16" className="size-3 fill-white">
                <path d="M8 14s-5.5-3.4-5.5-7.4A3 3 0 0 1 8 5a3 3 0 0 1 5.5 1.6C13.5 10.6 8 14 8 14z" />
              </svg>
              by Zahraa Bou Zeid
            </p>
          </div>
          <a
            href={state.repository}
            target="_blank"
            rel="noreferrer"
            aria-label="uniMCP on GitHub"
            title="uniMCP on GitHub"
            className="hover:text-white"
          >
            <svg aria-hidden viewBox="0 0 16 16" className="size-5 fill-current">
              <path d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z" />
            </svg>
          </a>
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center justify-between gap-4 border-b border-line px-10">
          <h1 className="text-lg font-semibold">
            {nav.find((item) => item.view === shown)?.label}
          </h1>
          {!admin && (
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
          )}
        </header>

        <main className="flex-1 overflow-auto px-10 py-8">
          <div className="max-w-4xl">
            {error && (
              <div className="mb-6">
                <ErrorText>{error}</ErrorText>
              </div>
            )}

            {shown === "people" ? (
              <PeopleView />
            ) : shown === "server" ? (
              <ServerView />
            ) : !university ? (
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
            ) : shown === "services" ? (
              <>
                <Table columns={SERVICE_COLUMNS}>
                  {state.services.map((service) => (
                    <ServiceRow
                      key={`${university.id}:${service.id}`}
                      service={service}
                      browserSignIn={state.browser_sign_in}
                      hosted={state.hosted}
                      onChanged={load}
                    />
                  ))}
                </Table>
              </>
            ) : shown === "tools" ? (
              <>
                <Table columns={TOOL_COLUMNS}>
                  {state.tools.map((tool) => (
                    <ToolRow key={tool.name} tool={tool} />
                  ))}
                </Table>
              </>
            ) : shown === "settings" ? (
              <SettingsView
                key={university.id}
                saved={state.custom_instructions}
                limit={state.custom_instructions_limit}
                onSaved={load}
              />
            ) : (
              state.command && (
                <SetupView command={state.command} apps={state.apps} onChanged={load} />
              )
            )}
          </div>
        </main>
      </div>
    </div>
  );
}
