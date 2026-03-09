# bosremote

`bosremote` is a CLI tool to remote control Antminers running Braiins OS.

## Overview

This tool allows you to manage and control multiple Braiins OS miners from your command line. It currently supports authenticating with miners and storing their credentials securely for future use.

**Note:** This tool has been tested for controlling Antminer S9s running Braiins OS. There are no guarantees for other models or versions, though it uses the standard Braiins OS GraphQL API.

## Installation

To build `bosremote`, you need to have Rust and Cargo installed.

```bash
git clone <repository-url>
cd bosremote
cargo build --release
```

The binary will be available at `target/release/bosremote`.

## Usage

### Login

To store credentials for a miner, use the `login` command. This will test the connection and, if successful, save the credentials to `~/.config/bosremote/miners.json`.

```bash
bosremote login <IP-OR-HOSTNAME> --username root --password <YOUR-PASSWORD>
```

Options:
- `-u, --username`: The username for the miner (default: `root`).
- `-p, --password`: The password for the miner.

### Stop

To stop the `bosminer` service on a miner, use the `stop` command. The miner must have been previously logged in.

```bash
# Stop a specific miner
bosremote stop <IP-OR-HOSTNAME>

# Stop all stored miners
bosremote stop --all
```

### Start

To start the `bosminer` service on a miner, use the `start` command. The miner must have been previously logged in.

```bash
# Start a specific miner
bosremote start <IP-OR-HOSTNAME>

# Start all stored miners
bosremote start --all
```

### Status

To get the current status of a miner, use the `status` command.

```bash
# Get status of a specific miner
bosremote status <IP-OR-HOSTNAME>

# Get status of all stored miners
bosremote status --all
```

### Set Power

To set the power target (in Watts) for a miner, use the `set-power` command. The miner must have been previously logged in.

If a power allowlist is configured, only values in the allowlist will be accepted.

```bash
# Set power target for a specific miner
bosremote set-power <IP-OR-HOSTNAME> <POWER-IN-WATTS>

# Set power target for all stored miners
bosremote set-power --all <POWER-IN-WATTS>
```

### Power Allowlist

To manage the allowlist of power settings for specific miners:

```bash
# Add a power setting to a specific miner's allowlist
bosremote allow-power 10.54.2.249 800

# Add a power setting to ALL miners' specific allowlists
bosremote allow-power --all 800

# Remove a power setting from a specific miner's allowlist
bosremote allow-power 10.54.2.249 800 --remove

# List allowed power settings for a specific miner
bosremote allow-power --host 10.54.2.249 --list

# List all allowlists for all miners
bosremote allow-power --all --list
```

When a miner's allowlist is empty, any power setting is allowed for that miner.

### Rate Limit

To set a rate limit in seconds between `set-power` commands for specific miners:

```bash
# Set a 60-second rate limit for a specific miner
bosremote rate-limit 10.54.2.249 60

# Set a 60-second rate limit for ALL miners
bosremote rate-limit --all 60

# Remove rate limit for a specific miner
bosremote rate-limit 10.54.2.249

# List rate limit for a specific miner
bosremote rate-limit --host 10.54.2.249 --list

# List all rate limits for all miners
bosremote rate-limit --all --list
```

The rate limit is enforced locally based on the last successful `set-power` command performed by `bosremote`.

### Configuration

The miner credentials are stored in a JSON file at:
`~/.config/bosremote/miners.json`

## Disclaimer

This software is provided "as is", without warranty of any kind. It was primarily tested on Antminer S9. Use it at your own risk.
