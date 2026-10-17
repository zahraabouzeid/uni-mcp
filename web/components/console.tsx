"use client";

import { useCallback, useEffect, useState } from "react";
import { announceClosing, api, errorMessage, type DashboardState } from "@/lib/api";
import type { Person } from "@/lib/server-api";
import { ConnectView } from "./connect-view";
import { SERVICE_COLUMNS, ServiceRow } from "./service-row";
import { SettingsView } from "./settings-view";
import { SetupView } from "./setup-view";
import { type Entry, Shell } from "./shell";
import { TOOL_COLUMNS, ToolRow } from "./tool-row";
import { ActionCell, Button, Cell, ErrorText, Table } from "./ui";

const VIEWS = ["services", "tools", "setup", "settings"] as const;
type View = (typeof VIEWS)[number];

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
  const nav: Entry<View>[] = university
    ? [
        { view: "services", label: "Services", count: `${connected}/${state.services.length}` },
        { view: "tools", label: "Tools", count: String(state.tools.length) },
        // Without a command there is nothing to hand to an AI app yet.
        ...(state.command || hosted ? [{ view: "setup", label: "Setup" } as Entry<View>] : []),
        { view: "settings", label: "Settings" },
      ]
    : [{ view: "services", label: "University" }];
  const shown = nav.some((item) => item.view === view) ? view : "services";

  return (
    <Shell
      color={university?.color}
      nav={nav}
      shown={shown}
      onShow={(next) => {
        window.location.hash = next;
        setView(next);
      }}
      account={account && { name: account.person.name, onLogOut: account.onLogOut }}
      version={state.version}
      repository={state.repository}
      headerEnd={
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
      }
    >
      {error && (
        <div className="mb-6">
          <ErrorText>{error}</ErrorText>
        </div>
      )}

      {!university ? (
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
      ) : shown === "services" ? (
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
      ) : shown === "tools" ? (
        <Table columns={TOOL_COLUMNS}>
          {state.tools.map((tool) => (
            <ToolRow key={tool.name} tool={tool} />
          ))}
        </Table>
      ) : shown === "settings" ? (
        <SettingsView
          key={university.id}
          saved={state.custom_instructions}
          limit={state.custom_instructions_limit}
          onSaved={load}
        />
      ) : hosted ? (
        <ConnectView />
      ) : (
        state.command && <SetupView command={state.command} apps={state.apps} onChanged={load} />
      )}
    </Shell>
  );
}
