"use client";

import { useState } from "react";
import { api, errorMessage } from "@/lib/api";
import { Button, ErrorText } from "./ui";

type Props = { saved: string; limit: number; onSaved: () => void };

export function CustomPromptView({ saved, limit, onSaved }: Props) {
  const [text, setText] = useState(saved);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const unchanged = text.trim() === saved;

  async function save() {
    setSaving(true);
    setError(null);
    try {
      await api("/custom-prompt", { custom_prompt: text });
      onSaved();
    } catch (e) {
      setError(errorMessage(e));
    }
    setSaving(false);
  }

  return (
    <div className="grid gap-3">
      <label className="grid gap-2">
        <span className="text-muted">
          What every AI app should know about you and your studies. Applies after restarting the AI
          app.
        </span>
        <textarea
          value={text}
          onChange={(event) => setText(event.target.value)}
          maxLength={limit}
          rows={10}
          placeholder="My examination regulations are in my cloud storage under Studium/PO.pdf."
          className="rounded-md border border-line bg-canvas px-3 py-2.5 leading-6 text-ink outline-none focus:border-ink"
        />
      </label>
      <div className="flex items-center justify-between gap-4">
        <span className="text-xs tabular-nums text-muted">
          {text.length}/{limit}
        </span>
        <Button variant="primary" onClick={save} disabled={saving || unchanged}>
          {unchanged ? "Saved" : "Save"}
        </Button>
      </div>
      {error && <ErrorText>{error}</ErrorText>}
    </div>
  );
}
