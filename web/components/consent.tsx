"use client";

import { type ReactNode, useEffect, useState } from "react";
import { errorMessage } from "@/lib/api";
import { serverApi, type Session } from "@/lib/server-api";
import { Card, LogIn } from "./account-forms";
import { Button, ErrorText } from "./ui";

export type Request = {
  clientId: string;
  redirectUri: string;
  codeChallenge: string;
  codeChallengeMethod: string | null;
  state: string | null;
};

export function readRequest(search: string): Request | null {
  const query = new URLSearchParams(search);
  const clientId = query.get("client_id");
  const redirectUri = query.get("redirect_uri");
  const codeChallenge = query.get("code_challenge");
  if (query.get("response_type") !== "code" || !clientId || !redirectUri || !codeChallenge) {
    return null;
  }
  return {
    clientId,
    redirectUri,
    codeChallenge,
    codeChallengeMethod: query.get("code_challenge_method"),
    state: query.get("state"),
  };
}

const READS = [
  "Your courses, their files and assignments",
  "Your timetable",
  "Your cloud storage",
  "Canteen menus",
];

export function Consent({ request }: { request: Request | null }) {
  const [app, setApp] = useState<string | null | undefined>(undefined);
  const [session, setSession] = useState<Session | null | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!request) return;
    const query = new URLSearchParams({
      client_id: request.clientId,
      redirect_uri: request.redirectUri,
    });
    serverApi<{ name: string }>(`/oauth/app?${query}`).then(
      (found) => setApp(found.name),
      () => setApp(null),
    );
    serverApi<Session>("/account/me").then(setSession, () => setSession(null));
  }, [request]);

  async function answer(allow: boolean) {
    if (!request) return;
    setBusy(true);
    setError(null);
    try {
      const { redirect } = await serverApi<{ redirect: string }>("/oauth/answer", {
        client_id: request.clientId,
        redirect_uri: request.redirectUri,
        code_challenge: request.codeChallenge,
        code_challenge_method: request.codeChallengeMethod,
        state: request.state,
        allow,
      });
      window.location.assign(redirect);
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  }

  async function logOut() {
    await serverApi("/account/logout", {}).catch(() => {});
    setSession(null);
  }

  if (!request) {
    return (
      <Card>
        <Problem>This link is not complete. Start connecting again from your AI app.</Problem>
      </Card>
    );
  }
  if (app === undefined || session === undefined) return null;
  if (app === null) {
    return (
      <Card>
        <Problem>
          This server does not know the app that sent you here. Start connecting again from your AI
          app.
        </Problem>
      </Card>
    );
  }
  if (!session) {
    return <LogIn intro={`to connect ${app} to your account`} onDone={setSession} />;
  }
  if (session.person.admin) {
    return (
      <Card>
        <div className="grid gap-4">
          <Problem>The admin account cannot connect apps. Log in with your own account.</Problem>
          <Button size="large" onClick={logOut}>
            Log out
          </Button>
        </div>
      </Card>
    );
  }

  return (
    <Card>
      <div className="grid gap-5">
        <div className="grid gap-1">
          <h1 className="text-lg font-semibold">Connect {app}</h1>
          <p className="text-muted">
            {app} wants to use uniMCP with your account{" "}
            <span className="font-medium text-ink">{session.person.name}</span>.
          </p>
        </div>
        <div className="grid gap-2">
          <p className="font-medium">It will be able to read:</p>
          <ul className="grid list-disc gap-1 pl-5">
            {READS.map((read) => (
              <li key={read}>{read}</li>
            ))}
          </ul>
          <p className="text-muted">
            It cannot change, send or delete anything, and you can disconnect it on your dashboard
            at any time.
          </p>
        </div>
        <div className="grid gap-2 sm:grid-cols-2">
          <Button size="large" variant="primary" disabled={busy} onClick={() => answer(true)}>
            Allow
          </Button>
          <Button size="large" disabled={busy} onClick={() => answer(false)}>
            Deny
          </Button>
        </div>
        {error && <ErrorText>{error}</ErrorText>}
        <p className="text-center text-xs text-muted">
          Not {session.person.name}?{" "}
          <button type="button" className="underline" onClick={logOut}>
            Log out
          </button>
        </p>
      </div>
    </Card>
  );
}

function Problem({ children }: { children: ReactNode }) {
  return (
    <div className="grid gap-1">
      <h1 className="text-lg font-semibold">Cannot connect</h1>
      <p className="text-muted">{children}</p>
    </div>
  );
}
