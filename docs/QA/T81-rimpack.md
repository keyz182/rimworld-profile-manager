# T81 QA runbook: share and open a modpack

Check that a modpack made from a real profile holds only what it should, and that opening it in another place shows the right mods.

- **Platform:** Linux, Windows and macOS. Do the checks on each platform that you ship for.
- **Time:** about 45 minutes.
- **Needs:** Steam with RimWorld, a profile with at least five mods (some from Steam, one local mod, one mod with a settings file), two saves, and a second computer or user account that lacks some of those mods. RimSort is needed for step 12.
- **Not covered by the automated tests:** a real Steam, the system file dialogs, a click on "Open Steam page" (the browser), RimSort, and any other computer. The tests cover the format, the refusals, the lists, and the dialogs in a window of 960 × 600.

## What this does to your computer

- Share writes one `.rimpack` file where you choose. It reads the profile and changes nothing in it.
- Open makes one new LIVE profile. It changes no profile that exists.
- Rollback is in the last section.

## Share

1. Close RimWorld. Start the app. Select the profile. Choose "⋯", then "Share as modpack…". Expected: the question shows. The note about private details shows under "Mod settings". Each box shows a file count and a size.
2. Make the window as small as it goes. Expected: the title and the "Share" and "Cancel" buttons stay on screen. The middle scrolls.
3. Tick both saves. Choose one as the save to start from. Untick that save. Expected: no save is chosen as the start save.
4. Type a bad link (`nope`). Expected: a warning shows and "Share" is off. Type `https://steamcommunity.com/sharedfiles/filedetails/?id=1`. Expected: the warning goes.
5. Untick the saves. Choose "Share". Save the file. Expected: the app says how big the file is. No file is left half-written.
6. List the file: `tar --zstd -tf <file>`. Expected: `rimpack.toml` first, then `SaveData/Config/ModsConfig.xml`, then settings files. No `Mods/` folder, no `.dll`, no game files, no saves.
7. Read `rimpack.toml`: `tar --zstd -xOf <file> rimpack.toml`. Expected: each Steam mod has `workshop_id`, `time_updated` and a Steam link. The local mod has no `workshop_id`. The core game is not listed. The game version is there.

## Open

8. On the second computer, start the app. Choose "Open modpack…" and pick the file. Expected: the question shows the pack name and description. "Mods to get from Steam", "Mods to find yourself" and "Mods you already have" hold the right mods.
9. Choose "Open Steam page" for one mod. Expected: your browser opens the Steam page for that mod. Nothing else opens.
10. Change the name. Choose "Restore". Expected: a new LIVE profile shows and is selected. Its mod list has the same order as the first profile.
11. Without installing the mods, choose Play on the new profile once, then quit the game from its menu. Open the profile's mod list in the app. Expected: it still lists every mod of the pack (the load order is kept; see R55). If it does not, write down what changed.
12. Choose "Export to RimSort" for the new profile. In RimSort, refresh. Expected: RimSort offers to download the missing mods. If RimSort's Steam database is off, the window is empty (R54): note it.

## Refusals

13. Rename a copy of any text file to `bad.rimpack`. Open it. Expected: a plain-words message. No dialog, no new profile.
14. Open the same pack twice. Expected: two profiles with different names.

## Rollback

1. Delete the new profiles that the checks made.
2. Delete the `.rimpack` test files.
