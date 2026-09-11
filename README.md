# waystone

`waystone` saves useful paths, fuzzy-picks them later, and opens or copies the selected path.

It is part of the Jamal Arcana ecosystem, but it does not require any other Jamal Arcana tool. Floating workflows are composed with `wisp`:

```bash
wisp waystone nvim
```

## Install

Requires a Rust toolchain for source installs.

```bash
./install.sh
```

By default this installs to `~/.local/bin`. Override with:

```bash
WAYSTONE_BIN_DIR=/usr/local/bin ./install.sh
```

Completions install by default to:

```text
~/.local/share/zsh/site-functions/_waystone
~/.local/share/bash-completion/completions/waystone
```

Override or disable completion installation with:

```bash
WAYSTONE_ZSH_COMPLETION_DIR=/path/to/site-functions ./install.sh
WAYSTONE_BASH_COMPLETION_DIR=/path/to/bash-completion ./install.sh
WAYSTONE_INSTALL_COMPLETIONS=0 ./install.sh
```

## Usage

```bash
waystone add ~/Projects/app app
waystone list
waystone pick
waystone select
waystone select --action
waystone nvim
waystone open less
```

In the open picker, Waystone starts in selection mode instead of focusing search:

- `/` enters search mode; `Esc` returns to selection mode
- `Esc` quits from selection mode
- `?` opens the full keybinding help popup

The full keybinding popup includes mutations like `r` to rename a selected entry's label and/or path, and `n` to create a new entry. These open a small form where `Tab` moves between label and path, `Enter` saves, and `Esc` cancels. New entries require a path; when the label is left blank, Waystone uses the resolved path as the label.

`waystone select --action` is the machine-readable picker mode for editor adapters. It prints one TSV row:

```text
open<TAB>/path/to/file
edit<TAB>/path/to/file
edit-return<TAB>/path/to/file
```

## Neovim

An fzf-backed Neovim adapter is available at `contrib/nvim/waystone.lua`. Put it with your Neovim plugins/config, then load it:

```lua
require("waystone").setup()
```

Commands:

- `:Waystone` opens the Waystone picker in a floating terminal.
- `:WaystoneAddCurrent [label]` saves the current buffer path.

Inside Neovim, `Enter`/`e` opens the selected file in the current window. `E` adds the selected file to the buffer list and reopens Waystone so you can keep selecting files.

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

- Rust/Cargo to build from source
- `fzf`
- macOS `pbcopy` for clipboard support
- optional openers like `nvim`, `less`, `bat`, `yazi`

## License

MIT © Jamal Dawson
