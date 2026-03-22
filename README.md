# clh-client

CLI client for [clh-server](https://github.com/okkez/clh-server) — fuzzy-search your shell command history across machines.

Uses [skim](https://github.com/skim-rs/skim) as the fuzzy finder to interactively filter records stored in clh-server.

## Features

- **Fuzzy search** — powered by skim (fzf-compatible TUI)
- **Auto dedup** — same command appears only once (most recently used wins)
- **Streaming display** — skim opens immediately while records are still being fetched
- **Auto pagination** — fetches all records transparently (supports `X-Total-Count` from server)
- **zsh / bash / fish integration** — auto-record every command and bind `Ctrl+S` / `Ctrl+T` to search
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

[add]
# Regular expressions — matching commands are not recorded (optional)
# ignore_patterns = ["^ls", "^cd ", "^pwd$", "^secret"]
```

### 2. Add shell integration

**zsh** — add to `~/.zshrc`:

```zsh
eval "$(clh setup)"
```

**bash** — add to `~/.bashrc`:

```bash
eval "$(clh setup)"
```

**fish** — add to `~/.config/fish/config.fish`:

```fish
clh setup | source
```

This sets up:
- **Auto-recording**: every command you run is silently POSTed to clh-server in the background
- **Ctrl+S binding**: fuzzy search filtered to the **current directory** — the selected command is pasted into your prompt
- **Ctrl+T binding**: fuzzy search across **all history** — the selected command is pasted into your prompt

## Usage

| Command | Description |
|---------|-------------|
| `clh` | Fuzzy search history (same as `clh search`) |
| `clh search` | Fuzzy search all history |
| `clh search --pwd PATH` | Fuzzy search filtered to a specific directory |
| `clh add --hostname H --pwd P --command C` | Record a command (called by shell hook automatically) |
| `clh setup [--shell zsh\|bash\|fish]` | Print shell integration script |
| `clh config show` | Show current config path and contents |
| `clh config init --url URL [--user U --password P]` | Create initial config |

### Keyboard shortcuts in skim

| Key | Action |
|-----|--------|
| `Enter` | Accept — pastes selected command into prompt |
| `Esc` / `Ctrl+C` | Cancel |
| `Ctrl+D` | Delete the selected record from the server, then continue searching |
| `Ctrl+P` / `↑` | Move up |
| `Ctrl+N` / `↓` | Move down |

> **Note on `Ctrl+D` deletion:**
> - In directory search (`Ctrl+S`): deletes **all** server records with that command name (scoped to the current directory's records).
> - In global search (`Ctrl+T`): deletes **only the single displayed record** to avoid accidentally removing history used in other directories.

## Server-side pagination

clh-client automatically handles pagination:

- If the server returns an `X-Total-Count` response header, records are fetched page by page (configurable via `page_size`).
- If the header is absent (older server), a single request with `limit=<page_size>` is made.

To enable full pagination support, add `limit`/`offset` query parameters and `X-Total-Count` header to clh-server. See [clh-server-gaps.md](../clh-server-gaps.md) for implementation notes.

## License

MIT
