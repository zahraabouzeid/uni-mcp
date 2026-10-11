"use client";

import { useState } from "react";
import { api, errorMessage } from "@/lib/api";
import { Button, ErrorText } from "./ui";

const PLACEHOLDER =
  "I study mechanical engineering in my 3rd semester.\n" +
  "My examination regulations are in my cloud storage under Studies/Regulations.pdf.";

type Props = { saved: string; limit: number; onSaved: () => void };

export function SettingsView({ saved, limit, onSaved }: Props) {
  const [text, setText] = useState(saved);
  const [saving, setSaving] = useState(false);
  const [justSaved, setJustSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const unchanged = text.trim() === saved;

  async function save() {
    setSaving(true);
    setError(null);
    try {
      await api("/custom-instructions", { custom_instructions: text });
      setJustSaved(true);
      onSaved();
    } catch (e) {
      setError(errorMessage(e));
    }
    setSaving(false);
  }

  return (
    <div className="grid gap-3">
      <label className="grid gap-2">
        <span className="font-medium">Custom instructions</span>
        <textarea
          value={text}
          onChange={(event) => {
            setText(event.target.value);
            setJustSaved(false);
          }}
          maxLength={limit}
          rows={10}
          placeholder={PLACEHOLDER}
          className="rounded-md border border-line bg-canvas px-3 py-2.5 leading-6 text-ink outline-none focus:border-ink"
        />
      </label>
      <div className="flex items-center justify-between gap-4">
        <span className="text-xs tabular-nums text-muted">
          {text.length} / {limit}
        </span>
        <span className="flex items-center gap-3">
          {justSaved && <span className="text-muted">Saved. Takes effect after a restart.</span>}
          <Button variant="primary" onClick={save} disabled={saving || unchanged}>
            Save
          </Button>
        </span>
      </div>
      {error && <ErrorText>{error}</ErrorText>}
    </div>
  );
}
