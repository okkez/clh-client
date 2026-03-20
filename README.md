# clh-client

CLI client for [clh-server](https://github.com/okkez/clh-server) — fuzzy-search your shell command history across machines.

Uses [skim](https://github.com/skim-rs/skim) as the fuzzy finder to interactively filter records stored in clh-server, replacing the default `Ctrl+R` with a server-backed, multi-host history search.

## Features

- **Fuzzy search** — powered by skim (fzf-compatible TUI)
- **Auto dedup** — same command appears only once (most recently used wins)
- **Auto pagination** — fetches all records transparently (supports `X-Total-Count` from server)
- **zsh integration** — auto-record every command and bind `Ctrl+R` to search
- **Basic auth** — compatible with Nginx-protected clh-server instances

## Requirements

- Rust 1.80+ (for `cargo install`)
- A running [clh-server](https://github.com/okkez/clh-server) instance

## Installation

```sh
cargo install --git https://github.com/okkez/clh-client
```

Or build locally:

```sh
git clone https://github.com/okkez/clh-client
cd clh-client
cargo build --release
# binary is at target/release/clh
```

## Setup

### 1. Create config

```sh
clh config init \
  --url https://clh.example.com \
  --user youruser \
  --password yourpassword
```

Config is stored at `~/.config/clh/config.toml`:

```toml
[server]
url = "https://clh.example.com"
basic_auth_user = "youruser"
basic_auth_password = "yourpassword"

[search]
# hostname = "my-machine"  # filter to one host (optional)
page_size = 1000
dedup = true

[add]
# Regular expressions — matching commands are not recorded (optional)
# ignore_patterns = ["^ls", "^cd ", "^pwd$", "^secret"]
```

### 2. Add zsh integration

Add the following to your `~/.zshrc`:

```zsh
eval "$(clh setup)"
```

This sets up:
- **Auto-recording**: every command you run is silently POSTed to clh-server in the background
- **Ctrl+R binding**: opens skim fuzzy search; the selected command is pasted into your prompt (not executed immediately)

## Usage

| Command | Description |
|---------|-------------|
| `clh` | Fuzzy search history (same as `clh search`) |
| `clh search` | Fuzzy search history |
| `clh add --hostname H --pwd P --command C` | Record a command (called by zsh hook automatically) |
| `clh setup` | Print zsh integration script |
| `clh config show` | Show current config path and contents |
| `clh config init --url URL [--user U --password P]` | Create initial config |

### Keyboard shortcuts in skim

| Key | Action |
|-----|--------|
| `Enter` | Accept — pastes command into prompt |
| `Esc` / `Ctrl+C` | Cancel |
| `Ctrl+P` / `↑` | Move up |
| `Ctrl+N` / `↓` | Move down |

## Server-side pagination

clh-client automatically handles pagination:

- If the server returns an `X-Total-Count` response header, records are fetched page by page (configurable via `page_size`).
- If the header is absent (older server), a single request with `limit=<page_size>` is made.

To enable full pagination support, add `limit`/`offset` query parameters and `X-Total-Count` header to clh-server. See [clh-server-gaps.md](../clh-server-gaps.md) for implementation notes.

## License

MIT
