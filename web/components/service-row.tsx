"use client";

import { type FormEvent, useEffect, useState } from "react";
import { api, errorMessage, type MoodleProgress, type Service } from "@/lib/api";
import { ActionCell, Button, Cell, DetailRow, ErrorText, Field, Status } from "./ui";

const POLL_MS = 1500;
const SIGN_IN_TIMEOUT_MS = 5 * 60 * 1000;

export const SERVICE_COLUMNS = ["Service", "Login", "Status", ""];

const LOGIN: Record<Service["kind"], string> = {
  moodle: "Browser sign-in",
  webdav: "App password",
  calendar: "iCal link",
};

type Props = {
  service: Service;
  browserSignIn: boolean;
  onChanged: () => void;
};

export function ServiceRow({ service, browserSignIn, onChanged }: Props) {
  const name = service.name;
  const [mode, setMode] = useState<"idle" | "form" | "waiting" | "busy">("idle");
  const [error, setError] = useState<string | null>(service.error);
  const connected = service.logged_in && !service.expired && !service.checking;

  useEffect(() => {
    if (mode !== "waiting") return;
    const started = Date.now();
    const timer = setInterval(async () => {
      try {
        const progress = await api<MoodleProgress>("/login/moodle");
        if (progress.state === "done") {
          setMode("idle");
          onChanged();
        } else if (progress.state === "failed") {
          setMode("idle");
          setError(progress.message);
        } else if (Date.now() - started > SIGN_IN_TIMEOUT_MS) {
          setMode("idle");
          setError("Sign-in timed out.");
        }
      } catch (e) {
        setMode("idle");
        setError(errorMessage(e));
      }
    }, POLL_MS);
    return () => clearInterval(timer);
  }, [mode, onChanged]);

  async function run(action: () => Promise<unknown>, then: "idle" | "waiting" = "idle") {
    setError(null);
    setMode("busy");
    try {
      await action();
      setMode(then);
      if (then === "idle") onChanged();
    } catch (e) {
      setMode("idle");
      setError(errorMessage(e));
    }
  }

  function connect() {
    if (service.kind !== "moodle") {
      setError(null);
      setMode("form");
    } else if (browserSignIn) {
      run(() => api("/login/moodle", {}), "waiting");
    } else {
      setError(`Run "unimcp login ${service.id}" in a terminal.`);
    }
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const field = (name: string) => String(form.get(name) ?? "");
    run(() =>
      service.kind === "calendar"
        ? api("/login/calendar", { link: field("link") })
        : api("/login/webdav", { user: field("user"), password: field("password") }),
    );
  }

  return (
    <>
      <tr>
        <Cell className="font-medium">{name}</Cell>
        <Cell className="whitespace-nowrap text-muted">{LOGIN[service.kind]}</Cell>
        <Cell>
          {mode === "waiting" ? (
            <Status ok={false}>Signing in</Status>
          ) : (
            <Status ok={connected}>
              {!service.logged_in
                ? "Not connected"
                : service.expired
                  ? "Sign-in expired"
                  : service.checking
                    ? "Checking"
                    : "Connected"}
            </Status>
          )}
        </Cell>
        <ActionCell>
          {mode === "waiting" || mode === "form" ? (
            <Button onClick={() => setMode("idle")}>Cancel</Button>
          ) : service.expired ? (
            <Button variant="primary" disabled={mode === "busy"} onClick={connect}>
              Reconnect
            </Button>
          ) : service.logged_in ? (
            <Button
              disabled={mode === "busy"}
              onClick={() => run(() => api("/logout", { service: service.id }))}
            >
              Disconnect
            </Button>
          ) : (
            <Button variant="primary" disabled={mode === "busy"} onClick={connect}>
              Connect
            </Button>
          )}
        </ActionCell>
      </tr>

      {mode === "waiting" && (
        <DetailRow columns={SERVICE_COLUMNS.length}>
          <p className="text-muted">
            Complete the sign-in in your browser, then allow it to open uniMCP.
          </p>
        </DetailRow>
      )}

      {mode === "form" && (
        <DetailRow columns={SERVICE_COLUMNS.length}>
          <form onSubmit={submit} className="flex flex-wrap items-end gap-3">
            {service.kind === "calendar" ? (
              <Field
                required
                label="iCal link"
                name="link"
                type="password"
                autoComplete="off"
                title="The link you receive when you publish your university calendar"
              />
            ) : (
              <>
                <Field required label="Username" name="user" autoComplete="username" />
                <Field
                  required
                  label="App password"
                  name="password"
                  type="password"
                  autoComplete="off"
                  title="Not your university password"
                />
              </>
            )}
            <Button type="submit" variant="primary">
              Connect
            </Button>
          </form>
        </DetailRow>
      )}

      {error && (
        <DetailRow columns={SERVICE_COLUMNS.length}>
          <ErrorText>{error}</ErrorText>
        </DetailRow>
      )}
    </>
  );
}
