"use client";

import { useEffect, useState } from "react";
import { errorMessage } from "@/lib/api";
import { serverApi } from "@/lib/server-api";
import { ActionCell, Button, Cell, ErrorText, Status, Table } from "./ui";

type Autostart = { on: boolean; moment: "boot" | "login" | null; note: string | null };

const WHEN = {
  boot: "Starts when this computer starts.",
  login: "Starts when you log in to this computer.",
};

export function ServerView() {
  const [autostart, setAutostart] = useState<Autostart | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    serverApi<{ autostart: Autostart }>("/server").then(
      (settings) => setAutostart(settings.autostart),
      (e) => setError(errorMessage(e)),
    );
  }, []);

  async function turn(on: boolean) {
    setBusy(true);
    setError(null);
    try {
      const settings = await serverApi<{ autostart: Autostart }>("/server/autostart", { on });
      setAutostart(settings.autostart);
    } catch (e) {
      setError(errorMessage(e));
    }
    setBusy(false);
  }

  return (
    <div className="grid gap-4">
      {autostart && (
        <Table>
          <tr>
            <Cell className="font-medium">Start with this computer</Cell>
            <Cell>
              {autostart.moment ? (
                <Status ok={autostart.on}>{autostart.on ? "On" : "Off"}</Status>
              ) : (
                <span className="text-muted">Not available on this system</span>
              )}
            </Cell>
            <ActionCell>
              {autostart.moment && (
                <Button
                  variant={autostart.on ? "secondary" : "primary"}
                  disabled={busy}
                  onClick={() => turn(!autostart.on)}
                >
                  {autostart.on ? "Turn off" : "Turn on"}
                </Button>
              )}
            </ActionCell>
          </tr>
        </Table>
      )}
      {autostart?.on && autostart.moment && (
        <p className="text-muted">{autostart.note ?? WHEN[autostart.moment]}</p>
      )}
      {error && <ErrorText>{error}</ErrorText>}
    </div>
  );
}
