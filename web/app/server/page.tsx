"use client";

import { useEffect, useState } from "react";
import { Join, LogIn } from "@/components/account-forms";
import { AdminConsole } from "@/components/admin-console";
import { Console } from "@/components/console";
import { Button, ErrorText } from "@/components/ui";
import { api, errorMessage } from "@/lib/api";
import { serverApi, type Session } from "@/lib/server-api";

function fromAddress(name: string) {
  return typeof window === "undefined"
    ? null
    : new URLSearchParams(window.location.search).get(name);
}

function clearAddress() {
  window.history.replaceState(null, "", window.location.pathname);
}

export default function Server() {
  // `undefined` until the server has said whether this browser is logged in.
  const [session, setSession] = useState<Session | null | undefined>(undefined);
  const person = session?.person;
  const [invite, setInvite] = useState(() => fromAddress("invite"));
  // The link Moodle sends back after a sign-in, where the browser passes it on.
  const [moodleLink, setMoodleLink] = useState(() => fromAddress("moodle"));
  const [moodleError, setMoodleError] = useState<string | null>(null);

  useEffect(() => {
    serverApi<Session>("/account/me").then(setSession, () => setSession(null));
  }, []);

  useEffect(() => {
    if (!person || person.admin || !moodleLink) return;
    api("/login/moodle/link", { link: moodleLink }).then(
      () => {
        clearAddress();
        setMoodleLink(null);
      },
      (e) => setMoodleError(errorMessage(e)),
    );
  }, [person, moodleLink]);

  function joined(session: Session) {
    // The invite is used up, so it should not stay in the address.
    clearAddress();
    setInvite(null);
    setSession(session);
  }

  async function logOut() {
    await serverApi("/account/logout", {}).catch(() => {});
    setSession(null);
  }

  if (session === undefined) return null;
  if (!session || !person) {
    return invite ? <Join invite={invite} onDone={joined} /> : <LogIn onDone={setSession} />;
  }
  if (person.admin) {
    return <AdminConsole person={person} about={session.about} onLogOut={logOut} />;
  }
  if (moodleError) {
    return (
      <main className="grid min-h-screen place-items-center px-6">
        <div className="grid max-w-md gap-4">
          <ErrorText>{moodleError}</ErrorText>
          <Button
            onClick={() => {
              clearAddress();
              setMoodleLink(null);
              setMoodleError(null);
            }}
          >
            Back to the dashboard
          </Button>
        </div>
      </main>
    );
  }
  if (moodleLink) return null;

  return <Console account={{ person, onLogOut: logOut }} />;
}
