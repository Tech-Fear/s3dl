use anyhow::{Context, Result};
use console::style;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub defaults: GlobalDefaults,
    #[serde(default)]
    pub env: BTreeMap<String, EnvConfig>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct GlobalDefaults {
    pub region: Option<String>,
    pub output_dir: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EnvConfig {
    pub bucket: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub profile: Option<String>,
    pub region: Option<String>,
    pub output_dir: Option<String>,
    #[serde(default)]
    pub services: BTreeMap<String, ServiceConfig>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ServiceConfig {
    pub bucket: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub profile: Option<String>,
    pub region: Option<String>,
    pub output_dir: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AuthMethod {
    StaticKeys {
        access_key: String,
        secret_key: String,
    },
    Profile(String),
    Default,
}

#[derive(Debug)]
pub struct ResolvedConfig {
    pub bucket: String,
    pub region: String,
    pub auth: AuthMethod,
    pub output_dir: String,
}

impl Config {
    pub fn new() -> Self {
        Config {
            defaults: GlobalDefaults::default(),
            env: BTreeMap::new(),
        }
    }

    pub fn resolve(&self, env_name: &str, service_name: Option<&str>) -> Result<ResolvedConfig> {
        let env_cfg = self
            .env
            .get(env_name)
            .with_context(|| format!("environment '{}' not found in config", env_name))?;

        let svc_cfg = service_name.and_then(|s| env_cfg.services.get(s));

        let bucket = svc_cfg
            .and_then(|s| non_empty(&s.bucket))
            .or_else(|| non_empty(&env_cfg.bucket))
            .with_context(|| {
                let svc = service_name.unwrap_or("(default)");
                format!("no bucket configured for env '{}', service '{}'", env_name, svc)
            })?
            .to_string();

        let region = svc_cfg
            .and_then(|s| non_empty(&s.region))
            .or_else(|| non_empty(&env_cfg.region))
            .or_else(|| non_empty(&self.defaults.region))
            .unwrap_or("us-east-1")
            .to_string();

        let output_dir = svc_cfg
            .and_then(|s| non_empty(&s.output_dir))
            .or_else(|| non_empty(&env_cfg.output_dir))
            .or_else(|| non_empty(&self.defaults.output_dir))
            .unwrap_or("~/Downloads")
            .to_string();

        let auth = resolve_auth(svc_cfg, env_cfg);

        Ok(ResolvedConfig {
            bucket,
            region,
            auth,
            output_dir,
        })
    }

    pub fn list_services(&self) {
        if self.env.is_empty() {
            println!("  No environments configured.");
            println!("  Run {} to get started.", style("s3dl setup").cyan());
            return;
        }

        for (env_name, env_cfg) in &self.env {
            let region = env_cfg
                .region
                .as_deref()
                .or(self.defaults.region.as_deref())
                .unwrap_or("us-east-1");

            let default_bucket = env_cfg
                .bucket
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("-");

            let creds = auth_label(resolve_auth(None, env_cfg));

            let out = env_cfg
                .output_dir
                .as_deref()
                .or(self.defaults.output_dir.as_deref())
                .unwrap_or("~/Downloads");

            println!(
                "  {} {}",
                style(format!("[{env_name}]")).green().bold(),
                style(format!(
                    "region={region}  bucket={default_bucket}  auth={creds}  output={out}"
                ))
                .dim()
            );

            if env_cfg.services.is_empty() {
                println!("    {}", style("(no services — uses env defaults)").dim());
            } else {
                for (svc_name, svc) in &env_cfg.services {
                    let bkt = svc
                        .bucket
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("(env default)");

                    let svc_auth = if svc.access_key.as_ref().is_some_and(|s| !s.is_empty()) {
                        "own keys"
                    } else if svc.profile.as_ref().is_some_and(|s| !s.is_empty()) {
                        "own profile"
                    } else {
                        "env default"
                    };

                    let svc_out = svc
                        .output_dir
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("(env default)");

                    println!(
                        "    {:<16} bucket={:<28} auth={:<12} output={}",
                        svc_name, bkt, svc_auth, svc_out
                    );
                }
            }
            println!();
        }
    }

    pub fn env_names(&self) -> Vec<&str> {
        self.env.keys().map(|s| s.as_str()).collect()
    }

    pub fn remove_env(&mut self, env_name: &str) -> bool {
        self.env.remove(env_name).is_some()
    }

    pub fn remove_service(&mut self, env_name: &str, svc_name: &str) -> bool {
        if let Some(env_cfg) = self.env.get_mut(env_name) {
            return env_cfg.services.remove(svc_name).is_some();
        }
        false
    }
}

fn resolve_auth(svc: Option<&ServiceConfig>, env: &EnvConfig) -> AuthMethod {
    if let Some(s) = svc {
        if let (Some(ak), Some(sk)) = (&s.access_key, &s.secret_key) {
            if !ak.is_empty() && !sk.is_empty() {
                return AuthMethod::StaticKeys {
                    access_key: ak.clone(),
                    secret_key: sk.clone(),
                };
            }
        }
        if let Some(p) = &s.profile {
            if !p.is_empty() {
                return AuthMethod::Profile(p.clone());
            }
        }
    }

    if let (Some(ak), Some(sk)) = (&env.access_key, &env.secret_key) {
        if !ak.is_empty() && !sk.is_empty() {
            return AuthMethod::StaticKeys {
                access_key: ak.clone(),
                secret_key: sk.clone(),
            };
        }
    }

    if let Some(p) = &env.profile {
        if !p.is_empty() {
            return AuthMethod::Profile(p.clone());
        }
    }

    AuthMethod::Default
}

fn auth_label(auth: AuthMethod) -> &'static str {
    match auth {
        AuthMethod::StaticKeys { .. } => "static keys",
        AuthMethod::Profile(_) => "aws profile",
        AuthMethod::Default => "aws default chain",
    }
}

fn non_empty(opt: &Option<String>) -> Option<&str> {
    opt.as_deref().filter(|s| !s.is_empty())
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("s3dl")
        .join("config.toml")
}

pub fn load_config() -> Result<Config> {
    let path = config_path();
    let content = fs::read_to_string(&path)
        .with_context(|| format!("could not read config at {}", path.display()))?;
    let config: Config = toml::from_str(&content).context("invalid config syntax")?;
    Ok(config)
}

pub fn save_config(config: &Config) -> Result<PathBuf> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = toml::to_string_pretty(config).context("failed to serialize config")?;
    fs::write(&path, &content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(path)
}
