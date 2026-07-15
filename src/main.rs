mod config;
mod s3;
mod setup;

use anyhow::{Context, Result};
use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use console::style;

/// S3 file downloader with multi-environment, multi-service credential management.
///
/// Download args can be used directly without the `get` subcommand:
///   s3dl -e prod -s kyc -f path/to/key
///   s3dl -f path/to/key --ak KEY --sk SECRET -b my-bucket
#[derive(Parser)]
#[command(
    name = "s3dl",
    version,
    args_conflicts_with_subcommands = true,
    after_help = "Quick start:\n  s3dl setup                                   # interactive config wizard\n  s3dl -e prod -s myservice -f path/to/key      # download a file\n  s3dl head -e prod -s myservice -f path/to/key # metadata only\n  s3dl completions zsh >> ~/.zshrc              # enable tab completions"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    download: DownloadArgs,
}

#[derive(Args, Debug)]
struct DownloadArgs {
    /// Environment name (e.g., prod, staging)
    #[arg(short, long)]
    env: Option<String>,

    /// Service name (e.g., kyc, esign). Omit to use env defaults.
    #[arg(short, long)]
    service: Option<String>,

    /// S3 object key
    #[arg(short, long)]
    file: Option<String>,

    /// Bucket (overrides config)
    #[arg(short, long)]
    bucket: Option<String>,

    /// Output directory or full file path (overrides config)
    #[arg(short, long)]
    output: Option<String>,

    /// AWS Access Key (overrides config)
    #[arg(long = "access-key", visible_alias = "ak")]
    access_key: Option<String>,

    /// AWS Secret Key (overrides config)
    #[arg(long = "secret-key", visible_alias = "sk")]
    secret_key: Option<String>,

    /// AWS Region (overrides config)
    #[arg(short, long)]
    region: Option<String>,

    /// Skip S3 metadata lookup for auto file extension
    #[arg(long)]
    no_auto_ext: bool,

    /// Suppress progress output
    #[arg(short, long)]
    quiet: bool,
}

#[derive(Args, Debug)]
struct HeadArgs {
    /// Environment name
    #[arg(short, long)]
    env: Option<String>,

    /// Service name
    #[arg(short, long)]
    service: Option<String>,

    /// S3 object key
    #[arg(short, long)]
    file: String,

    /// Bucket (overrides config)
    #[arg(short, long)]
    bucket: Option<String>,

    /// AWS Access Key (overrides config)
    #[arg(long = "access-key", visible_alias = "ak")]
    access_key: Option<String>,

    /// AWS Secret Key (overrides config)
    #[arg(long = "secret-key", visible_alias = "sk")]
    secret_key: Option<String>,

    /// AWS Region (overrides config)
    #[arg(short, long)]
    region: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Download a file from S3 (same as using args directly)
    Get {
        #[command(flatten)]
        args: DownloadArgs,
    },

    /// Show S3 object metadata without downloading
    Head {
        #[command(flatten)]
        args: HeadArgs,
    },

    /// Interactive first-time setup wizard
    Setup,

    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Generate shell tab-completions
    Completions {
        /// Shell to generate for
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show all configured environments and services
    List,
    /// Print config file path
    Path,
    /// Open config file in $EDITOR
    Edit,
    /// Add a new environment interactively
    AddEnv,
    /// Add a service to an existing environment
    AddService,
    /// Remove an environment and all its services
    RemoveEnv,
    /// Remove a service from an environment
    RemoveService,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Get { args }) => handle_download(args).await,
        Some(Command::Head { args }) => handle_head(args).await,
        Some(Command::Setup) => handle_setup(),
        Some(Command::Config { action }) => handle_config(action),
        Some(Command::Completions { shell }) => {
            clap_complete::generate(shell, &mut Cli::command(), "s3dl", &mut std::io::stdout());
            Ok(())
        }
        None => {
            if cli.download.file.is_some() {
                handle_download(cli.download).await
            } else {
                Cli::command().print_help()?;
                Ok(())
            }
        }
    }
}

fn resolve_from_args(
    env: Option<&str>,
    service: Option<&str>,
    bucket: Option<&str>,
    access_key: Option<&str>,
    secret_key: Option<&str>,
    region: Option<&str>,
) -> Result<config::ResolvedConfig> {
    let has_inline_creds =
        access_key.is_some() && secret_key.is_some() && bucket.is_some();

    if has_inline_creds && env.is_none() {
        return Ok(config::ResolvedConfig {
            bucket: bucket.unwrap().to_string(),
            region: region.unwrap_or("us-east-1").to_string(),
            auth: config::AuthMethod::StaticKeys {
                access_key: access_key.unwrap().to_string(),
                secret_key: secret_key.unwrap().to_string(),
            },
            output_dir: "~/Downloads".to_string(),
        });
    }

    let env_name = env.context(
        "--env is required (or provide --access-key, --secret-key, and --bucket for inline usage)",
    )?;

    let cfg = config::load_config().context("no config found — run `s3dl setup` to create one")?;
    let mut resolved = cfg.resolve(env_name, service)?;

    if let Some(b) = bucket {
        resolved.bucket = b.to_string();
    }
    if let Some(r) = region {
        resolved.region = r.to_string();
    }
    if let (Some(ak), Some(sk)) = (access_key, secret_key) {
        resolved.auth = config::AuthMethod::StaticKeys {
            access_key: ak.to_string(),
            secret_key: sk.to_string(),
        };
    }

    Ok(resolved)
}

async fn handle_head(args: HeadArgs) -> Result<()> {
    let resolved = resolve_from_args(
        args.env.as_deref(),
        args.service.as_deref(),
        args.bucket.as_deref(),
        args.access_key.as_deref(),
        args.secret_key.as_deref(),
        args.region.as_deref(),
    )?;

    let client = s3::build_client(&resolved).await?;
    let meta = s3::head_object(&client, &resolved.bucket, &args.file).await?;

    println!();
    println!("  {}", style("─".repeat(50)).dim());
    println!(
        "  {}  s3://{}/{}",
        style("Object").dim(),
        resolved.bucket,
        args.file
    );
    println!("  {}", style("─".repeat(50)).dim());

    if let Some(ct) = &meta.content_type {
        let ext = s3::mime_to_extension(ct)
            .map(|e| format!("  (.{e})"))
            .unwrap_or_default();
        println!("  {}  {}{}", style("Content-Type").dim(), ct, style(ext).cyan());
    }
    if let Some(cl) = meta.content_length {
        println!(
            "  {}  {} ({})",
            style("Size").dim(),
            format_bytes(cl as u64),
            cl
        );
    }
    if let Some(lm) = &meta.last_modified {
        println!("  {}  {}", style("Last Modified").dim(), lm);
    }
    if let Some(etag) = &meta.e_tag {
        println!("  {}  {}", style("ETag").dim(), etag);
    }
    if let Some(sc) = &meta.storage_class {
        println!("  {}  {}", style("Storage Class").dim(), sc);
    }
    if !meta.metadata.is_empty() {
        println!("  {}", style("User Metadata").dim());
        for (k, v) in &meta.metadata {
            println!("    {}  {}", style(format!("{k}:")).dim(), v);
        }
    }

    println!("  {}", style("─".repeat(50)).dim());
    println!();

    Ok(())
}

async fn handle_download(args: DownloadArgs) -> Result<()> {
    let file_key = args.file.as_deref().context("--file is required")?;

    let resolved = resolve_from_args(
        args.env.as_deref(),
        args.service.as_deref(),
        args.bucket.as_deref(),
        args.access_key.as_deref(),
        args.secret_key.as_deref(),
        args.region.as_deref(),
    )?;

    let cfg_output_dir = resolved.output_dir.clone();
    let client = s3::build_client(&resolved).await?;

    let mut detected_ext: Option<String> = None;
    let mut content_length: Option<i64> = None;

    if !args.no_auto_ext {
        if !args.quiet {
            eprint!("  Fetching metadata... ");
        }
        match s3::head_object(&client, &resolved.bucket, file_key).await {
            Ok(meta) => {
                content_length = meta.content_length;
                if let Some(ct) = &meta.content_type {
                    if let Some(ext) = s3::mime_to_extension(ct) {
                        detected_ext = Some(ext.to_string());
                        if !args.quiet {
                            eprintln!("{ct} -> .{ext}");
                        }
                    } else if !args.quiet {
                        eprintln!("{ct} (will detect from content)");
                    }
                } else if !args.quiet {
                    eprintln!("no content-type (will detect from content)");
                }
            }
            Err(e) => {
                if !args.quiet {
                    eprintln!("{} {e:#}", style("warning:").yellow());
                }
            }
        }
    }

    // Download to a temporary path first if we still need content-based detection
    let needs_content_detection = detected_ext.is_none() && !args.no_auto_ext;

    let ext_ref = detected_ext.as_deref();

    let initial_output_path = if let Some(ref o) = args.output {
        let p = std::path::PathBuf::from(o);
        if p.is_dir() {
            s3::resolve_output_path(file_key, Some(o), ext_ref, o)
        } else {
            p
        }
    } else {
        s3::resolve_output_path(file_key, None, ext_ref, &cfg_output_dir)
    };

    let auth_label = match &resolved.auth {
        config::AuthMethod::StaticKeys { .. } => "static keys".to_string(),
        config::AuthMethod::Profile(p) => format!("profile ({p})"),
        config::AuthMethod::Default => "aws default chain".to_string(),
    };

    if !args.quiet {
        let env_label = args.env.as_deref().unwrap_or("(inline)");
        eprintln!();
        eprintln!("  {}", style("─".repeat(44)).dim());
        eprintln!(
            "  {}  {}{}",
            style("Env").dim(),
            env_label,
            args.service
                .as_deref()
                .map(|s| format!(" / {s}"))
                .unwrap_or_default()
        );
        eprintln!("  {}  {}", style("Bucket").dim(), resolved.bucket);
        eprintln!("  {}  {}", style("Key").dim(), file_key);
        eprintln!("  {}  {}", style("Region").dim(), resolved.region);
        eprintln!("  {}  {}", style("Auth").dim(), auth_label);
        eprintln!("  {}", style("─".repeat(44)).dim());
        eprintln!();
    }

    let bytes_downloaded = s3::download(
        &client,
        &resolved.bucket,
        file_key,
        &initial_output_path,
        content_length,
        args.quiet,
    )
    .await?;

    // Content-based detection: inspect downloaded bytes and rename if type found
    let final_output_path = if needs_content_detection {
        if let Some(ext) = s3::detect_extension_from_content(&initial_output_path) {
            let new_path = append_extension(&initial_output_path, ext);
            if new_path != initial_output_path {
                std::fs::rename(&initial_output_path, &new_path)
                    .with_context(|| format!("failed to rename to {}", new_path.display()))?;
                if !args.quiet {
                    eprintln!(
                        "  {} Detected type from content: .{}",
                        style("i").cyan().bold(),
                        ext
                    );
                }
                new_path
            } else {
                initial_output_path
            }
        } else {
            initial_output_path
        }
    } else {
        initial_output_path
    };

    if !args.quiet {
        let size = format_bytes(bytes_downloaded);
        eprintln!(
            "  {} Downloaded {} to {}",
            style("✓").green().bold(),
            style(size).bold(),
            style(final_output_path.display()).underlined()
        );
    }

    Ok(())
}

fn append_extension(path: &std::path::Path, ext: &str) -> std::path::PathBuf {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    if let Some(dot_pos) = name.rfind('.') {
        let existing = &name[dot_pos + 1..];
        if existing.eq_ignore_ascii_case(ext) {
            return path.to_path_buf();
        }
    }

    let new_name = format!("{name}.{ext}");
    path.with_file_name(new_name)
}

fn handle_setup() -> Result<()> {
    let existing = config::load_config().ok();

    if existing.is_some() {
        let overwrite = dialoguer::Confirm::new()
            .with_prompt("Config already exists. Overwrite?")
            .default(false)
            .interact()?;

        if !overwrite {
            println!("  Cancelled. Use {} to modify.", style("s3dl config").cyan());
            return Ok(());
        }
    }

    let config = setup::run_setup()?;
    let path = config::save_config(&config)?;

    println!();
    println!(
        "  {} Config saved to {}",
        style("✓").green().bold(),
        style(path.display()).underlined()
    );
    println!();
    println!("  Get started:");
    println!(
        "    {}",
        style("s3dl -e prod -s myservice -f \"path/to/file\"").cyan()
    );
    println!();

    Ok(())
}

fn handle_config(action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::List => {
            let cfg = config::load_config().context("no config found — run `s3dl setup`")?;
            println!();
            cfg.list_services();
            println!("  Config: {}", style(config::config_path().display()).dim());
            println!();
        }

        ConfigAction::Path => {
            println!("{}", config::config_path().display());
        }

        ConfigAction::Edit => {
            let path = config::config_path();
            if !path.exists() {
                println!("  No config found. Run {} first.", style("s3dl setup").cyan());
                return Ok(());
            }

            let editor = std::env::var("EDITOR")
                .or_else(|_| std::env::var("VISUAL"))
                .unwrap_or_else(|_| {
                    if cfg!(target_os = "macos") {
                        "open -t".to_string()
                    } else {
                        "vi".to_string()
                    }
                });

            let parts: Vec<&str> = editor.split_whitespace().collect();
            let status = std::process::Command::new(parts[0])
                .args(&parts[1..])
                .arg(&path)
                .status()
                .with_context(|| format!("failed to launch editor: {editor}"))?;

            if !status.success() {
                anyhow::bail!("editor exited with {status}");
            }
        }

        ConfigAction::AddEnv => {
            let mut cfg = config::load_config().unwrap_or_else(|_| config::Config::new());
            setup::add_env(&mut cfg)?;
            let path = config::save_config(&cfg)?;
            println!(
                "\n  {} Saved to {}\n",
                style("✓").green().bold(),
                style(path.display()).underlined()
            );
        }

        ConfigAction::AddService => {
            let mut cfg = config::load_config()
                .context("no config found — run `s3dl setup` or `s3dl config add-env` first")?;
            setup::add_service(&mut cfg)?;
            let path = config::save_config(&cfg)?;
            println!(
                "\n  {} Saved to {}\n",
                style("✓").green().bold(),
                style(path.display()).underlined()
            );
        }

        ConfigAction::RemoveEnv => {
            let mut cfg =
                config::load_config().context("no config found — nothing to remove")?;
            if setup::remove_env(&mut cfg)? {
                let path = config::save_config(&cfg)?;
                println!(
                    "  {} Saved to {}\n",
                    style("✓").green().bold(),
                    style(path.display()).underlined()
                );
            }
        }

        ConfigAction::RemoveService => {
            let mut cfg =
                config::load_config().context("no config found — nothing to remove")?;
            if setup::remove_service(&mut cfg)? {
                let path = config::save_config(&cfg)?;
                println!(
                    "  {} Saved to {}\n",
                    style("✓").green().bold(),
                    style(path.display()).underlined()
                );
            }
        }
    }

    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
