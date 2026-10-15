# GitHub Issue Reproducer

[![Language](https://img.shields.io/github/languages/top/ulrichheringer/github-issue-reproducer?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/github/license/ulrichheringer/github-issue-reproducer)](LICENSE)

Save a GitHub issue and run reproduction steps that you configure. The CLI stores logs and selected files so you can share the setup and results. It does not generate reproduction steps.

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
