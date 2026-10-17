"use client";

import { type FormEvent, type ReactNode, useState, useSyncExternalStore } from "react";
import { errorMessage } from "@/lib/api";
import { serverApi, type Session } from "@/lib/server-api";
import { Brand } from "./brand";
import { Button, ErrorText, Field } from "./ui";

type Props = { onDone: (session: Session) => void };

export function LogIn({ onDone, intro }: Props & { intro?: ReactNode }) {
  return (
    <AccountForm title="Log in" action="Log in" path="/account/login" intro={intro} onDone={onDone}>
      <Field fill required label="Username" name="name" autoComplete="username" />
      <Field
        fill
        required
        label="Password"
        name="password"
        type="password"
        autoComplete="current-password"
      />
    </AccountForm>
  );
}

export function Join({ invite, onDone }: Props & { invite: string }) {
  return (
    <AccountForm
      title="Create your account"
      action="Create account"
      path="/account/accept"
      extra={{ invite }}
      onDone={onDone}
    >
      <Field
        fill
        required
        label="Username"
        name="name"
        autoComplete="username"
        minLength={2}
        maxLength={30}
        pattern="[A-Za-z0-9._\-]+"
        title="Letters, digits, dots, hyphens and underscores, no spaces"
      />
      <Field
        fill
        required
        label="Password"
        name="password"
        type="password"
        autoComplete="new-password"
        minLength={10}
        title="At least 10 characters"
      />
      <Field
        fill
        required
        label="Confirm password"
        name="confirm"
        type="password"
        autoComplete="new-password"
      />
    </AccountForm>
  );
}

type FormProps = Props & {
  title: string;
  intro?: ReactNode;
  action: string;
  path: string;
  extra?: Record<string, string>;
  children: ReactNode;
};

function AccountForm({ title, intro, action, path, extra, onDone, children }: FormProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    if (form.has("confirm") && form.get("confirm") !== form.get("password")) {
      setError("The passwords do not match.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const session = await serverApi<Session>(path, {
        ...extra,
        name: String(form.get("name") ?? ""),
        password: String(form.get("password") ?? ""),
      });
      onDone(session);
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  }

  return (
    <Card>
      <form onSubmit={submit} className="grid gap-4">
        <div className="grid gap-1">
          <h1 className="text-lg font-semibold">{title}</h1>
          {intro && <p className="text-muted">{intro}</p>}
        </div>
        {children}
        <Button type="submit" variant="primary" size="large" disabled={busy}>
          {action}
        </Button>
        {error && <ErrorText>{error}</ErrorText>}
      </form>
    </Card>
  );
}

export function Card({ children }: { children: ReactNode }) {
  const host = useSyncExternalStore(
    () => () => {},
    () => window.location.host,
    () => "",
  );

  return (
    <main className="grid min-h-screen place-items-center px-4 py-10">
      <div className="grid w-full max-w-sm gap-6">
        <header className="grid justify-items-center gap-1 text-center">
          <Brand className="text-2xl text-[#10497e] dark:text-white" />
          <p className="text-xs text-muted">{host}</p>
        </header>
        <div className="sm:rounded-xl sm:border sm:border-line sm:p-8">{children}</div>
      </div>
    </main>
  );
}
