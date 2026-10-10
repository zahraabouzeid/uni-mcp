# Contributing

Thanks for wanting to help. uni-mcp handles students' logins and university
data, so a few rules are strict.

## Rules

1. **Safety first.** Every tool is read-only: nothing may change, send, submit
   or delete anything. Logins stay in the system keyring and never appear in
   files, logs, error messages or tool output. They are only ever sent to the
   university's own hosts.
2. **No hardcoding.** Nothing about one university belongs in code. Addresses,
   names and colours go in `profiles/*.toml`.
3. **Official interfaces only.** Web service APIs, WebDAV, iCal. No scraping.
4. **Tested.** Parsing is done in pure functions with unit tests on sample
   data. No test talks to a real service.

## Workflow

1. Create a branch from `main`, for example `feat/pdf-reading` or
   `fix/moodle-timeout`. Nothing is committed to `main` directly.
2. Write [Conventional Commits](https://www.conventionalcommits.org):
   `feat(moodle): list grades`, `fix(web): keep the sidebar fixed`,
   `docs: explain app passwords`.
3. Run the checks:

   ```sh
   cargo fmt --all && cargo clippy --all-targets && cargo test
   cd web && npm run format && npm run lint && npm test && npm run build
   ```

4. Open a pull request into `main` and say what you changed and how you
   tested it. The same checks run there automatically and have to pass.

## Adding a university

Copy [`profiles/rwth.toml`](profiles/rwth.toml), rename it, and fill in your
university's addresses. Leave out a section if your university does not have
that service. Check each service against your own account before opening the
pull request, and say in it which ones you tried.

## Releasing

Set the new version in the root `Cargo.toml`, merge that, then tag `main`:

```sh
git tag v0.2.0 && git push origin v0.2.0
```

The tag starts a build for Windows, Linux and macOS and publishes a release
with the binaries.

## Working on the code

- [`AGENTS.md`](AGENTS.md) has the layout of the repository.
- [`web/README.md`](web/README.md) explains the setup page and `npm run dev`.

By contributing you agree that your work is licensed under the
[AGPL-3.0](LICENSE), like the rest of the project.
