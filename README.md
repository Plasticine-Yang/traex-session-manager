# tsm

`tsm` (`traex-session-manager`) is a terminal UI for listing, searching,
deleting, archiving, and renaming traex CLI sessions.

## Install

Install the latest release on macOS or Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/Plasticine-Yang/traex-session-manager/main/install.sh | sh
```

The installer verifies the release SHA256 checksum, installs the binary at
`~/.local/bin/tsm`, and creates
`~/.local/bin/traex-session-manager -> tsm`. If `~/.local/bin` is not on
`PATH`, it prints the shell setup needed without modifying your profile.

To build and install from source instead:

```sh
cargo install --path .
```

This installs the `tsm` binary through Cargo. Create the long-name alias
yourself if wanted:

```sh
ln -sfn tsm ~/.local/bin/traex-session-manager
```

## Update

Check whether a newer GitHub release exists without changing the installation:

```sh
tsm self-update --check
```

Install a newer release:

```sh
tsm self-update
```

The command does not reinstall the current version or downgrade a newer local
build.

## Usage

Run either installed name:

```sh
tsm
traex-session-manager
```

Use `--db <path>` to override traex Store discovery and `--version` to print
the installed version.
