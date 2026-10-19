# Modpacks

A modpack is a small file that names the mods of a profile, in load order. The file name ends in `.rimpack`. Use a modpack to share a setup with other players.

A modpack holds no mods and no game files. It holds the mod list and what you choose to add. The person who opens it gets the mods from Steam.

Opening a modpack always makes a new profile. It never changes a profile that you already have.

## Make a modpack

Close RimWorld first. Start the game once with the profile, so that it has a mod list.

1. Select a profile. Choose the "⋯" button, then "Share as modpack…". Any profile can be shared, also the Default one.
2. Tick what goes in the modpack. See "What you can tick".
3. Type a description and a link to a Steam collection if you want. Both are optional.
4. Choose "Share". Choose a name and a folder for the file.

The app writes the file in the background. If something goes wrong, the app removes the half-written file.

### What you can tick

| Box | What it adds |
|---|---|
| "Mod settings" | The settings files of your mods. This box starts ticked. See "Private details". |
| Scenarios, ideoligions, xenotypes and exported pawns | The things you made in the game. This box starts ticked. |
| Game settings | Screen size and key bindings. This box starts unticked. |
| Saves | One tick for each save. They start unticked. If you tick a save, you can choose one of them as the save to start from. |

The mod list is always in. The app also records, for each mod, its Steam number, the time Steam last updated it and its version when the mod says one.

### Private details

> Mod settings can hold private details, such as account logins. Only share settings you have checked.

The box shows how many files it adds and how big they are. Untick the box if you are not sure.

### What the app leaves out

A file that is not plain data is not put in the modpack. A mod that the app cannot list safely is left out of the list. The app tells you how many.

## Open a modpack

1. Under the profile list, choose "Open modpack…". Choose the `.rimpack` file. On Windows (installer) and Linux you can also double-click the file.
2. Read the question. Change the name of the new profile if you want.
3. Choose "Restore".

The question shows the mods in three lists:

| List | What it means |
|---|---|
| "Mods to get from Steam" | You do not have these mods. Each has an "Open Steam page" button. |
| "Mods to find yourself" | You do not have these mods, and the pack has no Steam number for them. The pack's own link is shown as text. |
| "Mods you already have" | These mods are installed. |

It also tells you if the pack needs an expansion that you do not have, if it was made for another game version, and if a mod that you have changed on Steam after the pack was made.

Each list shows the first 200 mods. A longer list ends with "and N more".

> This pack holds no mods. Mods may have changed since it was made.

### Get the missing mods

> Get the missing mods before you play. RimSort can fetch them if its Steam database is on: use Export to RimSort, then let RimSort download the rest.

1. Choose "Open Steam page" for each mod in the first list. Subscribe to it in Steam.
2. Or use "Export to RimSort" on the new profile. RimSort then offers to download the missing mods. It can only do this when its Steam database is switched on. That is RimSort's default.

The app can also get the mods for you, by one of two routes. It offers only the route that works on your computer.

#### With the Steam client

Use this when Steam is running. Choose "Restore and subscribe…". The app lists the mod names from the pack and then subscribes you in Steam. Steam keeps the subscriptions and updates the mods. "Remove subscriptions from the pack" in the More menu removes exactly the ones the app added.

Tick "Make it an OFFLINE profile" first to download private copies instead. The app does not subscribe you then, and the mods do not update.

#### With steamcmd

Use this when the app cannot reach Steam. It makes an OFFLINE profile and downloads the mods into it.

1. Open the pack. Choose "Restore and download…".
2. The first time, the app says what it will download and run. Choose "Download steamcmd and continue" to agree.
3. Check the list of mods. Choose "Download".

What happens:

- steamcmd is Valve's own downloader. The app downloads it from Valve once, over HTTPS, and keeps it in its own folder.
- The app saves a fingerprint of that file. If Valve's file changes later, the app stops before it makes a profile. It shows both fingerprints. Trust the new file only if you expect a change.
- steamcmd signs in to Steam without an account. It then updates itself from Valve. The app does not check those updates.
- The app downloads in batches. A pack can have any number of mods.
- If one mod fails, the profile stays LIVE and the app lists what failed.
- The app deletes steamcmd's copies of the mods after the profile has its own.
- On Linux, steamcmd needs the 32-bit system libraries. On a Mac, it needs Rosetta 2. The app says so when it cannot start.

In both routes, a mod must name the package id that the pack promised. If it does not, the app does not keep it.

## Limits

- The Workshop has no versions. A mod may have changed since the pack was made. A save may then not load.
- If you have no RimWorld from Steam (for example from GOG), the app cannot check what you have. It lists every mod as one to get.
- The app opens only Steam pages from the button, and only addresses built from a Steam number. A link inside a pack is text.
- A pack that is too big, or that holds anything it should not, is refused with a plain-words reason. Nothing is made.
- macOS: the file does not open by double-click yet. Use "Open modpack…".
