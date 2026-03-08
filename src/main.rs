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
    /// Stop bosminer on a miner
    Stop {
        /// Miner IP address or hostname
        host: Option<String>,
        /// Stop all stored miners
        #[arg(short, long)]
        all: bool,
    },
    /// Start bosminer on a miner
    Start {
        /// Miner IP address or hostname
        host: Option<String>,
        /// Start all stored miners
        #[arg(short, long)]
        all: bool,
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
        Commands::Stop { host, all } => {
            stop(host, all).await?;
        }
        Commands::Start { host, all } => {
            start(host, all).await?;
        }
    }

    Ok(())
}

async fn stop(host_arg: Option<String>, all: bool) -> Result<()> {
    let config = Config::load()?;
    let miners_to_stop = if all {
        config.miners.values().cloned().collect::<Vec<_>>()
    } else if let Some(host) = host_arg {
        if let Some(miner) = config.miners.get(&host) {
            vec![miner.clone()]
        } else {
            // If not in config, we can't stop because we don't have credentials
            anyhow::bail!("Miner {} not found in config. Please login first.", host);
        }
    } else {
        anyhow::bail!("Please specify a host or use --all");
    };

    if miners_to_stop.is_empty() {
        println!("No miners to stop.");
        return Ok(());
    }

    for miner in miners_to_stop {
        if let Err(e) = stop_miner(&miner).await {
            eprintln!("Failed to stop miner {}: {}", miner.host, e);
        }
    }

    Ok(())
}

async fn stop_miner(miner: &Miner) -> Result<()> {
    println!("Stopping bosminer on {}...", miner.host);

    let client = Client::builder()
        .cookie_store(true)
        .build()?;

    let url = if miner.host.contains("://") {
        format!("{}/graphql", miner.host)
    } else {
        format!("http://{}/graphql", miner.host)
    };

    // 1. Login to get session cookie
    let login_payload = serde_json::json!({
        "query": "mutation ($username: String!, $password: String!) {\n  auth {\n    login(username: $username, password: $password) {\n      ... on Error {\n        message\n        __typename\n      }\n      __typename\n    }\n    __typename\n  }\n}\n",
        "variables": {
            "username": miner.username,
            "password": miner.password.as_deref().unwrap_or("")
        }
    });

    let login_response = client
        .post(&url)
        .json(&login_payload)
        .send()
        .await
        .context("Failed to connect to miner for login")?;

    if !login_response.status().is_success() {
        anyhow::bail!("Login failed with status: {}", login_response.status());
    }

    let login_body: serde_json::Value = login_response.json().await.context("Failed to parse login response")?;
    if login_body["data"]["auth"]["login"]["__typename"] != "VoidResult" {
        let error_message = login_body["data"]["auth"]["login"]["message"].as_str().unwrap_or("Unknown error");
        anyhow::bail!("Login failed: {}", error_message);
    }

    // 2. Send stop mutation
    let stop_payload = serde_json::json!({
        "query": "mutation {\n  bosminer {\n    stop {\n      ... on VoidResult {\n        void\n        __typename\n      }\n      ... on BosminerError {\n        message\n        __typename\n      }\n      __typename\n    }\n    __typename\n  }\n}\n",
        "variables": {}
    });

    let stop_response = client
        .post(&url)
        .json(&stop_payload)
        .send()
        .await
        .context("Failed to send stop command")?;

    if !stop_response.status().is_success() {
        anyhow::bail!("Stop command failed with status: {}", stop_response.status());
    }

    let stop_body: serde_json::Value = stop_response.json().await.context("Failed to parse stop response")?;
    
    // Expected response: {"data":{"bosminer":{"__typename":"BosminerMutation","stop":{"__typename":"VoidResult","void":"void"}}}}
    let stop_result = &stop_body["data"]["bosminer"]["stop"];
    if stop_result["__typename"] == "VoidResult" {
        println!("Successfully stopped bosminer on {}.", miner.host);
    } else {
        let error_message = stop_result["message"].as_str().unwrap_or("Unknown error");
        anyhow::bail!("Stop command failed: {}", error_message);
    }

    Ok(())
}

async fn start(host_arg: Option<String>, all: bool) -> Result<()> {
    let config = Config::load()?;
    let miners_to_start = if all {
        config.miners.values().cloned().collect::<Vec<_>>()
    } else if let Some(host) = host_arg {
        if let Some(miner) = config.miners.get(&host) {
            vec![miner.clone()]
        } else {
            anyhow::bail!("Miner {} not found in config. Please login first.", host);
        }
    } else {
        anyhow::bail!("Please specify a host or use --all");
    };

    if miners_to_start.is_empty() {
        println!("No miners to start.");
        return Ok(());
    }

    for miner in miners_to_start {
        if let Err(e) = start_miner(&miner).await {
            eprintln!("Failed to start miner {}: {}", miner.host, e);
        }
    }

    Ok(())
}

async fn start_miner(miner: &Miner) -> Result<()> {
    println!("Starting bosminer on {}...", miner.host);

    let client = Client::builder()
        .cookie_store(true)
        .build()?;

    let url = if miner.host.contains("://") {
        format!("{}/graphql", miner.host)
    } else {
        format!("http://{}/graphql", miner.host)
    };

    // 1. Login to get session cookie
    let login_payload = serde_json::json!({
        "query": "mutation ($username: String!, $password: String!) {\n  auth {\n    login(username: $username, password: $password) {\n      ... on Error {\n        message\n        __typename\n      }\n      __typename\n    }\n    __typename\n  }\n}\n",
        "variables": {
            "username": miner.username,
            "password": miner.password.as_deref().unwrap_or("")
        }
    });

    let login_response = client
        .post(&url)
        .json(&login_payload)
        .send()
        .await
        .context("Failed to connect to miner for login")?;

    if !login_response.status().is_success() {
        anyhow::bail!("Login failed with status: {}", login_response.status());
    }

    let login_body: serde_json::Value = login_response.json().await.context("Failed to parse login response")?;
    if login_body["data"]["auth"]["login"]["__typename"] != "VoidResult" {
        let error_message = login_body["data"]["auth"]["login"]["message"].as_str().unwrap_or("Unknown error");
        anyhow::bail!("Login failed: {}", error_message);
    }

    // 2. Send start mutation
    let start_payload = serde_json::json!({
        "query": "mutation {\n  bosminer {\n    start {\n      ... on BosminerError {\n        message\n        __typename\n      }\n      __typename\n    }\n    __typename\n  }\n}\n",
        "variables": {}
    });

    let start_response = client
        .post(&url)
        .json(&start_payload)
        .send()
        .await
        .context("Failed to send start command")?;

    if !start_response.status().is_success() {
        anyhow::bail!("Start command failed with status: {}", start_response.status());
    }

    let start_body: serde_json::Value = start_response.json().await.context("Failed to parse start response")?;
    
    // Expected response: {"data":{"bosminer":{"__typename":"BosminerMutation","start":{"__typename":"VoidResult"}}}}
    let start_result = &start_body["data"]["bosminer"]["start"];
    if start_result["__typename"] == "VoidResult" {
        println!("Successfully started bosminer on {}.", miner.host);
    } else {
        let error_message = start_result["message"].as_str().unwrap_or("Unknown error");
        anyhow::bail!("Start command failed: {}", error_message);
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
