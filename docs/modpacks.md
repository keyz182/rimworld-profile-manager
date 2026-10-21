# Modpacks

A modpack is a small file that names the mods of a profile, in load order. The file name ends in `.rimpack`. Use a modpack to share a setup with other players.

A modpack holds no mods and no game files. It holds the mod list and what you choose to add. The person who opens it gets the mods from Steam.

Opening a modpack always makes a new profile. It never changes a profile that you already have.

## Make a modpack

Close RimWorld first. Start the game once with the profile, so that it has a mod list.

1. Select a profile. Choose the "⋯" button, then "Share as modpack…". Any profile can be shared, also the Default one.
2. Tick what goes in the modpack. See "What you can tick".
3. Type a description, choose a cover picture and add a link to a Steam collection if you want. All three are optional. See "Description and cover".
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

#### Choose single items

Under "Mod settings", and under the box for the things you made, choose "Choose which". A list opens with one box for each item. All items start ticked. Untick the items that you do not want to share.

- Mod settings show one box for each settings file. If the file name has a Steam number that matches a mod in your list, the app shows the name of that mod first.
- The things you made show under the name of their folder: Scenarios, Ideoligions, Xenotypes and Exported pawns.
- The list shows only files that a modpack can hold.
- If you untick every item of a box, the app unticks the box. Tick the box again to tick all items.
- The size of the box changes to show what is ticked, for example "3 of 12".

### Description and cover

The description can use Markdown. Choose "Preview" to see how it will look. Choose it again to go back to editing. The description can hold up to 2000 characters.

| You type | What people see |
|---|---|
| `# Title` | A heading |
| `**bold**` and `*italic*` | Bold and italic text |
| `- item` or `1. item` | A list |
| `> text` | A quote |
| `` `code` `` | Code |
| `[words](https://…)` | A link |

Images and raw HTML are not drawn. An image shows as its alt text. The app never loads anything from the net for a description.

A link opens only if it goes to a Steam page. Any other link does nothing when you choose it.

To add a cover, choose "Choose picture…". The picture must be a PNG or a JPEG. The app shrinks it to fit 1024 pixels and keeps it as a PNG. The result can be 2 MiB at most. If the app cannot use the picture, it says why and adds nothing.

### Private details

> Mod settings can hold private details, such as account logins. Only share settings you have checked.

The box shows how many files it adds and how big they are. Untick the box if you are not sure. Or choose "Choose which" and share only the settings that you have checked.

### What the app leaves out

A file that is not plain data is not put in the modpack. A mod that the app cannot list safely is left out of the list. The app tells you how many.

## Open a modpack

1. Under the profile list, choose "Open modpack…". Choose the `.rimpack` file. On Windows (installer) and Linux you can also double-click the file.
2. Read the question. Change the name of the new profile if you want.
3. Choose "Restore".

The question shows the cover and the description of the pack, then the mods in three lists:

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

Steam downloads the mods in its own time. The app looks every 15 seconds. When Steam has every mod of the pack, the profile shows this question:

> Steam now has every mod of this pack. Take this profile offline, so that Steam updates cannot change it?

Choose "Take offline…" to start the normal offline steps. Choose "Not now" if you want Steam to keep the mods up to date. The app then does not ask again for that profile. You can still take the profile offline from the profile page at any time.

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
- On a Mac, steamcmd needs Rosetta 2. It is an Intel program. The app says so before it downloads anything. To install Rosetta, run `softwareupdate --install-rosetta` in Terminal.
- On Linux, steamcmd needs the 32-bit system libraries. The app says so when steamcmd cannot start.

In both routes, a mod must name the package id that the pack promised. If it does not, the app does not keep it.

## A profile made from a modpack

A profile that you made from a modpack remembers it. Its page shows a part named "From a modpack" with the name, the cover and the description of the pack.

The app keeps a copy of the `.rimpack` file in the profile folder. It is as big as the pack, so a pack with many saves takes that much room twice. The app checks the free space before it copies.

A copy of the profile (Clone) does not keep the pack. It has no link to the pack and no copy of the file.

### Reset to the pack

Use "Reset to pack…" to put the profile back as the pack had it.

1. Close RimWorld.
2. Select the profile. Choose "Reset to pack…".
3. Read the question. Tick "Also restore the pack's saves" if you want the saves back. The box starts unticked.
4. Choose "Reset".

What happens:

- The app puts back the mod list, the mod settings and the things you made, as the pack held them.
- A file that is different goes to the recycle bin first. You can get it back from there.
- A file that is the same stays. A file that the pack does not hold stays too.
- A save of the pack replaces a save with the same name, and only if you ticked the box.

An OFFLINE profile holds its own copies of the mods. A pack lists mods and does not hold them. If the profile has no copy of a mod that the pack lists, the app names the mod and does not reset. Get the mod first, then reset.

The app needs the kept pack file. If you removed it, the app says so.

## Limits

- The Workshop has no versions. A mod may have changed since the pack was made. A save may then not load.
- If you have no RimWorld from Steam (for example from GOG), the app cannot check what you have. It lists every mod as one to get.
- The app opens only Steam pages. The "Open Steam page" button builds its address from a Steam number. A link in a description opens only if it is a Steam page. Any other link in a pack is text or does nothing.
- A pack that is too big, or that holds anything it should not, is refused with a plain-words reason. Nothing is made.
- Older versions of the app cannot open a pack that has a cover. A pack without a cover opens in all versions.
- Older versions of the app cannot open a pack that shares only some of the mod settings or only some of the things you made. If you tick every item, the pack opens in all versions.
- Older versions of the app cannot read the profile of a pack that you opened with this version. Keep to one version, or make a copy of the profile first.
- macOS: the file does not open by double-click yet. Use "Open modpack…".
