# AUR packages

Two packages for Arch Linux:

| Package | What it installs |
|---|---|
| `rwpm-bin` | The Linux release tarball from GitHub. |
| `rwpm` | A build from the release tag's source. It conflicts with `rwpm-bin`. |

Both put the app in `/opt/rwpm`, link `/usr/bin/rwpm` to it, and add a menu entry, icons and the `.rimrim` and `.rimpack` file types. The app finds its payload files beside its own file, so the folder must stay together.

The `PKGBUILD` files here are templates. Their `pkgver` and `sha256sums` are set again at each release. The packaging files in this folder are under the 0BSD licence (see `LICENSE`). The app is MIT.

## Update on each release

The release workflow runs `scripts/aur.sh <version> --push` after it publishes a release. The step does nothing until the repository has an `AUR_SSH_KEY` secret.

1. Make a new SSH key pair for this job: `ssh-keygen -t ed25519 -f aur-key -C rwpm-ci`.
2. Add the public key to your AUR account under "My Account".
3. Add the private key as the repository secret `AUR_SSH_KEY`.
4. Push the first version of each package by hand (below). The job only updates packages that exist.

## First upload, by hand

```sh
scripts/aur.sh 1.14.0                  # dry run; prints the work folder
cd <work folder>/rwpm-bin
git -c init.defaultBranch=master init
git remote add aur ssh://aur@aur.archlinux.org/rwpm-bin.git
git add PKGBUILD .SRCINFO LICENSE
git commit -m "Initial import"
git push aur master
```

Do the same in `<work folder>/rwpm` with the `rwpm` remote.

## Test a package

```sh
cp PKGBUILD /some/empty/folder && cd /some/empty/folder
makepkg -f
```
