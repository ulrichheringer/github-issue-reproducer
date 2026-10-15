# github-issue-reproducer

A small CLI for saving a GitHub issue next to the reproduction steps you choose to run. It never guesses or generates a reproduction for you.

## Install

Requires Rust 1.88 or newer.

```sh
cargo install --path .
```

## Use

Run `init` from the project you want to use, then edit `.issue-repro/reproduction.json` and add your setup and test commands as executable-plus-argument lists.

```sh
github-issue-reproducer init https://github.com/owner/project/issues/42
github-issue-reproducer run
github-issue-reproducer export --out ./issue-42-export
```

`run` saves command output under `.issue-repro/logs`. `export` copies the issue configuration, logs, and only the extra files listed in `export_files`.

For example, set `commands` to `[{"program":"cargo","args":["test"]}]` to run a Rust project's test suite. The CLI leaves this list empty until you add your own reproduction steps.
