# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

- - -
## [v1.14.0](https://github.com/keyz182/rimworld-profile-manager/compare/beb2d97e40cb5b613b9ff376bfe6520861cb3c9b..v1.14.0) - 2026-10-21
#### Features
- choose single mod settings and creations when sharing a pack - ([133346d](https://github.com/keyz182/rimworld-profile-manager/commit/133346da188f4848c125b3400c456c4feacdeb9e)) - [@keyz182](https://github.com/keyz182)
- let a pack list single mod settings and creations (schema 3) - ([beb2d97](https://github.com/keyz182/rimworld-profile-manager/commit/beb2d97e40cb5b613b9ff376bfe6520861cb3c9b)) - [@keyz182](https://github.com/keyz182)
#### Documentation
- describe choosing single mod settings and creations - ([3872371](https://github.com/keyz182/rimworld-profile-manager/commit/3872371a1fd790a8d2689986901ede4171901eff)) - [@keyz182](https://github.com/keyz182)

- - -

## [v1.13.0](https://github.com/keyz182/rimworld-profile-manager/compare/b6dc8dd0b6591c19d52fe141617cd7004a059f73..v1.13.0) - 2026-10-21
#### Features
- offer to take a pack profile offline once Steam has its mods - ([62eea2c](https://github.com/keyz182/rimworld-profile-manager/commit/62eea2cee7b07c31b52e9efa0da8595dcad6fc07)) - [@keyz182](https://github.com/keyz182)
- tell when Steam has fetched a pack's mods, and remember "Not now" - ([c877232](https://github.com/keyz182/rimworld-profile-manager/commit/c8772325a4e4d8f518c8c617eb743a75533a2df1)) - [@keyz182](https://github.com/keyz182)
- show a pack profile's pack and reset it to the pack - ([74ac2f5](https://github.com/keyz182/rimworld-profile-manager/commit/74ac2f5a53d32c1d60cef668f6e2bbd6cff324cd)) - [@keyz182](https://github.com/keyz182)
- choose a cover and write a markdown description when sharing a pack - ([eebfb9a](https://github.com/keyz182/rimworld-profile-manager/commit/eebfb9a7b6a2a6274408b42ff12a0fc14b943c2c)) - [@keyz182](https://github.com/keyz182)
- show a pack's cover and markdown description when opening it - ([e109148](https://github.com/keyz182/rimworld-profile-manager/commit/e109148c5ee898037d8923af072bd67c68f0e285)) - [@keyz182](https://github.com/keyz182)
- reset a pack profile to its pack - ([0e82203](https://github.com/keyz182/rimworld-profile-manager/commit/0e822039c2c72adc302d6cfa12bd742fdacf0ced)) - [@keyz182](https://github.com/keyz182)
- profiles keep the pack they were made from (schema 3) - ([925aa25](https://github.com/keyz182/rimworld-profile-manager/commit/925aa25915007e6290a32f1e832dae83224208b2)) - [@keyz182](https://github.com/keyz182)
- optional cover image in .rimpack (schema 2) - ([1bd1f62](https://github.com/keyz182/rimworld-profile-manager/commit/1bd1f623e14f6dcf1087e53bd0fd21802e262350)) - [@keyz182](https://github.com/keyz182)
#### Documentation
- describe pack covers, markdown descriptions, reset and the offline prompt - ([1cf37c8](https://github.com/keyz182/rimworld-profile-manager/commit/1cf37c83b734953379f5741b2dcda00bf9776040)) - [@keyz182](https://github.com/keyz182)
#### Miscellaneous Chores
- remove SPEC.md and the references to it - ([b6dc8dd](https://github.com/keyz182/rimworld-profile-manager/commit/b6dc8dd0b6591c19d52fe141617cd7004a059f73)) - [@keyz182](https://github.com/keyz182)
#### Style
- format with rustfmt - ([b4249be](https://github.com/keyz182/rimworld-profile-manager/commit/b4249be1a53b5eaf1bbec10f9b2fa265e7f854f8)) - [@keyz182](https://github.com/keyz182)

- - -

## [v1.12.1](https://github.com/keyz182/rimworld-profile-manager/compare/ca18855d102c6b7eb44da31dccb174eb9bae1dea..v1.12.1) - 2026-10-20
#### Bug Fixes
- (**gui**) let a long notice scroll and be dismissed - ([e8d2aa9](https://github.com/keyz182/rimworld-profile-manager/commit/e8d2aa9cfa00deb634c313dd5796c26945545b59)) - [@keyz182](https://github.com/keyz182)
- (**launcher**) copy the Steam helper beside the synced launcher - ([ca18855](https://github.com/keyz182/rimworld-profile-manager/commit/ca18855d102c6b7eb44da31dccb174eb9bae1dea)) - [@keyz182](https://github.com/keyz182)
- (**steam**) do not time out a mod that waits in Steam's download queue - ([6997228](https://github.com/keyz182/rimworld-profile-manager/commit/6997228be0e27b3c2c2f15b01226f34a2385d457)) - [@keyz182](https://github.com/keyz182)
#### Miscellaneous Chores
- merge origin/main (v1.12.0 release commit) into main - ([4c5127e](https://github.com/keyz182/rimworld-profile-manager/commit/4c5127efb523845f451fd6580f8f0c92eb9d7c43)) - [@keyz182](https://github.com/keyz182)
- merge fix/long-notice into main - ([29d4347](https://github.com/keyz182/rimworld-profile-manager/commit/29d4347fc60d49996337aa4277625ff885a7159b)) - [@keyz182](https://github.com/keyz182)

- - -

## [v1.12.0](https://github.com/keyz182/rimworld-profile-manager/compare/b6f835a9d8062f88ae9fc017274d1970825f95f9..v1.12.0) - 2026-10-20
#### Features
- restore modpacks with Steam or steamcmd, and slug RimSort instance names (#29) - ([c8d8c06](https://github.com/keyz182/rimworld-profile-manager/commit/c8d8c06b6874e14f0f8329642506917e402fb0c6)) - Kieran Evans
#### Tests
- (**gui**) render the Game versions screens for the wiki (#27) - ([27361e5](https://github.com/keyz182/rimworld-profile-manager/commit/27361e5a5df4e973d526f0ccfaadcc924b20824b)) - Kieran Evans
- (**gui**) render the backup dialogs for the wiki (#26) - ([b6f835a](https://github.com/keyz182/rimworld-profile-manager/commit/b6f835a9d8062f88ae9fc017274d1970825f95f9)) - Kieran Evans
#### Continuous Integration
- bump the actions group with 2 updates (#28) - ([5927012](https://github.com/keyz182/rimworld-profile-manager/commit/5927012cbe2c74edd96e9e24071b93e88a6563b7)) - dependabot[bot], dependabot[bot]

- - -

## [v1.11.0](https://github.com/keyz182/rimworld-profile-manager/compare/5c07f9fbb58bc8cdfd095b11133671fbd157879c..v1.11.0) - 2026-10-16
#### Features
- back up and restore OFFLINE profiles as .rimrim files (#24) - ([5c07f9f](https://github.com/keyz182/rimworld-profile-manager/commit/5c07f9fbb58bc8cdfd095b11133671fbd157879c)) - Kieran Evans

- - -

## [v1.10.0](https://github.com/keyz182/rimworld-profile-manager/compare/fa8718db64598ca7ead202c93e61842b88d1e624..v1.10.0) - 2026-10-13
#### Features
- (**gui**) explain game versions in the docs and Help, and scroll the Help sections (#21) - ([fa8718d](https://github.com/keyz182/rimworld-profile-manager/commit/fa8718db64598ca7ead202c93e61842b88d1e624)) - Kieran Evans

- - -

## [v1.9.0](https://github.com/keyz182/rimworld-profile-manager/compare/16b29c5c0eb250a0ecab323712090b2ba49f4a97..v1.9.0) - 2026-10-13
#### Features
- (**gui**) game version row and picker on the profile (#20) - ([16b29c5](https://github.com/keyz182/rimworld-profile-manager/commit/16b29c5c0eb250a0ecab323712090b2ba49f4a97)) - Kieran Evans

- - -

## [v1.8.0](https://github.com/keyz182/rimworld-profile-manager/compare/72d32de5ce12683e95ecdd53f763b7938b11a7bc..v1.8.0) - 2026-10-13
#### Features
- (**gui**) game versions screen with capture and delete (#19) - ([72d32de](https://github.com/keyz182/rimworld-profile-manager/commit/72d32de5ce12683e95ecdd53f763b7938b11a7bc)) - Kieran Evans

- - -

## [v1.7.0](https://github.com/keyz182/rimworld-profile-manager/compare/99aa87e83c179840fd06a52ec7ef1131b40c3c84..v1.7.0) - 2026-10-13
#### Features
- (**gui**) pin under the run lock, play a pinned build without Steam (#18) - ([99aa87e](https://github.com/keyz182/rimworld-profile-manager/commit/99aa87e83c179840fd06a52ec7ef1131b40c3c84)) - Kieran Evans

- - -

## [v1.6.0](https://github.com/keyz182/rimworld-profile-manager/compare/4cfb33c73cfbb690b6e575faa8afd160f9ddd1ad..v1.6.0) - 2026-10-13
#### Features
- (**run**) T55 launch the pinned game build (#17) - ([4cfb33c](https://github.com/keyz182/rimworld-profile-manager/commit/4cfb33c73cfbb690b6e575faa8afd160f9ddd1ad)) - Kieran Evans

- - -

## [v1.5.1](https://github.com/keyz182/rimworld-profile-manager/compare/20f0fcded4b1dcfc362270a99d7eb80b28834430..v1.5.1) - 2026-10-13
#### Bug Fixes
- (**gui**) T57 pinned RimSort export close-out, same missing-build text (#16) - ([20f0fcd](https://github.com/keyz182/rimworld-profile-manager/commit/20f0fcded4b1dcfc362270a99d7eb80b28834430)) - Kieran Evans

- - -

## [v1.5.0](https://github.com/keyz182/rimworld-profile-manager/compare/6b9ae6aa290a457830ec5bef24df0659cdda5cda..v1.5.0) - 2026-10-13
#### Features
- (**core**) T60 effective-game resolver (#15) - ([6b9ae6a](https://github.com/keyz182/rimworld-profile-manager/commit/6b9ae6aa290a457830ec5bef24df0659cdda5cda)) - Kieran Evans

- - -

## [v1.4.2](https://github.com/keyz182/rimworld-profile-manager/compare/4f8c02a4d734526a0b572772f0860c47958e67b9..v1.4.2) - 2026-10-13
#### Bug Fixes
- (**profile**) backprop §B.16 + §V.71: leave 1.7 logs out of a Default clone - ([a269cac](https://github.com/keyz182/rimworld-profile-manager/commit/a269cacf26cbe30910fb2ef73b818889647a9bbb)) - [@keyz182](https://github.com/keyz182)
#### Documentation
- (**spec**) link parked items to issues #6 #7, drop ?parity_fallback - ([33402c2](https://github.com/keyz182/rimworld-profile-manager/commit/33402c28a754f9a593b3df07392283169a01161d)) - [@keyz182](https://github.com/keyz182)
- (**spec**) record macOS builds spike, close T52 (R47-R51, B16-B19) (#13) - ([f54c98b](https://github.com/keyz182/rimworld-profile-manager/commit/f54c98b2dc222db12d7f06a181216ec4f4be4938)) - Kieran Evans

- - -

## [v1.4.1](https://github.com/keyz182/rimworld-profile-manager/compare/410ea72c74f11a18f790acd78ada5c10448d36d9..v1.4.1) - 2026-10-13
#### Bug Fixes
- (**core**) find RimWorld 1.7 on macOS, and reject the husk bundle (#12) - ([410ea72](https://github.com/keyz182/rimworld-profile-manager/commit/410ea72c74f11a18f790acd78ada5c10448d36d9)) - Kieran Evans

- - -

## [v1.4.0](https://github.com/keyz182/rimworld-profile-manager/compare/c6b50badbee302737c00e7fede7e187cfd62659d..v1.4.0) - 2026-10-13
#### Features
- (**core**) T53 game build store — capture, update, delete (#10) - ([c6b50ba](https://github.com/keyz182/rimworld-profile-manager/commit/c6b50badbee302737c00e7fede7e187cfd62659d)) - Kieran Evans

- - -

## [v1.3.0](https://github.com/keyz182/rimworld-profile-manager/compare/a214fe2af287146c95865a0265721f66032fc521..v1.3.0) - 2026-10-12
#### Features
- (**launch**) T65 launch RimWorld 1.7 (Waylith) profiles - ([e9a5d82](https://github.com/keyz182/rimworld-profile-manager/commit/e9a5d82e87575802da2f3d5818ac8c19a8a7bcc5)) - [@keyz182](https://github.com/keyz182)
- (**log**) T66 per-engine game log for RimWorld 1.7 - ([e839cfc](https://github.com/keyz182/rimworld-profile-manager/commit/e839cfce8364f8a954cd8476293bca227f094a75)) - [@keyz182](https://github.com/keyz182)
- (**payload**) T64 startup-hook payload for RimWorld 1.7 - ([b96ddac](https://github.com/keyz182/rimworld-profile-manager/commit/b96ddac2dd2bd99d2fa8b19f29f89eb41700c185)) - [@keyz182](https://github.com/keyz182)
#### Bug Fixes
- (**discovery**) backprop §B.13 + §V.69: find RimWorld 1.7 on Windows - ([204b0aa](https://github.com/keyz182/rimworld-profile-manager/commit/204b0aa16b2da54fa00733597d0a0d3f6acee350)) - [@keyz182](https://github.com/keyz182)
- (**launch**) backprop §B.14 + §V.70: no game-folder loader for 1.7 - ([2358acf](https://github.com/keyz182/rimworld-profile-manager/commit/2358acf47891e2440ba76396d7a8b88f02b8227f)) - [@keyz182](https://github.com/keyz182)
#### Documentation
- (**spec**) accept Windows 1.6 build overlay limit (issue #5) - ([cbe326b](https://github.com/keyz182/rimworld-profile-manager/commit/cbe326bd77c256702f0e2b152b3c3bca9184deaf)) - [@keyz182](https://github.com/keyz182)
- (**spec**) record Windows build spike, close T51 (R46) - ([1e998e9](https://github.com/keyz182/rimworld-profile-manager/commit/1e998e9401fd1125340381381d0e8e4084493d23)) - [@keyz182](https://github.com/keyz182)
- (**spec**) record Windows 1.7 spike, close T67 (R45) - ([43522e2](https://github.com/keyz182/rimworld-profile-manager/commit/43522e26e568042cb4d1e22d8215266ecbfc310f)) - [@keyz182](https://github.com/keyz182)
- (**spec**) record 1.7 smoke test (R43) - ([92c53ec](https://github.com/keyz182/rimworld-profile-manager/commit/92c53ec99882a160de64faa7395e574f4814b81b)) - [@keyz182](https://github.com/keyz182)
- (**spec**) live_log_17 → log-dir hook for LIVE 1.7 (R42) - ([10efcee](https://github.com/keyz182/rimworld-profile-manager/commit/10efcee02402f0b9e582309b2f69aec087e8fa04)) - [@keyz182](https://github.com/keyz182)
- (**spec**) decide ?live_log_17, capture LIVE 1.7 output - ([ad5ece5](https://github.com/keyz182/rimworld-profile-manager/commit/ad5ece549b234272bf1b91f3b7c0c75687b54506)) - [@keyz182](https://github.com/keyz182)
- (**spec**) close T61, superseded by T65 - ([1457738](https://github.com/keyz182/rimworld-profile-manager/commit/14577385b4471782ea141d34631831d17e4d8141)) - [@keyz182](https://github.com/keyz182)
- (**spec**) backprop §B.10-11 + §V.67-68: 1.7 startup hooks spike - ([37ca71b](https://github.com/keyz182/rimworld-profile-manager/commit/37ca71bc8913d401f89295e26cec9f63534e4ed3)) - [@keyz182](https://github.com/keyz182)
- (**spec**) plan v1.3 RimWorld 1.7 support - ([7dee8a6](https://github.com/keyz182/rimworld-profile-manager/commit/7dee8a6b090bb714978ce49e2e376981eb13cd43)) - [@keyz182](https://github.com/keyz182)
- (**spec**) backprop §B.7-9 + §V.62-63: 1.7 engine change, Version.txt lag - ([bddaf9f](https://github.com/keyz182/rimworld-profile-manager/commit/bddaf9f9d54edf27e03ebf5b348fdfc653d4e968)) - [@keyz182](https://github.com/keyz182)
- (**spec**) plan v1.2 game builds - ([0286f91](https://github.com/keyz182/rimworld-profile-manager/commit/0286f91a7979c3ec89509a1f5a2e723688a3e77a)) - [@keyz182](https://github.com/keyz182)
- T69 explain RimWorld 1.7 support - ([f5277f0](https://github.com/keyz182/rimworld-profile-manager/commit/f5277f0fe6443ad030f643fd87dde8f0167f0171)) - [@keyz182](https://github.com/keyz182)
- Update README.md - ([a214fe2](https://github.com/keyz182/rimworld-profile-manager/commit/a214fe2af287146c95865a0265721f66032fc521)) - Kieran Evans
#### Tests
- (**run**) T70 hold the run lock through a 1.7 Restart - ([1e28075](https://github.com/keyz182/rimworld-profile-manager/commit/1e28075054fc318ff62144d5be99e78c5a88c0a8)) - [@keyz182](https://github.com/keyz182)

- - -

## [v1.2.0](https://github.com/keyz182/rimworld-profile-manager/compare/b7809bc84947d90902809d52c8d7f8f83ce45222..v1.2.0) - 2026-10-12
#### Features
- (**macos**) macOS support (T37-T49) (#2) - ([b7809bc](https://github.com/keyz182/rimworld-profile-manager/commit/b7809bc84947d90902809d52c8d7f8f83ce45222)) - Kieran Evans
#### Bug Fixes
- (**release**) don't depend on macOS 26 tools when packaging (#4) - ([03efbdf](https://github.com/keyz182/rimworld-profile-manager/commit/03efbdf6314da79eb6698e4eec7e1bada4b34e09)) - Kieran Evans
- (**release**) set the bundle version portably on macOS runners (#3) - ([b32b1b6](https://github.com/keyz182/rimworld-profile-manager/commit/b32b1b6b2844ee0208fd26e0836c191742843396)) - Kieran Evans

- - -

## [v1.1.1](https://github.com/keyz182/rimworld-profile-manager/compare/eed7fcc300d52f81eac9a5ff9f4f202fedd1bc49..v1.1.1) - 2026-10-12
#### Bug Fixes
- (**gui**) show why in the Steam guide when the launcher copy failed - ([cc1e188](https://github.com/keyz182/rimworld-profile-manager/commit/cc1e188e57b87b768780c55d836a527a75be7c7a)) - [@keyz182](https://github.com/keyz182)
- (**release**) build and upload only this version's bundles - ([ac9d221](https://github.com/keyz182/rimworld-profile-manager/commit/ac9d221e61a8817e5d3a715b1b16644b9b36ebfc)) - [@keyz182](https://github.com/keyz182)
#### Documentation
- (**spec**) record the live AppImage launch through Steam - ([3d59809](https://github.com/keyz182/rimworld-profile-manager/commit/3d59809486eb1e49a8eea924a527def3ac8123b3)) - [@keyz182](https://github.com/keyz182)
- add a README with the logo - ([eed7fcc](https://github.com/keyz182/rimworld-profile-manager/commit/eed7fcc300d52f81eac9a5ff9f4f202fedd1bc49)) - [@keyz182](https://github.com/keyz182)
#### Tests
- run the moved-root launch inside the serial run tests - ([61feeb9](https://github.com/keyz182/rimworld-profile-manager/commit/61feeb94484932cc12b93be581ea929f323fd11f)) - [@keyz182](https://github.com/keyz182)
#### Continuous Integration
- bump cocogitto/cocogitto-action (#1) - ([df681d3](https://github.com/keyz182/rimworld-profile-manager/commit/df681d39d94d0744f8d8e3737d89d488e8c8bb8f)) - dependabot[bot], dependabot[bot]
#### Miscellaneous Chores
- (**icon**) lighter cloud in the app icon - ([19b1a58](https://github.com/keyz182/rimworld-profile-manager/commit/19b1a580bd5068930584bd0842670820a6917f5b)) - [@keyz182](https://github.com/keyz182)
- (**icon**) leave timestamps out of the generated PNGs - ([401c8bc](https://github.com/keyz182/rimworld-profile-manager/commit/401c8bc6550c90910002750ccd0f3776c5c0348c)) - [@keyz182](https://github.com/keyz182)
- (**icon**) new app icon artwork - ([d0bcf68](https://github.com/keyz182/rimworld-profile-manager/commit/d0bcf68fef9f33cca816c56e680c351aced113bb)) - [@keyz182](https://github.com/keyz182)

- - -

## [v1.1.0](https://github.com/keyz182/rimworld-profile-manager/compare/2b81f6af5a1e81c093f7f41a7759c1ade5186b95..v1.1.0) - 2026-10-12
#### Features
- (**linux**) ship an AppImage - ([c3fb0f2](https://github.com/keyz182/rimworld-profile-manager/commit/c3fb0f293a0a14184e427ea4f6ed39a0de2222ed)) - [@keyz182](https://github.com/keyz182)
- (**windows**) add an installer - ([1e0eefc](https://github.com/keyz182/rimworld-profile-manager/commit/1e0eefce0053f293ebbfbbf3160c259e38786892)) - [@keyz182](https://github.com/keyz182)
#### Bug Fixes
- (**release**) bump on a local main branch and stop hiding cog errors - ([e8c41a7](https://github.com/keyz182/rimworld-profile-manager/commit/e8c41a7456965edebf9f889c153943fed5152cfd)) - [@keyz182](https://github.com/keyz182)
- (**windows**) name the real menu item in the uninstall note - ([528311f](https://github.com/keyz182/rimworld-profile-manager/commit/528311ff55b9c4c4696210573de6871d862d7a4e)) - [@keyz182](https://github.com/keyz182)
#### Documentation
- add a script that renders the wiki screenshots - ([038742e](https://github.com/keyz182/rimworld-profile-manager/commit/038742e08e0b64e90f33a0e6c906b350333f0278)) - [@keyz182](https://github.com/keyz182)
#### Continuous Integration
- (**release**) build the two bundles in parallel with caches - ([1b471e7](https://github.com/keyz182/rimworld-profile-manager/commit/1b471e7735fd45b3d582788ba49a571236774d5a)) - [@keyz182](https://github.com/keyz182)
- (**release**) run bundles and publish when CI was skipped - ([2b81f6a](https://github.com/keyz182/rimworld-profile-manager/commit/2b81f6af5a1e81c093f7f41a7759c1ade5186b95)) - [@keyz182](https://github.com/keyz182)
#### Miscellaneous Chores
- drop the launch spikes, keep the probe mod for e2e - ([130a2e3](https://github.com/keyz182/rimworld-profile-manager/commit/130a2e3240f106eeb74a73729cab1babe5ba19ca)) - [@keyz182](https://github.com/keyz182)

- - -

## [v1.0.0](https://github.com/keyz182/rimworld-profile-manager/releases/tag/v1.0.0) - 2026-10-12
#### Features
- initial release - [@keyz182](https://github.com/keyz182)

- - -
