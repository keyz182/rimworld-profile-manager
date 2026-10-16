# T74 QA runbook: back up and restore an OFFLINE profile

Check that a `.rimrim` backup restores into a new profile, and that bad files are refused.

- **Platform:** Linux or Windows. On macOS use "Restore backup…" (opening from Finder comes later).
- **Time:** about 30 minutes, plus the copy time.
- **Scope:** the Back up and Restore dialogs and the files they make. The file association is checked in the T75 runbook.

## What this does to your disk

- A backup file the size of the profile (up to the size of the profile plus the game, if you tick "Game files").
- A restore makes a new profile folder of the same size. If the backup holds game files that are not stored yet, it also stores a game version.
- The checks leave these behind. See "Rollback".

## Before you start

1. Close RimWorld and the app.
2. Check the free space is more than twice the size of the profile you will use.
3. Pick an OFFLINE profile with a few saves. Write down its name. Call it "Source".

## Steps

1. **Offer.** Select "Source". Open "⋯". Expected: "Back up…" is in the menu. Select a LIVE profile and open "⋯". Expected: no "Back up…". Select Default. Expected: no "Back up…".
2. **Dialog.** Select "Source". Choose "Back up…". Expected: "Saves" is ticked; "Log files" and "Game files" are not. The note "Check each mod's licence or ask its author before you share this backup." shows. Tick "Game files" (when it shows). Expected: the note "This backup holds RimWorld's game files. Do not share it." shows too.
3. **Backup.** Untick "Game files". Choose "Back up". Name the file `qa-source` and save it. Expected: a bar shows progress; the line "Backed up “Source” to …/qa-source.rimrim" shows. The file is `qa-source.rimrim`. No `.qa-source.rimrim.tmp` file is left beside it.
4. **Restore.** Choose "Restore backup…". Pick `qa-source.rimrim`. Expected: the question shows the name, the size and the free space, and the mods note. Change the name to "QA Restored". Choose "Restore".
5. **Check the result.** Expected: "QA Restored" is in the list and selected. It is OFFLINE with the same mod count. Its saves are there, in the same order (newest first). "Source" is unchanged.
6. **Play.** Start "QA Restored". Expected: the game starts with the frozen mods and loads a save from the backup.
7. **Restore again.** Restore the same file with the same name. Expected: a second profile appears. Nothing is overwritten.
8. **Game files.** Pin "Source" to a stored game version. Back up with "Game files" ticked. Expected: the backup is about the size of the game bigger. On the same computer, restore it. Expected: no note, or "A different copy of game version … is already stored here" if you changed the stored copy. The stored game version is not changed.
9. **Refusals.** Rename a text file to `notes.rimrim` and choose "Restore backup…" on it. Expected: a red line says it is not a backup. Start RimWorld, then choose "Back up…". Expected: the app says RimWorld is running; nothing starts.
10. **Cancel.** Choose "Restore backup…", then close the file dialog. Expected: nothing changes.

## Rollback

1. Delete the profiles "QA Restored" and its copy from the app ("⋯", then "Delete").
2. Delete `qa-source.rimrim` and any other `.rimrim` file you made.
3. If step 8 stored a game version that you do not want, delete it under "Game versions".
