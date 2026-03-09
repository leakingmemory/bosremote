---
name: bosremote
description: Remote control Braiins OS miners using the `bosremote` CLI tool.
---

When the user asks for information about miners, use the `bosremote status` command.
When the user wants to set a power target for a miner, use the `bosremote set-power` command.
To stop a miner, use the `bosremote stop` command.
To start a miner, use the `bosremote start` command.

The tool handles safety limits like power allowlists, rate limiting, and stop-start delays automatically.
Always specify the host IP or hostname, or use `--all` to apply the command to all stored miners.
