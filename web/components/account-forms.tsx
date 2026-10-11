"use client";

import { type FormEvent, type ReactNode, useState } from "react";
import { errorMessage } from "@/lib/api";
import { type Person, serverApi } from "@/lib/server-api";
import { Button, ErrorText, Field } from "./ui";

type Props = { onDone: (person: Person) => void };

export function LogIn({ onDone }: Props) {
  return (
    <AccountForm title="Log in" action="Log in" path="/login" onDone={onDone}>
      <Field required label="Username" name="name" autoComplete="username" />
      <Field
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
      path="/accept"
      extra={{ invite }}
      onDone={onDone}
    >
      <Field
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
        required
        label="Password"
        name="password"
        type="password"
        autoComplete="new-password"
        minLength={10}
        title="At least 10 characters"
      />
    </AccountForm>
  );
}

type FormProps = Props & {
  title: string;
  action: string;
  path: string;
  extra?: Record<string, string>;
  children: ReactNode;
};

function AccountForm({ title, action, path, extra, onDone, children }: FormProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setBusy(true);
    setError(null);
    try {
      const { person } = await serverApi<{ person: Person }>(path, {
        ...extra,
        name: String(form.get("name") ?? ""),
        password: String(form.get("password") ?? ""),
      });
      onDone(person);
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  }

  return (
    <main className="grid min-h-screen place-items-center px-6">
      <form onSubmit={submit} className="grid w-60 gap-4">
        <h1 className="text-lg font-semibold">{title}</h1>
        {children}
        <Button type="submit" variant="primary" disabled={busy}>
          {action}
        </Button>
        {error && <ErrorText>{error}</ErrorText>}
      </form>
    </main>
  );
}
