# Backups

A backup puts an OFFLINE profile into one file. The file name ends in `.rimrim`. Use a backup to keep a safe copy of a profile, or to move a profile to another computer.

A restore always makes a new profile. It never changes a profile that you already have.

## Make a backup

Close RimWorld first. Check the free space (see "Disk use").

1. Select an OFFLINE profile. Choose the "⋯" button, then "Back up…".
2. Tick what goes in the backup. The mods and the settings of the profile are always in.
3. Choose "Back up". Choose a name and a folder for the file.

The app writes the file in the background. A bar shows the progress. Other changes wait until it ends. If something goes wrong, the app removes the half-written file. A file that already has that name stays until the new backup is complete.

Only OFFLINE profiles can be backed up. A LIVE profile uses your Steam Workshop mods and holds none of its own.

### What you can tick

| Box | What it adds |
|---|---|
| Saves | The saves of the profile. This box starts ticked. Without it, the backup keeps only the game settings. |
| "Log files" | The log files of the profile. They help when you ask for support. |
| Game files | The stored game version that the profile plays. This box starts unticked. It shows only when the profile plays a stored game version. |

## Restore a backup

1. Under the profile list, choose "Restore backup…". Choose the `.rimrim` file.
2. Read the question. Change the name of the new profile if you want.
3. Choose "Restore".

The new profile appears in the list and is selected. The app tells you if something was not restored (see "Game files").

## Share a backup

A backup holds other people's mods. The app shows this note before every backup and every restore:

> Check each mod's licence or ask its author before you share this backup.

A backup that holds game files holds RimWorld's own files. The app shows this note too:

> This backup holds RimWorld's game files. Do not share it.

## Game files

A backup can hold the game version that the profile plays. Restore treats it like this:

- The computer has no game version with that name. Restore stores it.
- The computer has that game version. Restore keeps its own copy and does not read the one in the backup. If the two copies differ, the app tells you.
- The backup holds game files for another system, for example Windows files on a Linux computer. Restore does not store them. The profile still plays that game version, so the app tells you to capture it or to choose another one.
- The backup holds no game files, and the computer has no such game version. The profile still names it. The app tells you to capture it or to choose another one.

## Disk use

The file holds the mods and the settings, and the boxes that you tick. The app compresses the data. Mods and game files often compress little, so plan for the size of the profile, plus the game if you tick "Game files". The app checks the free space and tells you if there is not enough.

## Limits

- Backup and restore wait while RimWorld runs, and while another long job runs.
- A mods folder or a save folder that holds a link, or a file whose name has `\` or `:`, cannot be backed up. The app names the files.
- In the game files, a link that points outside the game folder is left out. Steam makes some on Linux. The app names them after the backup.
- A backup cannot be larger than 128 GB.
- On macOS, choose "Restore backup…" to open a file. Opening a `.rimrim` file from Finder is not supported yet.
- The first-run setup comes first. When you start the app for the first time, finish the setup, then choose "Restore backup…".

## The file

A `.rimrim` file is a tar archive compressed with zstd. The first entry is `rimrim.toml`. It holds the name, the version of this app, the operating system, the sizes and the two notes above. The app reads only this entry to show you the question before a restore. Restore checks every entry before it writes anything, and it refuses a file that does not follow the rules.
