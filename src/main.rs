use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_DIR: &str = ".issue-repro";
const CONFIG_FILE: &str = "reproduction.json";

#[derive(Parser)]
#[command(
    version,
    about = "Capture and run a configurable GitHub issue reproduction"
)]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Fetch issue details and create an editable reproduction configuration.
    Init {
        issue_url: String,
        #[arg(long, default_value = DEFAULT_DIR)]
        dir: PathBuf,
    },
    /// Run configured setup and test commands, saving their output.
    Run {
        #[arg(long, default_value = DEFAULT_DIR)]
        dir: PathBuf,
    },
    /// Copy the configuration, logs, and explicitly selected files to a folder.
    Export {
        #[arg(long, default_value = DEFAULT_DIR)]
        dir: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Debug, Deserialize, Serialize)]
struct Issue {
    url: String,
    number: u64,
    title: String,
    body: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CommandSpec {
    program: String,
    #[serde(default)]
    args: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ReproductionConfig {
    issue: Issue,
    workspace: PathBuf,
    #[serde(default)]
    setup: Vec<CommandSpec>,
    #[serde(default)]
    commands: Vec<CommandSpec>,
    #[serde(default)]
    export_files: Vec<PathBuf>,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Action::Init { issue_url, dir } => init(&issue_url, &dir),
        Action::Run { dir } => run(&dir),
        Action::Export { dir, out } => export(&dir, &out),
    }
}

fn parse_issue_url(input: &str) -> Result<(String, String, u64)> {
    let clean = input
        .split(['?', '#'])
        .next()
        .unwrap_or(input)
        .trim_end_matches('/');
    let Some(path) = clean.strip_prefix("https://github.com/") else {
        bail!("expected an https://github.com/owner/repository/issues/number URL");
    };
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() != 4 || parts[2] != "issues" {
        bail!("expected an https://github.com/owner/repository/issues/number URL");
    }
    let number = parts[3]
        .parse::<u64>()
        .context("issue number must be a positive integer")?;
    if number == 0 || parts[0].is_empty() || parts[1].is_empty() {
        bail!("owner, repository, and a positive issue number are required");
    }
    Ok((parts[0].to_owned(), parts[1].to_owned(), number))
}

fn fetch_issue(issue_url: &str) -> Result<Issue> {
    let (owner, repo, number) = parse_issue_url(issue_url)?;
    let client = Client::builder()
        .user_agent(concat!(
            "github-issue-reproducer/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()?;
    let response = client
        .get(format!(
            "https://api.github.com/repos/{owner}/{repo}/issues/{number}"
        ))
        .send()
        .context("could not reach the GitHub API")?;
    if !response.status().is_success() {
        bail!("GitHub API returned {} for {issue_url}", response.status());
    }
    #[derive(Deserialize)]
    struct ApiIssue {
        html_url: String,
        number: u64,
        title: String,
        body: Option<String>,
    }
    let issue: ApiIssue = response.json().context("could not read issue details")?;
    Ok(Issue {
        url: issue.html_url,
        number: issue.number,
        title: issue.title,
        body: issue.body,
    })
}

fn config_path(dir: &Path) -> PathBuf {
    dir.join(CONFIG_FILE)
}

fn init(issue_url: &str, dir: &Path) -> Result<()> {
    let target = config_path(dir);
    if target.exists() {
        bail!(
            "{} already exists; refusing to overwrite it",
            target.display()
        );
    }
    let issue = fetch_issue(issue_url)?;
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    let config = ReproductionConfig {
        issue,
        workspace: std::env::current_dir()?,
        setup: Vec::new(),
        commands: Vec::new(),
        export_files: Vec::new(),
    };
    fs::write(&target, serde_json::to_vec_pretty(&config)?)
        .with_context(|| format!("could not write {}", target.display()))?;
    println!(
        "Created {}. Add your own setup and reproduction commands before running it.",
        target.display()
    );
    Ok(())
}

fn read_config(dir: &Path) -> Result<ReproductionConfig> {
    let path = config_path(dir);
    let data = fs::read(&path).with_context(|| format!("could not read {}", path.display()))?;
    serde_json::from_slice(&data)
        .with_context(|| format!("invalid configuration in {}", path.display()))
}

fn execute_commands(
    commands: &[CommandSpec],
    label: &str,
    workspace: &Path,
    log: &mut String,
) -> bool {
    let mut all_succeeded = true;
    for spec in commands {
        log.push_str(&format!("\n$ {} {}\n", spec.program, spec.args.join(" ")));
        let output = Command::new(&spec.program)
            .args(&spec.args)
            .current_dir(workspace)
            .output();
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                log.push_str(&format!("could not start command: {error}\n"));
                eprintln!("{label}: {} could not start: {error}", spec.program);
                all_succeeded = false;
                continue;
            }
        };
        log.push_str(&String::from_utf8_lossy(&output.stdout));
        log.push_str(&String::from_utf8_lossy(&output.stderr));
        log.push_str(&format!("\nexit status: {}\n", output.status));
        println!("{label}: {} — {}", spec.program, output.status);
        all_succeeded &= output.status.success();
    }
    all_succeeded
}

fn create_log_file(dir: &Path, epoch: u64) -> Result<(PathBuf, fs::File)> {
    let log_dir = dir.join("logs");
    fs::create_dir_all(&log_dir)?;
    for suffix in 0..u32::MAX {
        let name = if suffix == 0 {
            format!("run-{epoch}.log")
        } else {
            format!("run-{epoch}-{suffix}.log")
        };
        let path = log_dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("could not create {}", path.display()))
            }
        }
    }
    bail!("could not choose a unique log file name")
}

fn run(dir: &Path) -> Result<()> {
    let config = read_config(dir)?;
    if config.setup.is_empty() && config.commands.is_empty() {
        bail!(
            "no commands configured; edit {} first",
            config_path(dir).display()
        );
    }
    let epoch = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let (log_path, mut log_file) = create_log_file(dir, epoch)?;
    let mut log = format!(
        "Issue: {} — {}\nWorkspace: {}\n",
        config.issue.url,
        config.issue.title,
        config.workspace.display()
    );
    let setup_ok = execute_commands(&config.setup, "setup", &config.workspace, &mut log);
    let commands_ok = execute_commands(&config.commands, "run", &config.workspace, &mut log);
    log_file
        .write_all(log.as_bytes())
        .with_context(|| format!("could not write {}", log_path.display()))?;
    println!("Log saved to {}", log_path.display());
    if !setup_ok || !commands_ok {
        bail!("one or more commands failed; see {}", log_path.display());
    }
    Ok(())
}

fn ensure_relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("export paths must be non-empty relative paths without . or .. components");
    }
    Ok(())
}

fn absolute_lexical(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_file() {
        bail!(
            "export file does not exist or is not a file: {}",
            source.display()
        );
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)
        .with_context(|| format!("could not copy {}", source.display()))?;
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if entry.file_type()?.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn export(dir: &Path, out: &Path) -> Result<()> {
    let config = read_config(dir)?;
    let source_dir = absolute_lexical(dir)?;
    let output_dir = absolute_lexical(out)?;
    if output_dir.starts_with(&source_dir) {
        bail!("export path must be outside the reproduction directory");
    }
    if out.exists() {
        bail!("{} already exists; choose a new export path", out.display());
    }
    for relative in &config.export_files {
        ensure_relative(relative)?;
        if !config.workspace.join(relative).is_file() {
            bail!(
                "export file does not exist or is not a file: {}",
                relative.display()
            );
        }
    }
    fs::create_dir_all(out)?;
    copy_file(&config_path(dir), &out.join(CONFIG_FILE))?;
    copy_tree(&dir.join("logs"), &out.join("logs"))?;
    for relative in &config.export_files {
        copy_file(
            &config.workspace.join(relative),
            &out.join("files").join(relative),
        )?;
    }
    println!(
        "Exported issue details, logs, and selected files to {}",
        out.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ensure_relative, parse_issue_url, ReproductionConfig};
    use std::path::Path;

    #[test]
    fn parses_issue_url_and_ignores_query() {
        let parsed =
            parse_issue_url("https://github.com/rust-lang/rust/issues/123?tab=comments").unwrap();
        assert_eq!(parsed, ("rust-lang".into(), "rust".into(), 123));
    }

    #[test]
    fn rejects_pull_request_urls() {
        assert!(parse_issue_url("https://github.com/org/repo/pull/4").is_err());
    }

    #[test]
    fn config_does_not_invent_commands() {
        let config: ReproductionConfig = serde_json::from_str(
            r#"{"issue":{"url":"https://github.com/org/repo/issues/4","number":4,"title":"Bug","body":null},"workspace":"."}"#,
        )
        .unwrap();
        assert!(config.setup.is_empty());
        assert!(config.commands.is_empty());
    }

    #[test]
    fn export_paths_cannot_escape_the_workspace() {
        assert!(ensure_relative(Path::new("src/main.rs")).is_ok());
        assert!(ensure_relative(Path::new("../secrets.env")).is_err());
        assert!(ensure_relative(Path::new("C:\\secrets.env")).is_err());
    }
}
