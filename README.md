<img src=".github/logo.svg" alt="" width="72">

# uniMCP

I built uniMCP during my BSc. studies in
[Computational Engineering Science](https://www.ces.rwth-aachen.de/) at
[RWTH Aachen University](https://www.rwth-aachen.de/), so that an AI assistant
could help me with my own courses, files, timetable and even the canteen menu.

uniMCP is an [MCP](https://modelcontextprotocol.io) server that gives your AI
assistant read-only access to your university services. It runs locally on
your computer or on your own server, where several users can connect from any
device.

Unofficial, and not affiliated with any university.

![The dashboard](docs/screenshots/dashboard.png)

## Tools

![The tools on the dashboard](docs/screenshots/tools.png)

### Supported file formats

| Format                  | Read as                                         |
| ----------------------- | ----------------------------------------------- |
| PDF                     | Text, or pages as images for formulas and scans |
| Word, PowerPoint, Excel | Text and embedded images                        |
| Images                  | The image itself                                |
| Zip archives            | The file list and the files inside              |
| Plain text              | Text, such as code, Markdown or CSV             |

## Prompts

Prompt templates, available in clients that support MCP prompts:

| Prompt                | Arguments                    | Description                                   |
| --------------------- | ---------------------------- | --------------------------------------------- |
| My week               | None                         | This week's timetable and the work due in it  |
| Deadlines             | None                         | All open assignments, soonest first           |
| Explain a lecture     | `course`, `topic` (optional) | An explanation of the matching lecture slides |
| Help with an exercise | `course`, `sheet`            | Hints for an exercise sheet before a solution |
| Summarise a course    | `course`                     | An overview based on all course material      |

## Security and privacy

- **Read-only:** no tool can change, send or delete anything.
- **Self-hosted:** runs on a computer or server you control.
- **No university password:** Moodle uses the university's sign-in page, cloud
  storage an app password, the calendar a published link.
- **Encrypted credentials:** kept in the system keyring or encrypted on the
  server, never shown to the AI assistant.
- **Explicit consent:** on a server, an AI assistant only gets access after the
  user allows it.

## Supported universities

| University             | Courses | Cloud storage | Timetable  | Canteens  |
| ---------------------- | ------- | ------------- | ---------- | --------- |
| RWTH Aachen University | Moodle  | Sciebo        | RWTHonline | OpenMensa |

Each university is defined by a single file in [`profiles`](profiles). To add
another one, see [CONTRIBUTING.md](CONTRIBUTING.md).

## Getting started

Download uniMCP from the
[releases page](https://github.com/zahraabouzeid/uni-mcp/releases): the
`.msi` installer on Windows, or the single `unimcp` file on any platform.

Start it and the dashboard opens in the browser. Connect the university services there
and add uniMCP to Claude Desktop, Claude Code, Codex or Gemini CLI with one
click.

### Running on a server

In server mode, one uniMCP serves several people, and each connects Claude,
ChatGPT and other AI assistants from any device. See
[deploy/README.md](deploy/README.md) for running it with Docker.

![Allowing an AI assistant to connect](docs/screenshots/consent.png)

### Building from source

Requires [Rust](https://rustup.rs) and [Node.js](https://nodejs.org).

```sh
cd web && npm install && npm run build && cd ..
cargo build --release
./target/release/unimcp
```

## Contributing

Contributions are welcome, especially new universities. The rules and the
workflow are described in [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Copyright © 2026 Zahraa Bou Zeid. Licensed under the
[GNU Affero General Public License v3.0](LICENSE).
