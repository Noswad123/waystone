# waystone

`waystone` saves useful paths, fuzzy-picks them later, and opens or copies the selected path.

It is part of the Jamal Arcana ecosystem, but it does not require any other Jamal Arcana tool. Floating workflows are composed with `wisp`:

```bash
wisp waystone nvim
```

## Install

```bash
./install.sh
```

By default this installs to `~/.local/bin`. Override with:

```bash
WAYSTONE_BIN_DIR=/usr/local/bin ./install.sh
```

## Usage

```bash
waystone add ~/Projects/app app
waystone list
waystone pick
waystone nvim
waystone open less
```

## State

By default, registry state lives at:

```text
${XDG_STATE_HOME:-~/.local/state}/waystone/paths.tsv
```

Override with:

```bash
WAYSTONE_FILE=/path/to/paths.tsv waystone list
```

## Dependencies

- `fzf`
- macOS `pbcopy` for clipboard support
- optional openers like `nvim`, `less`, `bat`, `yazi`

## License

MIT © Jamal Dawson
