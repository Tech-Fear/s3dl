use anyhow::Result;
use console::style;
use dialoguer::{Confirm, Input, Password, Select};

use crate::config::{Config, EnvConfig, GlobalDefaults, ServiceConfig};

pub fn run_setup() -> Result<Config> {
    println!();
    println!("  {}", style("s3dl setup").bold().cyan());
    println!("  {}", style("─".repeat(40)).dim());
    println!();

    let region: String = Input::new()
        .with_prompt("Default AWS region")
        .default("us-east-1".to_string())
        .interact_text()?;

    let output_dir: String = Input::new()
        .with_prompt("Default output directory")
        .default("~/Downloads".to_string())
        .interact_text()?;

    let mut config = Config {
        defaults: GlobalDefaults {
            region: Some(region),
            output_dir: Some(output_dir),
        },
        env: Default::default(),
    };

    loop {
        println!();
        add_env(&mut config)?;

        if !Confirm::new()
            .with_prompt("Add another environment?")
            .default(false)
            .interact()?
        {
            break;
        }
    }

    Ok(config)
}

pub fn add_env(config: &mut Config) -> Result<()> {
    let env_name: String = Input::new()
        .with_prompt("Environment name (e.g., prod, staging, dev)")
        .interact_text()?;

    let region: String = Input::new()
        .with_prompt(format!("Region for '{env_name}'"))
        .default(
            config
                .defaults
                .region
                .clone()
                .unwrap_or_else(|| "us-east-1".to_string()),
        )
        .interact_text()?;

    let bucket: String = Input::new()
        .with_prompt(format!("Default bucket for '{env_name}'"))
        .allow_empty(true)
        .interact_text()?;

    let output_dir: String = Input::new()
        .with_prompt(format!("Output directory for '{env_name}'"))
        .default(
            config
                .defaults
                .output_dir
                .clone()
                .unwrap_or_else(|| "~/Downloads".to_string()),
        )
        .interact_text()?;

    let (access_key, secret_key, profile) = prompt_auth(&env_name)?;

    let env_default_output = config
        .defaults
        .output_dir
        .as_deref()
        .unwrap_or("~/Downloads");
    let output_dir_opt = if output_dir == env_default_output {
        None
    } else {
        non_empty_opt(output_dir)
    };

    let mut env_cfg = EnvConfig {
        bucket: non_empty_opt(bucket),
        access_key,
        secret_key,
        profile,
        region: Some(region),
        output_dir: output_dir_opt,
        services: Default::default(),
    };

    while Confirm::new()
        .with_prompt("Add a service?")
        .default(true)
        .interact()?
    {
        add_service_to_env(&env_name, &mut env_cfg)?;
    }

    config.env.insert(env_name, env_cfg);
    Ok(())
}

pub fn add_service(config: &mut Config) -> Result<()> {
    let env_names = config.env_names();
    if env_names.is_empty() {
        println!(
            "  {} No environments configured. Run {} first.",
            style("!").yellow(),
            style("s3dl setup").cyan()
        );
        return Ok(());
    }

    let env_names_owned: Vec<String> = env_names.into_iter().map(String::from).collect();
    let selection = Select::new()
        .with_prompt("Which environment?")
        .items(&env_names_owned)
        .interact()?;

    let env_name = env_names_owned[selection].clone();
    let env_cfg = config.env.get_mut(&env_name).unwrap();

    add_service_to_env(&env_name, env_cfg)?;

    Ok(())
}

pub fn remove_env(config: &mut Config) -> Result<bool> {
    let env_names = config.env_names();
    if env_names.is_empty() {
        println!("  {} No environments to remove.", style("!").yellow());
        return Ok(false);
    }

    let env_names_owned: Vec<String> = env_names.into_iter().map(String::from).collect();
    let selection = Select::new()
        .with_prompt("Which environment to remove?")
        .items(&env_names_owned)
        .interact()?;

    let env_name = &env_names_owned[selection];
    let svc_count = config.env[env_name].services.len();

    let label = if svc_count > 0 {
        format!(
            "Remove '{env_name}' and its {svc_count} service{}?",
            if svc_count == 1 { "" } else { "s" }
        )
    } else {
        format!("Remove '{env_name}'?")
    };

    if Confirm::new()
        .with_prompt(label)
        .default(false)
        .interact()?
    {
        config.remove_env(env_name);
        println!(
            "  {} Removed environment '{}'",
            style("✓").green().bold(),
            env_name
        );
        return Ok(true);
    }

    println!("  Cancelled.");
    Ok(false)
}

pub fn remove_service(config: &mut Config) -> Result<bool> {
    let env_names = config.env_names();
    if env_names.is_empty() {
        println!("  {} No environments configured.", style("!").yellow());
        return Ok(false);
    }

    let env_names_owned: Vec<String> = env_names.into_iter().map(String::from).collect();
    let env_sel = Select::new()
        .with_prompt("From which environment?")
        .items(&env_names_owned)
        .interact()?;

    let env_name = &env_names_owned[env_sel];
    let svc_names: Vec<String> = config.env[env_name]
        .services
        .keys()
        .cloned()
        .collect();

    if svc_names.is_empty() {
        println!(
            "  {} No services in '{env_name}'.",
            style("!").yellow()
        );
        return Ok(false);
    }

    let svc_sel = Select::new()
        .with_prompt("Which service to remove?")
        .items(&svc_names)
        .interact()?;

    let svc_name = &svc_names[svc_sel];

    if Confirm::new()
        .with_prompt(format!("Remove '{env_name}/{svc_name}'?"))
        .default(false)
        .interact()?
    {
        config.remove_service(env_name, svc_name);
        println!(
            "  {} Removed service '{}/{}'",
            style("✓").green().bold(),
            env_name,
            svc_name
        );
        return Ok(true);
    }

    println!("  Cancelled.");
    Ok(false)
}

fn add_service_to_env(env_name: &str, env_cfg: &mut EnvConfig) -> Result<()> {
    let svc_name: String = Input::new()
        .with_prompt("Service name (e.g., kyc, esign, uploads)")
        .interact_text()?;

    let bucket: String = Input::new()
        .with_prompt(format!("Bucket for '{env_name}/{svc_name}'"))
        .allow_empty(true)
        .interact_text()?;

    let output_dir: String = Input::new()
        .with_prompt(format!("Output directory for '{env_name}/{svc_name}'"))
        .default("(inherit from env)".to_string())
        .interact_text()?;

    let output_dir_opt = if output_dir == "(inherit from env)" {
        None
    } else {
        non_empty_opt(output_dir)
    };

    let use_own_creds = Confirm::new()
        .with_prompt(format!(
            "Use different credentials than '{env_name}' default?"
        ))
        .default(false)
        .interact()?;

    let (access_key, secret_key, profile) = if use_own_creds {
        prompt_auth(&format!("{env_name}/{svc_name}"))?
    } else {
        (None, None, None)
    };

    let region_override = Confirm::new()
        .with_prompt("Override region for this service?")
        .default(false)
        .interact()?;

    let region = if region_override {
        let r: String = Input::new().with_prompt("Region").interact_text()?;
        Some(r)
    } else {
        None
    };

    env_cfg.services.insert(
        svc_name,
        ServiceConfig {
            bucket: non_empty_opt(bucket),
            access_key,
            secret_key,
            profile,
            region,
            output_dir: output_dir_opt,
        },
    );

    Ok(())
}

fn prompt_auth(label: &str) -> Result<(Option<String>, Option<String>, Option<String>)> {
    let auth_options = &[
        "Access Key + Secret Key",
        "AWS CLI Profile",
        "Default chain (env vars / ~/.aws/credentials / instance role)",
    ];

    let auth_choice = Select::new()
        .with_prompt(format!("Auth method for '{label}'"))
        .items(auth_options)
        .default(0)
        .interact()?;

    match auth_choice {
        0 => {
            let ak: String = Input::new()
                .with_prompt("Access Key ID")
                .interact_text()?;

            let sk: String = Password::new()
                .with_prompt("Secret Access Key")
                .interact()?;

            Ok((non_empty_opt(ak), non_empty_opt(sk), None))
        }
        1 => {
            let profile: String = Input::new()
                .with_prompt("AWS profile name")
                .interact_text()?;

            Ok((None, None, non_empty_opt(profile)))
        }
        _ => Ok((None, None, None)),
    }
}

fn non_empty_opt(s: String) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s)
    }
}
