# uni-mcp

Give your AI app read-only access to your university: your courses, your files
and your timetable. It runs on your own computer as an
[MCP](https://modelcontextprotocol.io) server.

Unofficial, and not affiliated with any university.

## What it can do

| Tool                | What it reads                                    |
| ------------------- | ------------------------------------------------ |
| `list_courses`      | The courses you are enrolled in                  |
| `list_course_files` | Slides, exercise sheets and other course files   |
| `list_assignments`  | Assignments and their deadlines                  |
| `read_course_file`  | A plain-text course file                         |
| `list_events`       | Your timetable: lectures, exercises, exam dates  |
| `list_files`        | A folder in your cloud storage                   |
| `read_file`         | A plain-text file from your cloud storage        |
| `status`            | Which services are connected                     |

PDF files cannot be read yet.

## Safety

- **Read-only.** No tool can change, send, submit or delete anything.
- **Local.** Your data goes from the university straight to your computer.
- **No university password.** Moodle signs you in on the university's own page,
  cloud storage uses an app password, the calendar a link you publish yourself.
- **Logins live in your system keyring**, and never pass through the AI.

## Universities

| University             | Courses | Cloud storage | Timetable  |
| ---------------------- | ------- | ------------- | ---------- |
| RWTH Aachen University | Moodle  | Sciebo        | RWTHonline |

A university is one file in [`profiles`](profiles). To add yours, copy
`rwth.toml`, change the addresses, and open a pull request.

## Getting started

There are no downloads yet, so it is built from source. You need
[Rust](https://rustup.rs) and [Node.js](https://nodejs.org).

```sh
cd web && npm install && npm run build && cd ..
cargo build --release
./target/release/unimcp
```

`unimcp` opens a page in your browser where you connect your services, try the
tools, and get the line that adds it to your AI app.

Browser sign-in for Moodle works on Windows only for now; elsewhere
`unimcp login moodle` asks you to paste a link.

## Development

See [`web/README.md`](web/README.md) for working on the page and
[`AGENTS.md`](AGENTS.md) for the rules of the project.

```sh
cargo fmt --all && cargo clippy --all-targets && cargo test
```

## License

Copyright © 2026 Zahraa Bou Zeid. Licensed under the
[GNU Affero General Public License v3.0](LICENSE).
