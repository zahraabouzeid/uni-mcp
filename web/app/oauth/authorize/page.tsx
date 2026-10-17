"use client";

import { useMemo, useSyncExternalStore } from "react";
import { Consent, readRequest } from "@/components/consent";

export default function Authorize() {
  const search = useSyncExternalStore(
    () => () => {},
    () => window.location.search,
    () => null,
  );
  const request = useMemo(() => (search === null ? undefined : readRequest(search)), [search]);

  if (request === undefined) return null;
  return <Consent request={request} />;
}
