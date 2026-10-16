"use client";

import { useCallback, useEffect, useState } from "react";
import { errorMessage } from "@/lib/api";
import { type Person, serverApi } from "@/lib/server-api";
import { ActionCell, Button, Cell, CodeBlock, ErrorText, Table } from "./ui";

const COLUMNS = ["Username", "Role", ""];

// The admin works on the server itself, so invites carry the address members use.
export function PeopleView({ publicUrl }: { publicUrl?: string | null }) {
  const [people, setPeople] = useState<Person[]>([]);
  const [link, setLink] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(
    () =>
      serverApi<{ people: Person[] }>("/people").then(
        (loaded) => setPeople(loaded.people),
        (e) => setError(errorMessage(e)),
      ),
    [],
  );

  useEffect(() => {
    load();
  }, [load]);

  async function invite() {
    setError(null);
    setCopied(false);
    try {
      const { invite } = await serverApi<{ invite: string }>("/invites", {});
      setLink(`${publicUrl ?? window.location.origin}/server?invite=${invite}`);
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  async function copy() {
    if (!link) return;
    await navigator.clipboard.writeText(link);
    setCopied(true);
  }

  async function remove(id: string) {
    if (confirming !== id) {
      setConfirming(id);
      return;
    }
    setConfirming(null);
    setError(null);
    try {
      await serverApi("/people/remove", { id });
    } catch (e) {
      setError(errorMessage(e));
    }
    load();
  }

  return (
    <div className="grid gap-6">
      <Table columns={COLUMNS}>
        {people.map((person) => (
          <tr key={person.id}>
            <Cell className="font-medium">{person.name}</Cell>
            <Cell className="text-muted">{person.admin ? "Admin" : "Member"}</Cell>
            <ActionCell>
              {!person.admin && (
                <Button onClick={() => remove(person.id)}>
                  {confirming === person.id ? "Confirm removal" : "Remove"}
                </Button>
              )}
            </ActionCell>
          </tr>
        ))}
      </Table>

      <div className="grid gap-2">
        <div className="flex items-center justify-between gap-4">
          <span className="text-muted">{link && "This link works once and for one week."}</span>
          <span className="flex gap-2">
            {link && <Button onClick={copy}>{copied ? "Copied" : "Copy"}</Button>}
            <Button variant="primary" onClick={invite}>
              Invite someone
            </Button>
          </span>
        </div>
        {link && <CodeBlock code={link} className="whitespace-pre-wrap break-all bg-panel" />}
      </div>

      {error && <ErrorText>{error}</ErrorText>}
    </div>
  );
}
