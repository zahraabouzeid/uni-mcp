"use client";

import { useState } from "react";
import type { About, Person } from "@/lib/server-api";
import { PeopleView } from "./people-view";
import { ServerView } from "./server-view";
import { type Entry, Shell } from "./shell";

type View = "people" | "settings";

const NAV: Entry<View>[] = [
  { view: "people", label: "People" },
  { view: "settings", label: "Settings" },
];

type Props = { person: Person; about: About; onLogOut: () => void };

// The admin looks after the server and has no university of their own on it.
export function AdminConsole({ person, about, onLogOut }: Props) {
  const [view, setView] = useState<View>("people");

  return (
    <Shell
      nav={NAV}
      shown={view}
      onShow={setView}
      account={{ name: person.name, onLogOut }}
      version={about.version}
      repository={about.repository}
    >
      {view === "people" ? <PeopleView /> : <ServerView />}
    </Shell>
  );
}
