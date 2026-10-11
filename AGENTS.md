# uniMCP

A local, read-only MCP server that gives AI apps access to a student's
university services: courses, files and timetable.

## Rules

- **Read-only.** No tool may change, send, submit or delete anything.
- **Secrets stay in the OS keyring.** Never in files, logs, error messages or
  tool output, and never asked for in the chat.
- **Logins only go to the university's own hosts**, as named in its profile.
- **Nothing about one university in code.** That belongs in `profiles/*.toml`.
- **Official interfaces only.** No scraping.

## Layout

- `crates/uni-core`: traits, data types, profiles, secret storage
- `crates/uni-moodle`, `uni-webdav`, `uni-ical`, `uni-openmensa`: one
  connector per service
- `crates/uni-mcp`: the `unimcp` binary (MCP tools, commands, dashboard backend)
- `web`: the dashboard, see `web/AGENTS.md` and `web/README.md`
- `profiles`: one file per university

## Working here

- Before finishing: `cargo fmt --all`, `cargo clippy --all-targets`,
  `cargo test`, and in `web`: `npm run format`, `npm run lint`, `npm test`,
  `npm run build`.
- Work on a branch, never on `main`.
- Use Conventional Commits.
- Few comments: only where the reason is not obvious from the code.

## Versions

The version is set once, in the root `Cargo.toml`, and follows Semantic
Versioning (`MAJOR.MINOR.PATCH`):

- **Patch** (0.2.0 to 0.2.1): only `fix` commits since the last release.
- **Minor** (0.2.0 to 0.3.0): at least one `feat` commit.
- **Major** (1.0.0 to 2.0.0): a breaking change, marked with `!` as in `feat!:`.
  While the version starts with 0, a breaking change is a minor step.

A new version on `main` is released automatically, so only change it when a
release is wanted.
