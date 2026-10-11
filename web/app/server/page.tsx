"use client";

import { useEffect, useState } from "react";
import { Join, LogIn } from "@/components/account-forms";
import { Console } from "@/components/console";
import { Button, ErrorText } from "@/components/ui";
import { api, errorMessage } from "@/lib/api";
import { type Person, serverApi } from "@/lib/server-api";

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
  const [person, setPerson] = useState<Person | null | undefined>(undefined);
  const [invite, setInvite] = useState(() => fromAddress("invite"));
  // The link Moodle sends back after a sign-in, where the browser passes it on.
  const [moodleLink, setMoodleLink] = useState(() => fromAddress("moodle"));
  const [moodleError, setMoodleError] = useState<string | null>(null);

  useEffect(() => {
    serverApi<{ person: Person }>("/account/me").then(
      (answer) => setPerson(answer.person),
      () => setPerson(null),
    );
  }, []);

  useEffect(() => {
    if (!person || !moodleLink) return;
    api("/login/moodle/link", { link: moodleLink }).then(
      () => {
        clearAddress();
        setMoodleLink(null);
      },
      (e) => setMoodleError(errorMessage(e)),
    );
  }, [person, moodleLink]);

  function joined(person: Person) {
    // The invite is used up, so it should not stay in the address.
    clearAddress();
    setInvite(null);
    setPerson(person);
  }

  async function logOut() {
    await serverApi("/account/logout", {}).catch(() => {});
    setPerson(null);
  }

  if (person === undefined) return null;
  if (!person) {
    return invite ? <Join invite={invite} onDone={joined} /> : <LogIn onDone={setPerson} />;
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
