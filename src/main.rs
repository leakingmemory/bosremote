use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "bosremote")]
#[command(about = "Remote control Braiins OS miners", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Login to a miner and store credentials
    Login {
        /// Miner IP address or hostname
        host: String,
        /// Username
        #[arg(short, long, default_value = "root")]
        username: String,
        /// Password
        #[arg(short, long)]
        password: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Miner {
    host: String,
    username: String,
    password: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct Config {
    miners: HashMap<String, Miner>,
}

impl Config {
    fn load() -> Result<Self> {
        let config_path = get_config_path()?;
        if !config_path.exists() {
            return Ok(Config::default());
        }
        let content = fs::read_to_string(config_path)?;
        let config: Config = serde_json::from_str(&content).context("Failed to parse config file")?;
        Ok(config)
    }

    fn save(&self) -> Result<()> {
        let config_path = get_config_path()?;
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(config_path, content)?;
        Ok(())
    }
}

fn get_config_path() -> Result<PathBuf> {
    let home = directories::UserDirs::new()
        .context("Could not determine home directory")?;
    let config_path = home.home_dir().join(".config").join("bosremote").join("miners.json");
    Ok(config_path)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Login {
            host,
            username,
            password,
        } => {
            login(host, username, password).await?;
        }
    }

    Ok(())
}

async fn login(host: String, username: String, password: Option<String>) -> Result<()> {
    println!("Testing login to {}...", host);

    let client = Client::builder()
        // .danger_accept_invalid_certs(true) // Requires TLS feature
        .build()?;

    let login_url = if host.contains("://") {
        format!("{}/graphql", host)
    } else {
        format!("http://{}/graphql", host)
    };

    let login_payload = serde_json::json!({
        "query": "mutation ($username: String!, $password: String!) {\n  auth {\n    login(username: $username, password: $password) {\n      ... on Error {\n        message\n        __typename\n      }\n      __typename\n    }\n    __typename\n  }\n}\n",
        "variables": {
            "username": username,
            "password": password.as_deref().unwrap_or("")
        }
    });

    let login_response = client
        .post(&login_url)
        .json(&login_payload)
        .send()
        .await
        .context("Failed to connect to miner")?;

    if login_response.status().is_success() {
        let response_body: serde_json::Value = login_response.json().await.context("Failed to parse response body")?;
        
        // Success response: {"data":{"auth":{"__typename":"Auth","login":{"__typename":"VoidResult"}}}}
        let is_success = response_body["data"]["auth"]["login"]["__typename"] == "VoidResult";
        
        if is_success {
            println!("Login successful!");
            let mut config = Config::load()?;
            config.miners.insert(
                host.clone(),
                Miner {
                    host,
                    username,
                    password,
                },
            );
            config.save()?;
            println!("Credentials saved.");
        } else {
            let error_message = response_body["data"]["auth"]["login"]["message"].as_str().unwrap_or("Unknown error");
            anyhow::bail!("Login failed: {}", error_message);
        }
    } else if login_response.status() == reqwest::StatusCode::UNAUTHORIZED || login_response.status() == reqwest::StatusCode::FORBIDDEN {
        anyhow::bail!("Login failed: Unauthorized. Please check your username and password.");
    } else {
        anyhow::bail!("Login failed with status: {}", login_response.status());
    }

    Ok(())
}
