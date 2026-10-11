import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { serverApi } from "@/lib/server-api";
import { Join, LogIn } from "./account-forms";
import { PeopleView } from "./people-view";

vi.mock("@/lib/server-api", async (original) => ({
  ...(await original<typeof import("@/lib/server-api")>()),
  serverApi: vi.fn(),
}));

const owner = { id: "a", name: "Zahraa", owner: true };
const member = { id: "b", name: "Bestie", owner: false };

afterEach(() => {
  cleanup();
  vi.mocked(serverApi).mockReset();
});

test("the owner sees everyone, and only members can be removed", async () => {
  vi.mocked(serverApi).mockResolvedValue({ people: [owner, member] });
  render(<PeopleView />);

  expect(await screen.findByText("Zahraa")).toBeTruthy();
  expect(screen.getByText("Owner")).toBeTruthy();
  expect(screen.getByText("Member")).toBeTruthy();
  expect(screen.getAllByRole("button", { name: "Remove" }).length).toBe(1);
});

test("removing someone asks once more before it happens", async () => {
  vi.mocked(serverApi).mockResolvedValue({ people: [owner, member] });
  const user = userEvent.setup();
  render(<PeopleView />);

  await user.click(await screen.findByRole("button", { name: "Remove" }));
  expect(serverApi).not.toHaveBeenCalledWith("/people/remove", expect.anything());

  await user.click(screen.getByRole("button", { name: "Confirm removal" }));
  expect(serverApi).toHaveBeenCalledWith("/people/remove", { id: "b" });
});

test("an invite is shown as a link to this server", async () => {
  vi.mocked(serverApi).mockImplementation(async (path) =>
    path === "/invites" ? { invite: "c0de" } : { people: [owner] },
  );
  const user = userEvent.setup();
  render(<PeopleView />);

  await user.click(await screen.findByRole("button", { name: "Invite someone" }));

  expect(await screen.findByText(`${window.location.origin}/server?invite=c0de`)).toBeTruthy();
  expect(screen.getByText("This link works once and for one week.")).toBeTruthy();
});

test("accepting an invite sends it with the chosen name and password", async () => {
  vi.mocked(serverApi).mockResolvedValue({ person: member });
  const onDone = vi.fn();
  const user = userEvent.setup();
  render(<Join invite="c0de" onDone={onDone} />);

  await user.type(screen.getByLabelText("Username"), "Bestie");
  await user.type(screen.getByLabelText("Password"), "correct horse battery");
  await user.click(screen.getByRole("button", { name: "Create account" }));

  expect(serverApi).toHaveBeenCalledWith("/accept", {
    invite: "c0de",
    name: "Bestie",
    password: "correct horse battery",
  });
  expect(onDone).toHaveBeenCalledWith(member);
});

test("a refused login shows why and stays on the form", async () => {
  vi.mocked(serverApi).mockRejectedValue(new Error("wrong name or password"));
  const onDone = vi.fn();
  const user = userEvent.setup();
  render(<LogIn onDone={onDone} />);

  await user.type(screen.getByLabelText("Username"), "Zahraa");
  await user.type(screen.getByLabelText("Password"), "not the password");
  await user.click(screen.getByRole("button", { name: "Log in" }));

  expect((await screen.findByRole("alert")).textContent).toBe("wrong name or password");
  expect(onDone).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Log in" })).toHaveProperty("disabled", false);
});
