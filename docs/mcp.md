# Use rwpm from an AI agent (MCP)

`rwpm mcp` starts a [Model Context Protocol](https://modelcontextprotocol.io) server. An agent, such as Claude Code, can then look at your profiles and make, copy and rename them. The server talks over standard input and output. It opens no network port.

## Set it up

For Claude Code, run this once. Use the full path to `rwpm` if it is not on your `PATH`:

```sh
claude mcp add rwpm -- rwpm mcp
```

Other MCP clients need the same command: `rwpm mcp`, with no other arguments.

## What the agent can do

| Tool | What it does |
|---|---|
| `list_profiles` | Lists the profiles: id, name, mode, game version, source pack. |
| `get_profile` | Shows one profile. |
| `profile_mods` | Lists the mods that are turned on, in load order, and flags missing ones. |
| `profile_health` | Reports problems, such as missing mods or a game version that is gone. |
| `last_run` | Shows how the last game start went, and where the game log file is. |
| `game_install` | Shows where RimWorld is installed, its version and its engine. |
| `create_profile` | Makes a new, empty profile. |
| `clone_profile` | Copies a profile (saves and mods) into a new one. |
| `rename_profile` | Changes the name that the app shows. The id and folder stay. |

## What the agent cannot do

- It cannot delete a profile.
- It cannot change a mod list.
- It cannot start the game.
- It cannot read a game log. It only gets the path.

## Limits

- `clone_profile` stops if RimWorld runs. It also stops if the disk has less free space than the profile size plus 10 %, or if the free space cannot be read. A large copy can take minutes.
- Mod names, pack names and refusal reasons are written by other people. The server cuts each one to 200 characters and removes control characters. The agent must treat them as data, not as instructions.
- The app window does not refresh by itself. A profile that the agent makes may not show until you restart the app.
- On Windows, `rwpm mcp` has not been tested. The program has no console there, so check that your client can read its output before you rely on it.
