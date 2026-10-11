"use client";

import { useEffect, useState } from "react";
import { Join, LogIn } from "@/components/account-forms";
import { PeopleView } from "@/components/people-view";
import { ServerView } from "@/components/server-view";
import { Button } from "@/components/ui";
import { type Person, serverApi } from "@/lib/server-api";

const NEUTRAL = "#1f1f23";
const VIEWS = ["People", "Server"] as const;

export default function Server() {
  // `undefined` until the server has said whether this browser is logged in.
  const [person, setPerson] = useState<Person | null | undefined>(undefined);
  const [view, setView] = useState<(typeof VIEWS)[number]>("People");
  const [invite, setInvite] = useState(() =>
    typeof window === "undefined"
      ? null
      : new URLSearchParams(window.location.search).get("invite"),
  );

  useEffect(() => {
    serverApi<{ person: Person }>("/me").then(
      (answer) => setPerson(answer.person),
      () => setPerson(null),
    );
  }, []);

  function joined(person: Person) {
    // The invite is used up, so it should not stay in the address.
    window.history.replaceState(null, "", window.location.pathname);
    setInvite(null);
    setPerson(person);
  }

  async function logOut() {
    await serverApi("/logout", {}).catch(() => {});
    setPerson(null);
  }

  if (person === undefined) return null;
  if (!person) {
    return invite ? <Join invite={invite} onDone={joined} /> : <LogIn onDone={setPerson} />;
  }

  return (
    <div className="flex h-screen overflow-hidden">
      <aside
        className="flex w-60 shrink-0 flex-col overflow-y-auto text-white"
        style={{ backgroundColor: NEUTRAL }}
      >
        <p className="flex h-14 shrink-0 items-center bg-black/25 px-4 text-lg tracking-tight">
          <span
            aria-hidden
            className="mr-2 size-7 bg-current [mask:url(/icon.svg)_center/contain_no-repeat]"
          />
          <span className="font-light">uni</span>
          <span className="font-bold">MCP</span>
        </p>

        {person.owner && (
          <nav className="mt-4 grid gap-0.5 px-3">
            {VIEWS.map((name) => (
              <button
                key={name}
                type="button"
                aria-current={view === name ? "page" : undefined}
                onClick={() => setView(name)}
                className={`flex h-8 items-center rounded-md px-2 text-left ${
                  view === name
                    ? "bg-white/15 font-medium"
                    : "text-white/75 hover:bg-white/10 hover:text-white"
                }`}
              >
                {name}
              </button>
            ))}
          </nav>
        )}

        <div className="mt-auto flex items-center justify-between gap-2 px-4 py-4">
          <span className="truncate text-white/75">{person.name}</span>
          <Button onClick={logOut}>Log out</Button>
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center border-b border-line px-10">
          <h1 className="text-lg font-semibold">{person.owner ? view : person.name}</h1>
        </header>
        <main className="flex-1 overflow-auto px-10 py-8">
          <div className="max-w-4xl">
            {person.owner && (view === "People" ? <PeopleView /> : <ServerView />)}
          </div>
        </main>
      </div>
    </div>
  );
}
