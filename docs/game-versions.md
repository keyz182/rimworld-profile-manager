# Game versions

A game version is a full copy of RimWorld that this app keeps. A profile can play one. So a profile can stay on 1.6 while Steam moves to 1.7.

Each profile keeps its own saves, settings and mod list. A game version changes only which game the profile starts.

## Store a game version

Close RimWorld first. Check the free space (see "Disk use").

1. In Steam, switch RimWorld to the version that you want to store. Wait until Steam finishes the update.
2. In this app, choose "Game versions" at the top. Then choose "Capture current game".
3. Read the size and the free space in the question. Choose "Store it".
4. In Steam, switch RimWorld back to the version that you normally play.

The app copies the game in the background. A bar shows the progress. Other changes wait until the copying ends.

If the app already holds that game version, it copies nothing. It tells you the name of the stored game version.

## Use a game version

1. Select the profile. In the "Game version" section, choose "Change…".
2. Choose the game version. Then choose "Use it".

The section now shows "Plays:" and the name of the game version. The saves, settings and mods of the profile do not change.

To go back, choose "Change…" and pick "Steam install".

The Default profile cannot use a stored game version. It always plays the Steam install, because it shares the saves of the game itself.

### Older game versions and saves

A save made in a newer game version can fail to open in an older one. When the profile has saves and you choose an older game version, the app asks:

> Saves made in 1.7 may not open in 1.6. Change anyway?

Choose "Cancel" to keep the profile as it is. Choose "Change it" to go on. The app does not make a backup of the saves.

### Start from Steam or RimSort

A profile with a stored game version starts that game version from the app. It does the same from RimSort. From Steam, it does the same when you use the launch options that this app shows.

Steam's own Play button, without those launch options, starts the Steam install. It starts it as the Default profile. On 1.7 the game then clears the mod list of Default (see the README, "How it works").

Workshop mods still come from Steam. A stored game version does not hold them.

## Update a stored game version

Steam can change a game version that you stored. It can issue a new build with the same version number. You can add or remove an expansion. You can also switch to a different Steam branch.

In these cases, "Capture current game" offers "Update it". The app copies the new files first. Then it swaps them in. The old files stay until the new files are ready. If the update fails, the old files stay and still work.

Every profile that uses the game version gets the new files. The "Mods" folder inside the game version is kept. Local mods that you put there stay.

## Delete a stored game version

1. Choose "Game versions" at the top.
2. Choose "Delete" beside the game version. Then choose "Delete it".

The app moves the game version to the recycle bin.

You cannot delete a game version while a profile uses it. The list shows the names of those profiles. Change their game version first.

If the app cannot read the file of a profile, it cannot tell if that profile uses the game version. The list then shows the name of that profile and keeps "Delete" off. Repair or remove that profile first.

## Disk use

A stored game version is a full copy of the game folder. Profiles that use the same game version share it. A second profile adds no disk use for the game.

On a file system with copy-on-write, the copying shares data with the original. It takes almost no extra space until Steam changes the original. On other file systems, the copying takes the full size.

These measurements are examples. Your numbers depend on the game version, the file system and the expansions that you own.

| System | File system | Game version | Result |
|---|---|---|---|
| Linux | btrfs | 1.6.4871 | The game is 1.65 GiB. The copying took 1 second and no extra space. |
| macOS | APFS | 1.6.4871 rev595 | The game is 948 MiB. The copying took 15 seconds and about 11 MiB extra. |
| Windows | NTFS | 1.6.4871 rev590 | The game is 920 MiB. The copying took 3 seconds and the full 920 MiB. |

After Steam changes the original, the stored game version keeps the files that changed. In the Linux test, the stored game version then owned 871 MiB of the 1.65 GiB.

An update keeps the old files until the new files are ready. On a file system without copy-on-write, you need room for both. The size in the question counts the new files only.

The app refuses to start when the free space is smaller than the size of the game.

### Where the game versions are

The game versions are in the "builds" folder inside your profiles folder. Each game version has a folder that is named after it, for example `1.6.4871-rev598`.

| System | Default place |
|---|---|
| Linux | `~/.local/share/RimWorld Profile Manager/profiles/builds/` |
| Linux, Flatpak Steam | `~/.var/app/com.valvesoftware.Steam/data/RimWorld Profile Manager/profiles/builds/` |
| Windows | `%LOCALAPPDATA%\RimWorld Profile Manager\profiles\builds\` |
| macOS | `~/Library/Application Support/RimWorld Profile Manager/profiles/builds/` |

If you move the profiles folder in Settings, the game versions move with it. The size check for the move includes them.

## Limits

- **No Steam overlay on Windows for 1.6.** The game runs. It connects to Steam and shows your Workshop mods. The overlay does not open. See [issue 5](https://github.com/keyz182/rimworld-profile-manager/issues/5).
- **No cancel button.** If you close the app during the copying, the old game version stays safe. The app removes the partial game version when you open "Game versions" or capture again. Until then, it uses disk space.
- **Steam and RimSort cannot start a profile during the copying.** The start is refused. The message says "another profile is already being launched or played; close it first". The Play button in the app waits and says that the app is busy.
- **RimWorld must be closed.** While RimWorld runs, the "Capture current game" and "Delete" buttons stay off. The app says "RimWorld is running. Close it first." If the game starts later, the action stops at once. The message says "RimWorld is running; close the game first".
- **Steam must have finished its update.** If Steam is still updating, the app says "Steam is updating". Try again in a minute.
- **A Steam install is needed.** The app cannot store a game from another shop.
- **Stored game versions do not get Steam updates.** To get a newer build, update the game in Steam. Then choose "Capture current game" and "Update it".
