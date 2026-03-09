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
    /// Get status of a miner
    Status {
        host: Option<String>,
        /// Get status of all stored miners
        #[arg(short, long)]
        all: bool,
    },
    /// Set power target for the miner
    SetPower {
        host: Option<String>,
        /// Power target in Watts
        power: u32,
        /// Set power for all stored miners
        #[arg(short, long)]
        all: bool,
    },
    /// Manage the allowlist of power settings
    AllowPower {
        /// Miner IP address or hostname (to set allowlist for a specific miner)
        host: Option<String>,
        /// Power setting to add or remove
        power: Option<u32>,
        /// Remove the specified power setting from the allowlist
        #[arg(short, long)]
        remove: bool,
        /// List all allowed power settings
        #[arg(short, long)]
        list: bool,
        /// Apply allowlist change to all stored miners
        #[arg(short, long)]
        all: bool,
    },
    /// Set a rate limit (in seconds) between allowed set-power commands
    RateLimit {
        /// Miner IP address or hostname
        host: Option<String>,
        /// Rate limit in seconds
        seconds: Option<u64>,
        /// List the current rate limit(s)
        #[arg(short, long)]
        list: bool,
        /// Apply rate limit to all stored miners
        #[arg(short, long)]
        all: bool,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Miner {
    host: String,
    username: String,
    password: Option<String>,
    #[serde(default)]
    power_allowlist: Vec<u32>,
    #[serde(default)]
    rate_limit_seconds: Option<u64>,
    #[serde(default)]
    last_set_power_timestamp: Option<u64>,
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
        let content = fs::read_to_string(&config_path)?;
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
        Commands::Status { host, all } => {
            status(host, all).await?;
        }
        Commands::SetPower { host, power, all } => {
            set_power(host, power, all).await?;
        }
        Commands::AllowPower { host, power, remove, list, all } => {
            allow_power(host, power, remove, list, all).await?;
        }
        Commands::RateLimit { host, seconds, list, all } => {
            rate_limit(host, seconds, list, all).await?;
        }
    }

    Ok(())
}

async fn status(host_arg: Option<String>, all: bool) -> Result<()> {
    let config = Config::load()?;
    let miners_to_check = if all {
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

    if miners_to_check.is_empty() {
        println!("No miners to check.");
        return Ok(());
    }

    for miner in miners_to_check {
        if let Err(e) = status_miner(&miner).await {
            eprintln!("Failed to get status for miner {}: {}", miner.host, e);
        }
        println!(); // Add a newline between miners
    }

    Ok(())
}

async fn status_miner(miner: &Miner) -> Result<()> {
    println!("Getting status for {}...", miner.host);

    let client = Client::builder()
        .cookie_store(true)
        .build()?;

    let base_url = if miner.host.contains("://") {
        miner.host.clone()
    } else {
        format!("http://{}", miner.host)
    };
    let gql_url = format!("{}/graphql", base_url);

    // 1. Login to get session cookie
    let login_payload = serde_json::json!({
        "query": "mutation ($username: String!, $password: String!) {\n  auth {\n    login(username: $username, password: $password) {\n      ... on Error {\n        message\n        __typename\n      }\n      __typename\n    }\n    __typename\n  }\n}\n",
        "variables": {
            "username": miner.username,
            "password": miner.password.as_deref().unwrap_or("")
        }
    });

    let login_response = client
        .post(&gql_url)
        .json(&login_payload)
        .send()
        .await;
    
    match login_response {
        Ok(resp) => {
            if !resp.status().is_success() {
                anyhow::bail!("Login failed with status: {}", resp.status());
            }
            let login_body: serde_json::Value = resp.json().await.context("Failed to parse login response")?;
            if login_body["data"]["auth"]["login"]["__typename"] != "VoidResult" {
                let error_message = login_body["data"]["auth"]["login"]["message"].as_str().unwrap_or("Unknown error");
                anyhow::bail!("Login failed: {}", error_message);
            }
        },
        Err(e) => {
            anyhow::bail!("Failed to connect to miner for login: {}", e);
        }
    }

    // 2. Query status (Trying a query instead of subscription)
    let query_payload = serde_json::json!({
        "query": "query {\n  bosminer {\n    info {\n      __typename\n      summary {\n        poolStatus\n        tunerStatus\n        realHashrate {\n          mhs5S\n          mhsAv\n        }\n        temperature {\n          degreesC\n          name\n        }\n        power {\n          approxConsumptionW\n          limitW\n        }\n      }\n      fans {\n        name\n        rpm\n      }\n    }\n    __typename\n  }\n}\n"
    });

    let query_response = client
        .post(&gql_url)
        .json(&query_payload)
        .send()
        .await;
    
    let query_body: serde_json::Value = match query_response {
        Ok(resp) => {
            if !resp.status().is_success() {
                anyhow::bail!("Status query failed with status: {}", resp.status());
            }
            let body: serde_json::Value = resp.json().await.context("Failed to parse status response")?;
            body
        },
        Err(e) => {
            anyhow::bail!("Failed to send status query: {}", e);
        }
    };
    let info = &query_body["data"]["bosminer"]["info"];

    if !info.is_null() {
        print_status(info);
    } else {
        anyhow::bail!("Failed to retrieve status: bosminer.info is null or invalid. Response: {}", query_body);
    }

    Ok(())
}

fn print_status(info: &serde_json::Value) {
    let summary = &info["summary"];
    println!("Status: {}", summary["poolStatus"].as_str().unwrap_or("N/A"));
    println!("Tuner: {}", summary["tunerStatus"].as_str().unwrap_or("N/A"));
    
    let hashrate = &summary["realHashrate"];
    let mhs_5s = hashrate["mhs5S"].as_f64().unwrap_or(0.0);
    let mhs_av = hashrate["mhsAv"].as_f64().unwrap_or(0.0);
    println!("Hashrate (5s): {:.2} TH/s", mhs_5s / 1_000_000.0);
    println!("Hashrate (Av): {:.2} TH/s", mhs_av / 1_000_000.0);

    let temp = &summary["temperature"];
    if temp.is_array() {
        if let Some(t) = temp.as_array().and_then(|a| a.first()) {
            println!("Temperature: {}°C ({})", t["degreesC"], t["name"].as_str().unwrap_or("N/A"));
        }
    } else {
        println!("Temperature: {}°C ({})", temp["degreesC"], temp["name"].as_str().unwrap_or("N/A"));
    }

    let power = &summary["power"];
    println!("Power: {}W / {}W limit", power["approxConsumptionW"], power["limitW"]);

    if let Some(fans) = info["fans"].as_array() {
        print!("Fans: ");
        let fan_info: Vec<String> = fans.iter().map(|f| {
            format!("{}: {} RPM", f["name"].as_str().unwrap_or("?"), f["rpm"])
        }).collect();
        println!("{}", fan_info.join(", "));
    }
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

async fn set_power(host_arg: Option<String>, power: u32, all: bool) -> Result<()> {
    let mut config = Config::load()?;
    
    let hosts_to_set = if all {
        config.miners.keys().cloned().collect::<Vec<_>>()
    } else if let Some(host) = host_arg {
        if config.miners.contains_key(&host) {
            vec![host]
        } else {
            anyhow::bail!("Miner {} not found in config. Please login first.", host);
        }
    } else {
        anyhow::bail!("Please specify a host or use --all");
    };

    if hosts_to_set.is_empty() {
        println!("No miners to update.");
        return Ok(());
    }

    let mut changed = false;
    for host in hosts_to_set {
        let miner = config.miners.get(&host).unwrap().clone();
        
        // Check miner-specific allowlist
        if !miner.power_allowlist.is_empty() && !miner.power_allowlist.contains(&power) {
            println!("Error: Power setting {}W is not in the allowlist for miner {}.", power, miner.host);
            println!("Miner allowed settings: {:?}", miner.power_allowlist);
            continue;
        }

        // Check rate limit
        if let Some(limit) = miner.rate_limit_seconds {
            if let Some(last_ts) = miner.last_set_power_timestamp {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs();
                let elapsed = now.saturating_sub(last_ts);
                if elapsed < limit {
                    println!("Error: Rate limit active for miner {}. Please wait {} more seconds.", miner.host, limit - elapsed);
                    continue;
                }
            }
        }

        if let Err(e) = set_power_miner(&miner, power).await {
            eprintln!("Failed to set power for miner {}: {}", miner.host, e);
        } else {
            // Update last_set_power_timestamp on success
            if let Some(m) = config.miners.get_mut(&host) {
                m.last_set_power_timestamp = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_secs()
                );
                changed = true;
            }
        }
    }

    if changed {
        config.save()?;
    }

    Ok(())
}

async fn set_power_miner(miner: &Miner, power: u32) -> Result<()> {
    println!("Setting power target to {}W on {}...", power, miner.host);

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

    // 2. Send set power mutation
    let mutation_payload = serde_json::json!({
        "query": "mutation ($tuneInput: AutotuningIn!, $apply: Boolean!) {\n  bosminer {\n    config {\n      updateAutotuning(input: $tuneInput, apply: $apply) {\n        ... on AttributeError {\n          message\n          __typename\n        }\n        ... on AutotuningError {\n          mode\n          message\n          performanceScaling {\n            powerStep\n            shutdownDuration\n            minPowerTarget\n            hashrateStep\n            minHashrateTarget\n            __typename\n          }\n          powerTarget\n          hashrateTarget\n          __typename\n        }\n        ... on AutotuningOut {\n          __typename\n        }\n        __typename\n      }\n      __typename\n    }\n    __typename\n  }\n}\n",
        "variables": {
            "tuneInput": {
                "powerTarget": power
            },
            "apply": true
        }
    });

    let response = client
        .post(&url)
        .json(&mutation_payload)
        .send()
        .await
        .context("Failed to send set power command")?;

    if !response.status().is_success() {
        anyhow::bail!("Set power command failed with status: {}", response.status());
    }

    let body: serde_json::Value = response.json().await.context("Failed to parse response body")?;
    
    // Expected success: {"data":{"bosminer":{"__typename":"BosminerMutation","config":{"__typename":"BosminerConfigurator","updateAutotuning":{"__typename":"AutotuningOut"}}}}}
    let update_result = &body["data"]["bosminer"]["config"]["updateAutotuning"];
    let typename = update_result["__typename"].as_str().unwrap_or("");

    match typename {
        "AutotuningOut" => {
            println!("Successfully set power target to {}W on {}.", power, miner.host);
        },
        "AttributeError" | "AutotuningError" => {
            let error_message = update_result["message"].as_str().unwrap_or("Unknown error");
            anyhow::bail!("Failed to set power: {}", error_message);
        },
        _ => {
            anyhow::bail!("Unexpected response from miner: {}", typename);
        }
    }

    Ok(())
}

async fn allow_power(
    host_arg: Option<String>,
    power: Option<u32>,
    remove: bool,
    list: bool,
    all: bool,
) -> Result<()> {
    let mut config = Config::load()?;

    if list {
        if all {
            for miner in config.miners.values() {
                if !miner.power_allowlist.is_empty() {
                    println!("Miner {} allowlist: {:?}", miner.host, miner.power_allowlist);
                } else {
                    println!("Miner {} allowlist: empty (any value allowed)", miner.host);
                }
            }
        } else if let Some(host) = host_arg {
            if let Some(miner) = config.miners.get(&host) {
                if miner.power_allowlist.is_empty() {
                    println!("Power allowlist for {} is empty (any value allowed).", host);
                } else {
                    println!("Power allowlist for {}: {:?}", host, miner.power_allowlist);
                }
            } else {
                anyhow::bail!("Miner {} not found in config.", host);
            }
        } else {
            println!("Use --host <HOST> to see miner-specific allowlist or --all to see all.");
        }
        return Ok(());
    }

    let p = power.context("Please specify a power setting to add or remove")?;

    if all {
        for miner in config.miners.values_mut() {
            update_allowlist(&mut miner.power_allowlist, p, remove);
        }
        println!(
            "{} {}W {} all miners' allowlists.",
            if remove { "Removed" } else { "Added" },
            p,
            if remove { "from" } else { "to" }
        );
    } else if let Some(host) = host_arg {
        if let Some(miner) = config.miners.get_mut(&host) {
            update_allowlist(&mut miner.power_allowlist, p, remove);
            println!(
                "{} {}W {} allowlist for {}.",
                if remove { "Removed" } else { "Added" },
                p,
                if remove { "from" } else { "to" },
                host
            );
        } else {
            anyhow::bail!("Miner {} not found in config.", host);
        }
    } else {
        anyhow::bail!("Please specify a host using --host <HOST> or use --all.");
    }

    config.save()?;
    Ok(())
}

fn update_allowlist(list: &mut Vec<u32>, power: u32, remove: bool) {
    if remove {
        if let Some(pos) = list.iter().position(|&x| x == power) {
            list.remove(pos);
        }
    } else {
        if !list.contains(&power) {
            list.push(power);
            list.sort_unstable();
        }
    }
}

async fn rate_limit(
    host_arg: Option<String>,
    seconds: Option<u64>,
    list: bool,
    all: bool,
) -> Result<()> {
    let mut config = Config::load()?;

    if list {
        if all {
            for miner in config.miners.values() {
                if let Some(limit) = miner.rate_limit_seconds {
                    println!("Miner {} rate limit: {}s", miner.host, limit);
                } else {
                    println!("Miner {} rate limit: none", miner.host);
                }
            }
        } else if let Some(host) = host_arg {
            if let Some(miner) = config.miners.get(&host) {
                if let Some(limit) = miner.rate_limit_seconds {
                    println!("Miner {} rate limit: {}s", host, limit);
                } else {
                    println!("Miner {} rate limit: none", host);
                }
            } else {
                anyhow::bail!("Miner {} not found in config.", host);
            }
        } else {
            println!("Use --host <HOST> to see miner-specific rate limit or --all to see all.");
        }
        return Ok(());
    }

    let s = seconds; // This can be None to remove the rate limit

    if all {
        for miner in config.miners.values_mut() {
            miner.rate_limit_seconds = s;
        }
        if let Some(val) = s {
            println!("Set rate limit to {}s for all miners.", val);
        } else {
            println!("Removed rate limit for all miners.");
        }
    } else if let Some(host) = host_arg {
        if let Some(miner) = config.miners.get_mut(&host) {
            miner.rate_limit_seconds = s;
            if let Some(val) = s {
                println!("Set rate limit to {}s for {}.", val, host);
            } else {
                println!("Removed rate limit for {}.", host);
            }
        } else {
            anyhow::bail!("Miner {} not found in config.", host);
        }
    } else {
        anyhow::bail!("Please specify a host using --host <HOST> or use --all.");
    }

    config.save()?;
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
                    power_allowlist: Vec::new(),
                    rate_limit_seconds: None,
                    last_set_power_timestamp: None,
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
