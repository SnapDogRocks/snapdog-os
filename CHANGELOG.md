# Changelog

## [0.9.4](https://github.com/SnapDogRocks/snapdog-os/compare/v0.9.3...v0.9.4) (2026-07-15)


### Bug Fixes

* **deps:** bump snapdog to 0.25.2 ([#96](https://github.com/SnapDogRocks/snapdog-os/issues/96)) ([9fd14cf](https://github.com/SnapDogRocks/snapdog-os/commit/9fd14cf579168c3b42a197aafd5ece8719108454))

## [0.9.3](https://github.com/SnapDogRocks/snapdog-os/compare/v0.9.2...v0.9.3) (2026-07-12)


### Bug Fixes

* **webui:** show the reconnect overlay immediately on any reboot ([#91](https://github.com/SnapDogRocks/snapdog-os/issues/91)) ([57eeed9](https://github.com/SnapDogRocks/snapdog-os/commit/57eeed91fdef681b2917ad6bfdac7ee3b8f12fad))

## [0.9.2](https://github.com/SnapDogRocks/snapdog-os/compare/v0.9.1...v0.9.2) (2026-07-12)


### Bug Fixes

* **ci:** pass --repo to gh pr merge in release workflow ([#88](https://github.com/SnapDogRocks/snapdog-os/issues/88)) ([3384166](https://github.com/SnapDogRocks/snapdog-os/commit/3384166431375b0f19b770b79fb6c78c57bff219))

## [0.9.1](https://github.com/SnapDogRocks/snapdog-os/compare/v0.9.0...v0.9.1) (2026-07-11)


### Bug Fixes

* **ctrl:** reliable local-time auto-update scheduler + NTP hardening + save UX ([#86](https://github.com/SnapDogRocks/snapdog-os/issues/86)) ([a6f1b18](https://github.com/SnapDogRocks/snapdog-os/commit/a6f1b18ac9aeddc04f960485247e03da84f00dcc))

## [0.9.0](https://github.com/SnapDogRocks/snapdog-os/compare/v0.8.1...v0.9.0) (2026-07-11)


### Features

* **webui:** complete i18n + enforce full translation (build/commit/CI gates) ([#82](https://github.com/SnapDogRocks/snapdog-os/issues/82)) ([86283a8](https://github.com/SnapDogRocks/snapdog-os/commit/86283a870ec5e9609fefb2cdef2bca91fa0c8f02))

## [0.8.1](https://github.com/SnapDogRocks/snapdog-os/compare/v0.8.0...v0.8.1) (2026-07-09)


### Bug Fixes

* **ctrl:** use tryboot-aware reboot in the auto-update loop ([#75](https://github.com/SnapDogRocks/snapdog-os/issues/75)) ([e376ec0](https://github.com/SnapDogRocks/snapdog-os/commit/e376ec06aed9c6b6b089f0ec4c3a66d6f2ab4b9e))

## [0.8.0](https://github.com/SnapDogRocks/snapdog-os/compare/v0.7.3...v0.8.0) (2026-07-09)


### Features

* **webui:** confirm Wi-Fi disconnect when it is the device's only link ([#73](https://github.com/SnapDogRocks/snapdog-os/issues/73)) ([d6d9380](https://github.com/SnapDogRocks/snapdog-os/commit/d6d9380b78546cb6c0e128d870e607fa0814107c))


### Bug Fixes

* **webui:** stale WiFi-disconnect state and client-config defaults flash ([#71](https://github.com/SnapDogRocks/snapdog-os/issues/71)) ([fe69844](https://github.com/SnapDogRocks/snapdog-os/commit/fe6984457f39da3ddcab5ccd4c6c9ca3a10d0a34))

## [0.7.3](https://github.com/SnapDogRocks/snapdog-os/compare/v0.7.2...v0.7.3) (2026-07-08)


### Bug Fixes

* **snapdog-update:** detect reboot via booted-slot bundle version ([#67](https://github.com/SnapDogRocks/snapdog-os/issues/67)) ([8b6db71](https://github.com/SnapDogRocks/snapdog-os/commit/8b6db71f4620852a22f2853e332809a4360f9f8f))

## [0.7.2](https://github.com/SnapDogRocks/snapdog-os/compare/v0.7.1...v0.7.2) (2026-07-08)


### Bug Fixes

* **snapdog-ctrl:** address PR review comments, split template service into dedicated rw/ro services, and propagate errors ([7d01964](https://github.com/SnapDogRocks/snapdog-os/commit/7d0196478b97041f618cb7710404acb24492b876))
* **snapdog-ctrl:** drop CAP_SYS_ADMIN by using systemd remount helper ([940a4ff](https://github.com/SnapDogRocks/snapdog-os/commit/940a4ffa7654b235bfecd95b5c011726e5690d6b), [fbb4071](https://github.com/SnapDogRocks/snapdog-os/commit/fbb407198c53a8e3d4286e3e04034625d7e3c2d1))

## [0.7.1](https://github.com/SnapDogRocks/snapdog-os/compare/v0.7.0...v0.7.1) (2026-07-08)


### Bug Fixes

* **os:** set RAUC max-bundle-download-size so URL OTA installs work ([b0c24c4](https://github.com/SnapDogRocks/snapdog-os/commit/b0c24c44898f021a75666fbc99f6e279b48810ff))
* **os:** set RAUC max-bundle-download-size so URL OTA installs work ([ef3b5f0](https://github.com/SnapDogRocks/snapdog-os/commit/ef3b5f0811f4b95bb9749d0819229906f4c2ffa8))

## [0.7.0](https://github.com/SnapDogRocks/snapdog-os/compare/v0.6.1...v0.7.0) (2026-07-08)


### Features

* **buildroot:** dynamically default update channel based on version type ([23a3269](https://github.com/SnapDogRocks/snapdog-os/commit/23a3269996cbe9e2620ee1facc025a3e5b409f8d))
* device-setup, OTA-update & hardware-detection overhaul ([#58](https://github.com/SnapDogRocks/snapdog-os/issues/58)) ([223acd7](https://github.com/SnapDogRocks/snapdog-os/commit/223acd768440e1da6504d7fbc212d91087259e4f))
* **rauc:** add RPi tryboot A/B rollback for OTA ([18592d7](https://github.com/SnapDogRocks/snapdog-os/commit/18592d7776cfeb01bae2b2a50e432cf761a33ad8))
* **rauc:** tryboot A/B rollback and OTA upgrade hardening ([b4122bf](https://github.com/SnapDogRocks/snapdog-os/commit/b4122bff03624e24f5a5379b78a2c78b3e2c4a9e))
* **webui:** gate hardware tuning behind an apply + reboot-choice requester ([5dd370f](https://github.com/SnapDogRocks/snapdog-os/commit/5dd370f4425aca2d21b9b0b9cac5975113a6793c))
* **webui:** hardware tuning — apply + reboot-choice requester ([aa5a0f3](https://github.com/SnapDogRocks/snapdog-os/commit/aa5a0f39703b2a41010b23cc5f6d3fe6c37debaa))


### Bug Fixes

* **buildroot:** build RAUC with its D-Bus service (BR2_PACKAGE_RAUC_DBUS) ([dd22ee6](https://github.com/SnapDogRocks/snapdog-os/commit/dd22ee6f15bf36f702e6988884dc74d6b3a39bf4))
* **buildroot:** enable runtime SSH and WiFi client on the read-only rootfs ([cdddd32](https://github.com/SnapDogRocks/snapdog-os/commit/cdddd3263098a37b57a7ca3c15d05733adc5c64e))
* **buildroot:** fix first-boot /data resize race that left /data read-only ([b296f9e](https://github.com/SnapDogRocks/snapdog-os/commit/b296f9e263691fb8bdfefe733e90ae8753ae8583))
* **buildroot:** make first-boot /data resize robust (never leaves it read-only) ([23b682d](https://github.com/SnapDogRocks/snapdog-os/commit/23b682d6b250cf466f590b913339e98ae8099183))
* **buildroot:** make post-build.sh idempotent for incremental rebuilds ([589ad2b](https://github.com/SnapDogRocks/snapdog-os/commit/589ad2b4a8651dd964128398d30a987d6267845c))
* **buildroot:** resolve various read-only rootfs and server startup quirks ([a61a678](https://github.com/SnapDogRocks/snapdog-os/commit/a61a6788dca48aab52d3ac8dec9478238b64618f))
* **ctrl:** drive update progress from real RAUC status, surface errors ([78d1adb](https://github.com/SnapDogRocks/snapdog-os/commit/78d1adb9cec943d75bfa0c61d1b4c0b5472f1171))
* **ctrl:** read timezone from the localtime symlink chain (not timedatectl) ([#61](https://github.com/SnapDogRocks/snapdog-os/issues/61)) ([b343034](https://github.com/SnapDogRocks/snapdog-os/commit/b3430342db61deda11b0c62b7c372ea309233edc))
* **ctrl:** report real update signature status + in-app install confirm ([33e545b](https://github.com/SnapDogRocks/snapdog-os/commit/33e545b2668e24f4ea2f8b0a3fb0ec7c50c3885d))
* **ctrl:** SSH toggle, WiFi boot, DAC-detect SSOT and soundcard dropdown ([8a621fc](https://github.com/SnapDogRocks/snapdog-os/commit/8a621fcd615100b7d08fe664b08c42ca7956f9d3))
* **ctrl:** stop auto-update reinstall loop with version gate + failed-bundle tracking ([fbd6b87](https://github.com/SnapDogRocks/snapdog-os/commit/fbd6b87b508586dcea5a6b72d956f97b716a12f5))
* **ota:** make snapdog-update OTA work end-to-end (four device-side bugs) ([64a52a5](https://github.com/SnapDogRocks/snapdog-os/commit/64a52a586e48ea5a9c35df880b16539b5f7885b1))
* **rauc:** pin board-specific compatible in local builds ([d95ec57](https://github.com/SnapDogRocks/snapdog-os/commit/d95ec5732b4ba41c27383eefe3762c789ab78aa3))
* **release:** enable auto-merge on release PRs (read .number, not .html_url) ([2e228e3](https://github.com/SnapDogRocks/snapdog-os/commit/2e228e34890cfdac636a4c57c0b97f2631f6e5dc))
* **snapdog-update:** decode install progress as percentage ([4300ca6](https://github.com/SnapDogRocks/snapdog-os/commit/4300ca655177a05d404b0437c5f68f61983639ad))
* **snapdog-update:** require 'installing' before treating idle as install-complete ([7632fd1](https://github.com/SnapDogRocks/snapdog-os/commit/7632fd1398a6e23001d09c89a35e0b25911484d1))
* **snapdog-update:** surface the real cause + a connectivity hint on transport errors ([0d29542](https://github.com/SnapDogRocks/snapdog-os/commit/0d295420859cb1ef69094fcff6192f7fd3d18bcb))
* **snapdog-update:** survive transient status polls and drive the reboot ([c3dbfff](https://github.com/SnapDogRocks/snapdog-os/commit/c3dbfffb7826643ed2e3af8bbf38fb1d1e2953d6))
* SSH, WiFi, DAC auto-detect and soundcard picker on the read-only rootfs ([66c7ee8](https://github.com/SnapDogRocks/snapdog-os/commit/66c7ee83797e55dec4da47c2c6332d30e9faa47f))
* **webui:** address PR review on hardware-tuning draft state ([2643b19](https://github.com/SnapDogRocks/snapdog-os/commit/2643b1961e4e8ca6a4be99960d4f109c50da9c4f))

## [0.6.1](https://github.com/SnapDogRocks/snapdog-os/compare/v0.6.0...v0.6.1) (2026-07-03)


### Bug Fixes

* **build:** ensure newline when applying config overrides to prevent symbol corruption ([727ba38](https://github.com/SnapDogRocks/snapdog-os/commit/727ba381d32f7d1909e522cc8c9ee8f3a772cfef))
* **buildroot:** correct malformed snapdog-client.hash for 0.21.1 ([9929806](https://github.com/SnapDogRocks/snapdog-os/commit/9929806fe3ad17e68ddcf90824a73a3af569b81b))
* **buildroot:** update snapdog-client archive hash to v0.20.0 to prevent build failure ([bb62698](https://github.com/SnapDogRocks/snapdog-os/commit/bb62698c46c38020d1bcee408e7fefb69855117e))
* **buildroot:** use 64-bit defconfig for raspberry pi zero 2 w ([044d5a1](https://github.com/SnapDogRocks/snapdog-os/commit/044d5a1a225f80a164e49eadb17653a97c40d478))

## [0.6.0](https://github.com/SnapDogRocks/snapdog-os/compare/v0.5.0...v0.6.0) (2026-06-21)


### Features

* **network:** in-process captive DNS + DHCP option 114, drop dnsmasq ([1438904](https://github.com/SnapDogRocks/snapdog-os/commit/143890477991a21f50d1f5219953df295e991fde))
* **release:** single-source version + beta/release channels, no nightly drift ([a2c7aa0](https://github.com/SnapDogRocks/snapdog-os/commit/a2c7aa0ad864605469181475bc6b2636332d0697))


### Bug Fixes

* **buildroot:** delete brcmfmac hash file instead of blanking it ([fe50d12](https://github.com/SnapDogRocks/snapdog-os/commit/fe50d1228fa49a1a522266bd32c25e5c2708298a))
* **network:** ConfigureWithoutCarrier on AP so networkd assigns IP before hostapd ([32a9143](https://github.com/SnapDogRocks/snapdog-os/commit/32a91431056e84a08fcc17f0fe6b0b70cb1a2f6b))
* **network:** seed default ethernet DHCP on first boot ([910d315](https://github.com/SnapDogRocks/snapdog-os/commit/910d31523ad7b6c76ea987818b24b9296a6fa368))
* **server_config:** drop redundant deref in KNX test asserts ([f94b486](https://github.com/SnapDogRocks/snapdog-os/commit/f94b486055cd8448af21d07ca406afb8e7d588db))
* **webui:** bump hono to 4.12.26 and js-yaml to 4.2.0 ([0fbc71e](https://github.com/SnapDogRocks/snapdog-os/commit/0fbc71ee133488661f8a0eb1216e508980481f0f))

## [0.5.0](https://github.com/SnapDogRocks/snapdog-os/compare/v0.4.1...v0.5.0) (2026-06-14)


### Features

* **ctrl:** expand KNX zone group addresses in config generator ([65b23a3](https://github.com/SnapDogRocks/snapdog-os/commit/65b23a3a81cece4743e0408d14879be5989bf985))
* **webui:** expand settings UI with KNX addresses and styling improvements ([ac25040](https://github.com/SnapDogRocks/snapdog-os/commit/ac25040442edfa96652c53ddf313779d8d3a82cb))


### Bug Fixes

* **ci:** pin upload/download-artifact to v7/v8 in snapdog-update jobs ([1ff0853](https://github.com/SnapDogRocks/snapdog-os/commit/1ff085326929db041ce7ef7c0c5c05f1bf295cb5))

## [0.4.1](https://github.com/SnapDogRocks/snapdog-os/compare/v0.4.0...v0.4.1) (2026-06-07)


### Bug Fixes

* **ci:** add zero2w defconfig mapping + clear firmware hash file ([826ed50](https://github.com/SnapDogRocks/snapdog-os/commit/826ed50d684084a2135dc964b70335de32e44952))
* **ci:** use windows-latest + windows-11-arm for snapdog-update builds ([59fc91e](https://github.com/SnapDogRocks/snapdog-os/commit/59fc91ee4613e7075735bb6b7339940dd6948635))

## [0.4.0](https://github.com/SnapDogRocks/snapdog-os/compare/v0.3.0...v0.4.0) (2026-06-07)


### Features

* add Raspberry Pi Zero 2 W support ([a9fa8d5](https://github.com/SnapDogRocks/snapdog-os/commit/a9fa8d5214e04ab1bebb77945ebd67e6766dcd6a))
* **build:** completely rename SNAPDOG_PI_VERSION to SNAPDOG_BOARD in RAUC config and workflows ([49c9dbc](https://github.com/SnapDogRocks/snapdog-os/commit/49c9dbc9aac9b11cccd04ce255a52092a0ed88cf))
* **build:** refactor build configs and script paths to be board-agnostic ([3c5bef9](https://github.com/SnapDogRocks/snapdog-os/commit/3c5bef9bd901ae4db7f903497d2a6c7c303180db))
* **ctrl:** Now Playing mini-player with MPRIS2 D-Bus integration ([c2757f5](https://github.com/SnapDogRocks/snapdog-os/commit/c2757f5f3bd73b7d77f68572576fb4815cae7777))
* **kernel:** append real-time scheduler optimizations and HZ settings to kernel fragment ([34fe818](https://github.com/SnapDogRocks/snapdog-os/commit/34fe818db97f6f44ae9676703405c2996fb0e4d8))
* **settings:** export/import device settings as tar.gz ([f031922](https://github.com/SnapDogRocks/snapdog-os/commit/f03192269e910e23167592f6a1b2e9cada1ba46a))
* **softap:** derive unique SSID from WiFi MAC address ([f4163e1](https://github.com/SnapDogRocks/snapdog-os/commit/f4163e190e83df7fdb40bc34e00d3b8b7b8384ec))
* **tuning:** add accessible hover-and-tap tooltips for tuning options ([d800c2a](https://github.com/SnapDogRocks/snapdog-os/commit/d800c2aa00682da1b98740bbbe4728c3d2abb5f4))
* **tuning:** implement device-agnostic hardware tuning HAL ([a5dc600](https://github.com/SnapDogRocks/snapdog-os/commit/a5dc6009913843a8947514cf3a4c8e3fa541c621))
* **tuning:** write full RT scheduling overrides in systemd drop-in ([9e4333f](https://github.com/SnapDogRocks/snapdog-os/commit/9e4333ff5099d6ca3c692b49a5dae53c69135db1))
* **update:** add build script for compile-time git versioning ([460fd2b](https://github.com/SnapDogRocks/snapdog-os/commit/460fd2b6f2494bc133957e0ad774b1168cb41df7))
* **update:** harden update CLI for operators ([78a0c1e](https://github.com/SnapDogRocks/snapdog-os/commit/78a0c1e07c90208237a77b7e6cbb0dc891a7ffaa))
* **update:** implement developer firmware upgrade tool (snapdog-update) ([0527b5e](https://github.com/SnapDogRocks/snapdog-os/commit/0527b5e205bab43f0d5f61779c327731ba6ce07b))


### Bug Fixes

* enable raspi-wifi package (hostapd/dnsmasq/wpa_supplicant missing from image) ([f463d11](https://github.com/SnapDogRocks/snapdog-os/commit/f463d11653eb26dfcab6558fc38593864bc588d1))
* **network:** kernel panic in brcmfmac P2P during AP start ([953dbc8](https://github.com/SnapDogRocks/snapdog-os/commit/953dbc8a925b4caee6c198dfdc4e9da686616fb6))
* **network:** validate setup SSID derivation ([25141aa](https://github.com/SnapDogRocks/snapdog-os/commit/25141aa8256230d36db837a4701b46a10fc367a4))
* replace all metaneutrons references with SnapDogRocks org ([431bf49](https://github.com/SnapDogRocks/snapdog-os/commit/431bf49df7aa5435fd5286572501536f556bfc2b))
* resolve clippy warnings for closures and map_or in config_txt.rs ([7a10f25](https://github.com/SnapDogRocks/snapdog-os/commit/7a10f2533fb720690ba8f6d6f41ff4498c7b622f))
* resolve mpris_client compilation and clippy errors in release builds ([aa5a5d2](https://github.com/SnapDogRocks/snapdog-os/commit/aa5a5d2884a33012830828275ec37969509385ae))
* **tuning:** make config.txt parsing robust to spacing, inline comments, and arguments ([703ccf0](https://github.com/SnapDogRocks/snapdog-os/commit/703ccf0869945ed4705d3c17ad5ceb1909373ebb))
* **tuning:** resolve clippy compiler and lint warnings ([b95844f](https://github.com/SnapDogRocks/snapdog-os/commit/b95844f9f1b7f9175e369ebcdbeafe0afa0a70b5))
* update snapdog client/server download URL to SnapDogRocks org ([207d9c7](https://github.com/SnapDogRocks/snapdog-os/commit/207d9c7cad7aaf49ced46c572958e344ab65b825))
* **update:** extend raw flash challenge ttl ([0ecd2e5](https://github.com/SnapDogRocks/snapdog-os/commit/0ecd2e554b49ae52f1b391d974fbe26fef7eb91e))
* **update:** harden reboot verification ([f99b6d7](https://github.com/SnapDogRocks/snapdog-os/commit/f99b6d765ab4411ae04563d983aacbeaa0064a5e))
* write OS version to /etc/snapdog-os.version during build ([bca0208](https://github.com/SnapDogRocks/snapdog-os/commit/bca0208087e2495d72cd05f358e4def07a783295))

## [0.3.0](https://github.com/SnapDogRocks/snapdog-os/compare/v0.2.0...v0.3.0) (2026-05-30)


### Features

* **ci:** add latest image redirect on R2 ([7fd441f](https://github.com/SnapDogRocks/snapdog-os/commit/7fd441f863f3e78cf96db21aca5fbb87793cdc4c))
* **ctrl:** output logs to HDMI framebuffer (tty1) for debug ([7ad78b2](https://github.com/SnapDogRocks/snapdog-os/commit/7ad78b2af1220cec3cdafd21358cb223599d15b9))
* **webui:** upgrade Next.js 15→16 (Turbopack) ([00a1a95](https://github.com/SnapDogRocks/snapdog-os/commit/00a1a9558fbde9f07a4eef4dfe0828d89535ed32))


### Bug Fixes

* **ci:** add x86_64 native optional deps for Next.js Turbopack ([632e219](https://github.com/SnapDogRocks/snapdog-os/commit/632e219b53656811b63eb83a9a874ceaf681ee00))
* **ci:** downgrade Next.js 16→15 (removes Turbopack native dep requirement) ([51c3cd6](https://github.com/SnapDogRocks/snapdog-os/commit/51c3cd6ce2439593a569145d2b436aea4bae7d8e))
* **ci:** remove apt-get from Publish step (jq/openssl pre-installed on GitHub runners) ([6c3dad3](https://github.com/SnapDogRocks/snapdog-os/commit/6c3dad3d18153a01afdd93f58946bfe7f295e1c2))
* **ci:** remove sudo apt-get rauc from Package step (runner already has it) ([f8f3a1b](https://github.com/SnapDogRocks/snapdog-os/commit/f8f3a1b319356a714af50cd442901c2abde47087))
* **ci:** skip AWS CLI install if already present on runner ([081edd4](https://github.com/SnapDogRocks/snapdog-os/commit/081edd45caf62f8c78eb2f1bba07bcecc6392265))
* **ci:** use npm install instead of npm ci (resolves platform-specific native deps) ([393161a](https://github.com/SnapDogRocks/snapdog-os/commit/393161a8228156183e1a26acece038acd5d65f71))
* correct update URL (updates.snapdog.cc, not update.snapdog.cc) ([dc49338](https://github.com/SnapDogRocks/snapdog-os/commit/dc49338c224089ce9d0388018380db6a0774cf93))
* downgrade eslint to v9 (v10 incompatible with eslint-config-next) ([3d794f2](https://github.com/SnapDogRocks/snapdog-os/commit/3d794f2ba09bbfd2b56935cf7a1ea73e4a9c5e4f))
* **network:** add default DHCP .network files in snapdog-data-init ([0d080dd](https://github.com/SnapDogRocks/snapdog-os/commit/0d080ddc589a1d4c4769c9d256c51bbe1fd1dc9d))
* **network:** stop resolved before starting dnsmasq in AP mode ([3c06e08](https://github.com/SnapDogRocks/snapdog-os/commit/3c06e08efb9e6531d5250642c3e9090974e519b9))
* regenerate lockfile with Node 22.13.0 (matches CI) ([3ec4807](https://github.com/SnapDogRocks/snapdog-os/commit/3ec48075c9f37e739bc7b0857a895b7baa1eecdc))
* remove .wrangler cache, add to gitignore ([b94fc80](https://github.com/SnapDogRocks/snapdog-os/commit/b94fc801c8245204e35d23d48629a341a32018f4))
* remove core dump from repo, add to gitignore ([70ced5e](https://github.com/SnapDogRocks/snapdog-os/commit/70ced5e627cfa8c30efe87c5e9501d9b0e836063))
* remove core dump, add to gitignore ([5804820](https://github.com/SnapDogRocks/snapdog-os/commit/5804820e8d4fc9931eadecb08a625dd3fd520a45))

## [0.2.0](https://github.com/metaneutrons/snapdog-os/compare/v0.1.0...v0.2.0) (2026-05-29)


### Features

* **audio:** auto-detect DAC at startup + immediate reboot ([420121f](https://github.com/metaneutrons/snapdog-os/commit/420121f4aa24638ccff8d13865e8ba4052865411))
* **audio:** auto-detect DAC from HAT EEPROM ([4afa543](https://github.com/metaneutrons/snapdog-os/commit/4afa54353de4b27809f5ed99ca342860202bc001))
* **audio:** auto-detect DAC UX improvements ([24f63af](https://github.com/metaneutrons/snapdog-os/commit/24f63af7b6d4ec298a06b795488c1c564d400ec0))
* **audio:** default codec f32lz4 + 32-bit depth ([c731e3d](https://github.com/metaneutrons/snapdog-os/commit/c731e3d8959faf7b19f516d466cf72e306f4e8da))
* **auth:** optional password protection for web UI ([4fc0f6d](https://github.com/metaneutrons/snapdog-os/commit/4fc0f6dcc41bbbfefab0d0f630f5687daafb0e5d))
* **auth:** unified device password (WebUI + console) ([f541db4](https://github.com/metaneutrons/snapdog-os/commit/f541db49b21ac652a266e34c6673758fe996aeab))
* **buildroot:** base system packages ([af3c174](https://github.com/metaneutrons/snapdog-os/commit/af3c1747effa0ceeb87caa2ff9d07b7995cd9166))
* **buildroot:** external tree for Pi 3/4/5 ([aaed702](https://github.com/metaneutrons/snapdog-os/commit/aaed702a47ef0676906713cd31c5fcbebec6029c))
* **buildroot:** OTA updater with SHA256 and auto-rollback ([1e1ac07](https://github.com/metaneutrons/snapdog-os/commit/1e1ac072010845bb4dbd338744805174d21df146))
* **buildroot:** snapdog-client, snapdog-ctrl, and meta-package ([116d2b8](https://github.com/metaneutrons/snapdog-os/commit/116d2b88481048cb247368b8446d2c6c803db246))
* **ctrl:** derive version from git describe ([9147656](https://github.com/metaneutrons/snapdog-os/commit/9147656d976dc0eb9b5ce718e7ed8c2ec302c17a))
* **ctrl:** show real IP address in startup log ([64ed85e](https://github.com/metaneutrons/snapdog-os/commit/64ed85e112036abb96fa7f0c960ca53d93571752))
* enable framebuffer console + USB-C OTG serial console ([f94a7e1](https://github.com/metaneutrons/snapdog-os/commit/f94a7e1b0904f18d29f15cc4125a0bc6ea0467c8))
* full NVMe/device-agnostic support ([9f45a38](https://github.com/metaneutrons/snapdog-os/commit/9f45a38921d411b923fdf9a194f94b1a9523882a))
* **kernel:** add virtio built-in for QEMU testing ([2f0c902](https://github.com/metaneutrons/snapdog-os/commit/2f0c9022714bc29b9258c5ff500c39acf364c000))
* **mdns:** feature-gated mDNS backends (astro-dnssd default, mdns-sd alt) ([a0662c4](https://github.com/metaneutrons/snapdog-os/commit/a0662c4b9a41e4f5ebe19d08386c6a50e1bca141))
* preflight health check + warning banner in WebUI ([c6ac7df](https://github.com/metaneutrons/snapdog-os/commit/c6ac7df81d1799000cfe5d6d0cb8f5f3d63f045d))
* **rauc:** enterprise-grade OTA via RAUC ([ff030f1](https://github.com/metaneutrons/snapdog-os/commit/ff030f1c7a174ab7b44955985070a46fc5b304e1))
* **rauc:** Phase 1 — RAUC on target with custom RPi bootloader backend ([ddd0c99](https://github.com/metaneutrons/snapdog-os/commit/ddd0c99bfa71d5e9b524327e16404b88821bd900))
* **rauc:** Phase 2 — bundle generation in CI ([66a1595](https://github.com/metaneutrons/snapdog-os/commit/66a1595f28ec151104a96065cf4da3c7ba4100a2))
* **rauc:** Phase 3 — snapdog-ctrl D-Bus integration ([36143f5](https://github.com/metaneutrons/snapdog-os/commit/36143f5c05d4229112858544aa104d3058af5d9e))
* **rauc:** Phase 4 — remove snapdog-updater package ([772d602](https://github.com/metaneutrons/snapdog-os/commit/772d60242f2b4289bf326911926e8e3d6e1c20b5))
* reboot confirmation after manual update + raw flash escape hatch ([8d33f0f](https://github.com/metaneutrons/snapdog-os/commit/8d33f0f76ffbcb4ac482c3e87fd89a77b5269cc2))
* **security:** switch OTA signing to Ed25519 ([a892e8b](https://github.com/metaneutrons/snapdog-os/commit/a892e8b0d6599234a98026f8d8696006d060c0b3))
* **server:** add name, advertise_snapcast, airplay.mode, subsonic.format, client.icon, client.max_volume ([f020f7c](https://github.com/metaneutrons/snapdog-os/commit/f020f7c2005bd87037e28399a993f5ff796ca7e7))
* **server:** API keys management in WebUI ([1e57ad9](https://github.com/metaneutrons/snapdog-os/commit/1e57ad939f8359c5147b6500d841ed914262445a))
* **server:** backend — toml_edit config module + buildroot package + API ([108be99](https://github.com/metaneutrons/snapdog-os/commit/108be99e17987e01b4f54d94f616a97754720e55))
* show component versions in Dashboard ([f700a42](https://github.com/metaneutrons/snapdog-os/commit/f700a42ffcf36d0c286c4f95fd21d4e8faa47e60))
* snapdog-ctrl manages all optional services ([a22fd91](https://github.com/metaneutrons/snapdog-os/commit/a22fd918a1d195cb20e2c36db6f5d49d61cf8904))
* **snapdog-ctrl:** Rust device config service ([a033567](https://github.com/metaneutrons/snapdog-os/commit/a033567ed8cd23756bdfacc7058b7c6db3df7faa))
* **softap:** configurable enable + password via ctrl.toml ([857b4c7](https://github.com/metaneutrons/snapdog-os/commit/857b4c7482f93186952de61f502fa7212f1789b6))
* **ui:** device name, emoji picker, volume slider, airplay mode, subsonic format ([6dc0848](https://github.com/metaneutrons/snapdog-os/commit/6dc0848ce223a7e273dc745b82df3dff42c9a811))
* **update:** add interval setting (daily/weekly/monthly) ([2e07fb7](https://github.com/metaneutrons/snapdog-os/commit/2e07fb719df085729a52aba3ad2e4e690c3f1d8f))
* **update:** auto-update scheduler ([28780b5](https://github.com/metaneutrons/snapdog-os/commit/28780b5727d03a2a54e967042a75ec65e9c434cd))
* **webui:** Next.js 16 static UI with 7 tabs ([77ed96e](https://github.com/metaneutrons/snapdog-os/commit/77ed96eeccde36c32146687b6ea678447e2bb231))
* **webui:** Server tab with sub-tabs + Client enable/disable ([0b1b53b](https://github.com/metaneutrons/snapdog-os/commit/0b1b53b55d23b68dba2e4f35f0e763f70782ee82))


### Bug Fixes

* /var/lib as tmpfs + USB gadget built-in ([0040890](https://github.com/metaneutrons/snapdog-os/commit/0040890836823d0af3e2d100317bdc6ac7b85268))
* add BR2_PACKAGE_AVAHI_LIBDNSSD_COMPATIBILITY ([e8d7cae](https://github.com/metaneutrons/snapdog-os/commit/e8d7cae1930ce3f92f70e118335de5c7fccd778e))
* add dnsmasq.service for SoftAP DHCP ([5659aa0](https://github.com/metaneutrons/snapdog-os/commit/5659aa056ffe4746cf2ff49585d0b1bfb647b6a9))
* add hostapd.service for SoftAP mode ([b580018](https://github.com/metaneutrons/snapdog-os/commit/b580018731e71be80b8ff1a31f609632a0dad5dc))
* add snapdog-ctrl package to meta-package (installs systemd service) ([6bd1d37](https://github.com/metaneutrons/snapdog-os/commit/6bd1d376c302a8bd3e293af54fc8dea7c0b3ddd8))
* address code review findings ([0be27bd](https://github.com/metaneutrons/snapdog-os/commit/0be27bdc313bae6867b58e08e04a9ae7e8c0887d))
* address remaining code review findings ([fe05f98](https://github.com/metaneutrons/snapdog-os/commit/fe05f98a7749e17baea5804162c38b4cb1f28346))
* **build:** proper config override without duplicates ([eb24032](https://github.com/metaneutrons/snapdog-os/commit/eb2403223c293c83cef6e7eec14974d307676ef4))
* **ci:** add ports.ubuntu.com for arm64 avahi cross-compile ([0bdce95](https://github.com/metaneutrons/snapdog-os/commit/0bdce9503a9f70d05af9353df01a910c3f87b3ab))
* **ci:** Docker container with --network=host (fixes DNS proxy bug) ([e96cc85](https://github.com/metaneutrons/snapdog-os/commit/e96cc85f2d0539d1a0f578571139d9ce171bb915))
* **ci:** install AWS CLI v2 directly (awscli package unavailable on 24.04) ([7592278](https://github.com/metaneutrons/snapdog-os/commit/7592278e14f1b7911b58091aa0019acd217e2676))
* **ci:** install libavahi-compat-libdnssd-dev for native clippy/test ([46e105b](https://github.com/metaneutrons/snapdog-os/commit/46e105b875cf9e4383536ef24a9312d4aca01de9))
* **ci:** replace heredoc with printf (YAML heredoc breaks parsing) ([9663583](https://github.com/metaneutrons/snapdog-os/commit/966358330856463a332a5da0b43b744d4da8e527))
* **ci:** run directly on host (Docker container networking broken on cachy) ([980c2c3](https://github.com/metaneutrons/snapdog-os/commit/980c2c3eefd44b30e96dc2ef429da55ef4a98d2b))
* **ci:** update sanity checks for RAUC + auto-reboot after update ([ed553a7](https://github.com/metaneutrons/snapdog-os/commit/ed553a7d77c569b0070d5a75cf1acbc6b1aa63e8))
* **ci:** use --network=host for container (Docker DNS broken on custom networks) ([641ebde](https://github.com/metaneutrons/snapdog-os/commit/641ebde1b405a74174d60531683ab829dd112805))
* **ci:** use DEB822 format for arm64 multiarch sources ([0f5e4b6](https://github.com/metaneutrons/snapdog-os/commit/0f5e4b6bd08df553ad2773ac0683ba064db06c7c))
* **ci:** use Docker container on self-hosted runner ([3ea0eaa](https://github.com/metaneutrons/snapdog-os/commit/3ea0eaab9f77f44bdbfe2a830d6875e4dd1c03c4))
* **ci:** use Docker container on self-hosted runner (cachy) ([84c801b](https://github.com/metaneutrons/snapdog-os/commit/84c801b38dfa208b625d65be9f1e27e69c44f5de))
* Config.in tab syntax error on line 16 ([63bcc7f](https://github.com/metaneutrons/snapdog-os/commit/63bcc7ff46dc3d7673660bf5ecb95b2bae5c63a0))
* **config:** set subsonic cache to tmpfs, remove managed=true ([20665e6](https://github.com/metaneutrons/snapdog-os/commit/20665e673a95589343a6c1d0cd84af3ae685aa77))
* create /var/empty for sshd privilege separation ([fcb82e0](https://github.com/metaneutrons/snapdog-os/commit/fcb82e0735e86fcb6262aa557ad8a2a835499882))
* create /var/lib subdirs via tmpfiles.d (proper solution) ([d033b93](https://github.com/metaneutrons/snapdog-os/commit/d033b93771fcaeb6679cff9221ad66917f9f1a77))
* default SNAPDOG_ROOT_DEV in .mk files (fixes kernel panic) ([2d3689b](https://github.com/metaneutrons/snapdog-os/commit/2d3689ba44116542458f1a05a9322ff40d1c6f3b))
* derive inactive partition from cmdline (supports NVMe + eMMC) ([6122eec](https://github.com/metaneutrons/snapdog-os/commit/6122eec7bde5dc4524226091a0449a929d2cc4ff))
* **dev:** add @parcel/watcher dependency (fixes pre-push hook on macOS) ([f9dd825](https://github.com/metaneutrons/snapdog-os/commit/f9dd825fbc6753de15a1665bbe24c5083f33fd93))
* **dev:** add libavahi-compat-libdnssd-dev to Dockerfile for astro-dnssd cross-compile ([ad16b53](https://github.com/metaneutrons/snapdog-os/commit/ad16b53a5313f6073698f9fa3f9b02d383602a64))
* **dev:** always rebuild snapdog-ctrl (no stale binary cache) ([383bfc8](https://github.com/metaneutrons/snapdog-os/commit/383bfc8ed1f0754cad7e0f33ab24e0185427844a))
* **dev:** update Dockerfile and docker-compose for current state ([d7ddefe](https://github.com/metaneutrons/snapdog-os/commit/d7ddefef2f05b3a51d9b2afb20965cc40ba11ae9))
* disable BR2_TARGET_GENERIC_HOSTNAME to avoid post-build conflict ([41ae71e](https://github.com/metaneutrons/snapdog-os/commit/41ae71e765063350fc8e5cf7fe3d8f634e98db15))
* don't invite users to manually edit managed config file ([b0d22bc](https://github.com/metaneutrons/snapdog-os/commit/b0d22bc37811c51f155d2b2c430036345c4d9ade))
* extract magic time constants in auto_update ([2b753ec](https://github.com/metaneutrons/snapdog-os/commit/2b753ecc763a28c2481cd624ba5545d2d75e798d))
* harden system operations and partition handling ([bd5441e](https://github.com/metaneutrons/snapdog-os/commit/bd5441e7d5fbdd7d4b6ba1fc38cf29133fa65c80))
* **kernel:** re-enable DRM for framebuffer console ([4265c22](https://github.com/metaneutrons/snapdog-os/commit/4265c22a6642b09588a70ef1f91853d260b13f08))
* Makefile tab character on line 11 ([a0d581d](https://github.com/metaneutrons/snapdog-os/commit/a0d581db60bc1cccc8ce9387ab52834035b60120))
* mask systemd-networkd-wait-online (cosmetic boot failure) ([fea9be5](https://github.com/metaneutrons/snapdog-os/commit/fea9be5b9a4889ca213a893d6957a567e9616bef))
* **mock:** complete mock coverage for all API endpoints ([f24b1bf](https://github.com/metaneutrons/snapdog-os/commit/f24b1bfb034bc339b75560ed864443dda8eb8783))
* **network:** only start AP if no network at all, auto-close on connect ([f15c7ca](https://github.com/metaneutrons/snapdog-os/commit/f15c7cae65bcbbb9828c2db92f6d4e19008fdb4a))
* never panic on critical health issues, show error screen instead ([3df22c7](https://github.com/metaneutrons/snapdog-os/commit/3df22c710cce5cb2908bb141b2fb1ebd0dde8d12))
* pre-format data partition + create /data mountpoint ([17387e5](https://github.com/metaneutrons/snapdog-os/commit/17387e5edfd83026514722baa2de7d2447247d02))
* regenerate package-lock.json (sync with package.json) ([ae80c73](https://github.com/metaneutrons/snapdog-os/commit/ae80c73908afdd3eefbc277e0103d43dbbc6ed8f))
* remove [Install] from client/server services (snapdog-ctrl manages them) ([db6aa59](https://github.com/metaneutrons/snapdog-os/commit/db6aa59083699ba750ff61527ffc794e6a1eaf2e))
* remove [Install] from hostapd/dnsmasq services (prevent auto-enable) ([b4acc0a](https://github.com/metaneutrons/snapdog-os/commit/b4acc0a25777fbdcc59901e3387bfbe1793087cc))
* resolve all lint issues + upgrade deps ([67a2fca](https://github.com/metaneutrons/snapdog-os/commit/67a2fca65ada68e8f664f3e1024efce1939a98e3))
* resolve all TODOs, dead code, and unjustified allows ([e2ca66a](https://github.com/metaneutrons/snapdog-os/commit/e2ca66a65d819640003e78a684f5dd0bcb230ab8))
* resolve target-finalize rsync error + trim kernel ([b5c6309](https://github.com/metaneutrons/snapdog-os/commit/b5c6309a886f8f60a33df50d737e76b042d6334c))
* snapdog-data-init must wait for /data mount ([28b3f83](https://github.com/metaneutrons/snapdog-os/commit/28b3f835bbcce4b48240931700a90d7bb9360ce0))
* **softap:** restart resolved + bind dnsmasq to wlan0 only ([f1129d7](https://github.com/metaneutrons/snapdog-os/commit/f1129d7391c2a62c0b7a51dba3af0df94b95217d))
* suppress dead_code warnings for unused RAUC helpers ([bce8fa7](https://github.com/metaneutrons/snapdog-os/commit/bce8fa79c54809e61dd2a41e63628fa155f5189d))
* suppress unused variable warning in trigger_update ([828488d](https://github.com/metaneutrons/snapdog-os/commit/828488d9430d7efae42b409cb183a8e9d2d0eda1))
* **ui:** About modal 50px narrower (398px) ([3ed6a1d](https://github.com/metaneutrons/snapdog-os/commit/3ed6a1d00023bf2d4620c46c102ac85bae8b7ce9))
* **ui:** About modal grid 2/3 + 1/3 (model/source wider, version/license narrower) ([a59c153](https://github.com/metaneutrons/snapdog-os/commit/a59c1534758dcc2f67fdd6f27cffe4c2a22cff10))
* **ui:** add placeholder hint to AirPlay password field (i18n) ([b2f484b](https://github.com/metaneutrons/snapdog-os/commit/b2f484b0a33257b5b5b03dc0289197283016338e))
* **ui:** codec-aware bit depth constraints ([a09800a](https://github.com/metaneutrons/snapdog-os/commit/a09800a1a7eabfad3addf80fa31dadbfbdb02c76))
* **ui:** remove translate-x overflow on About modal cards ([b662ff8](https://github.com/metaneutrons/snapdog-os/commit/b662ff8010db5217140d381ea4f5a7f6c87bb7ea))
* **ui:** update AutoUpdateSettings to match RAUC API (channel instead of interval) ([f8aa6d5](https://github.com/metaneutrons/snapdog-os/commit/f8aa6d51fe2167a0fa4a7adcaf8304c3f3dda1a5))
* **ui:** widen About modal (max-w-sm → max-w-md) ([c4447d6](https://github.com/metaneutrons/snapdog-os/commit/c4447d6e95411bbb883977dcb767a9fb72e3e7ea))
* **webui:** replace all direct fetch() with api client ([62351d3](https://github.com/metaneutrons/snapdog-os/commit/62351d361a91b7addf04d3972d7961183ac02f61))
* **webui:** resolve ESLint cascading setState error in auth effect ([caa1832](https://github.com/metaneutrons/snapdog-os/commit/caa18322731adfd3e11c002f58de18b58d0a1351))
* **ws:** exempt /api/ws from authentication and resolve all-features mDNS type mismatch ([d27f24e](https://github.com/metaneutrons/snapdog-os/commit/d27f24e3ebc8d19696c7222b592d05b4bdef7d8c))


### Performance Improvements

* **dev:** enable ccache for local Docker builds ([b34329a](https://github.com/metaneutrons/snapdog-os/commit/b34329a913186809aa86ab203c21aa366d56b8a4))
* empty rootfs-b partition (saves ~1GB image size) ([17e182d](https://github.com/metaneutrons/snapdog-os/commit/17e182d6a9fb346d86bf8b49223769dd3636e2cc))
* **kernel:** disable IIO, MD, NET_SCHED, BRIDGE ([c54ba10](https://github.com/metaneutrons/snapdog-os/commit/c54ba101e8c631bed0ac06dbdd4745c050067b31))
