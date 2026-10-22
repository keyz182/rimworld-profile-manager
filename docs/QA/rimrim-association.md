# QA runbook: open `.rimrim` files from the file manager

Check that a double-click on a `.rimrim` file opens this app on the Restore question, and that uninstalling removes the link.

- **Platform:** Windows 10 or 11 (installer), and Linux (tar.gz and AppImage). macOS is out of scope.
- **Time:** about 30 minutes.
- **Needs:** one `.rimrim` file made with "Back up…" (see `rimrim-backup.md`).
- **Not covered by the automated tests:** the Windows steps and every double-click. The tests check the files the installers write, and run the Linux script against a scratch home folder.

## What this does to your computer

- Windows: the installer writes keys under `HKCU\Software\Classes` (`.rimrim` and `rwpm.backup`). It does not need admin rights.
- Linux tar.gz: the script writes `~/.local/share/applications/rwpm.desktop`, `~/.local/share/mime/packages/rwpm.xml` and the icons under `~/.local/share/icons/`.
- Rollback is in the last section.

## Windows (installer)

1. Uninstall any older version of the app. Close the app.
2. Run the new `…-windows-x86_64-setup.exe`. Finish the installer.
3. Open Registry Editor. Expected: `HKCU\Software\Classes\.rimrim` has the default value `rwpm.backup`. `HKCU\Software\Classes\rwpm.backup\shell\open\command` is `"<install folder>\rwpm.exe" "%1"`.
4. In Explorer, find the `.rimrim` file. Expected: it shows the app icon and the type "RimWorld Profile Manager backup".
5. Double-click the file. Expected: the app opens and the "Restore a backup" question shows for that file. No console window opens.
6. Choose "Cancel". Leave the app open. Double-click the file again. Expected: the same window shows the question again. No second window opens. Leave the question open and double-click another `.rimrim` file. Expected: the question stays, and a notice tells you to finish what is open and open the file again. Choose "Cancel".
7. Put the `.rimrim` file in a folder whose name has a space and a `&`. Double-click it. Expected: the question shows (the path is handled as one argument).
8. On a computer with no profiles yet (first start), double-click the file. Expected: the first-run setup shows. Finish it. Choose "Restore backup…" and pick the file. (The file from the double-click is not kept through setup.)
9. Uninstall the app from Settings. Expected: both keys are gone. Double-click the file. Expected: Windows asks which app to use.
10. Install again. In Registry Editor set `HKCU\Software\Classes\.rimrim` to `other.app`. Uninstall. Expected: `.rimrim` still reads `other.app` (the installer leaves a link that is not its own), and `rwpm.backup` is gone.

## Linux, tar.gz

1. Unpack the bundle. Run `./install-desktop-entry.sh`. Expected: it says the app is in the menu and that `.rimrim` files open in it.
2. Run `xdg-mime query filetype backup.rimrim`. Expected: `application/x-rimrim`. Run `xdg-mime query default application/x-rimrim`. Expected: `rwpm.desktop`.
3. Double-click the file in your file manager. Expected: the app opens on the Restore question.
4. Move the unpacked folder. Run the script again from the new place. Expected: the double-click still works.
5. Run `./install-desktop-entry.sh --remove`. Expected: `xdg-mime query default application/x-rimrim` prints nothing. The menu entry is gone.

## Linux, AppImage

1. Add the AppImage with your AppImage tool (for example AppImageLauncher).
2. Expected: the tool lists `.rimrim` for the app, and a double-click opens the Restore question. If your tool does not read the type, note it in the QA result. "Restore backup…" still works.

## Rollback

1. Windows: uninstall the app. Delete the `.rimrim` test files.
2. Linux: run `./install-desktop-entry.sh --remove`. Delete the test files and any AppImage entry.
3. Delete any profiles made by the restore checks.
