# Changelog

## [0.6.0](https://github.com/SnapDogRocks/snapdog-os/compare/snapdog-ctrl-v0.5.0...snapdog-ctrl-v0.6.0) (2026-06-21)


### Features

* **network:** in-process captive DNS + DHCP option 114, drop dnsmasq ([1438904](https://github.com/SnapDogRocks/snapdog-os/commit/143890477991a21f50d1f5219953df295e991fde))


### Bug Fixes

* **network:** ConfigureWithoutCarrier on AP so networkd assigns IP before hostapd ([32a9143](https://github.com/SnapDogRocks/snapdog-os/commit/32a91431056e84a08fcc17f0fe6b0b70cb1a2f6b))
* **server_config:** drop redundant deref in KNX test asserts ([f94b486](https://github.com/SnapDogRocks/snapdog-os/commit/f94b486055cd8448af21d07ca406afb8e7d588db))
* **webui:** bump hono to 4.12.26 and js-yaml to 4.2.0 ([0fbc71e](https://github.com/SnapDogRocks/snapdog-os/commit/0fbc71ee133488661f8a0eb1216e508980481f0f))

## [0.5.0](https://github.com/SnapDogRocks/snapdog-os/compare/snapdog-ctrl-v0.4.0...snapdog-ctrl-v0.5.0) (2026-06-14)


### Features

* **ctrl:** expand KNX zone group addresses in config generator ([65b23a3](https://github.com/SnapDogRocks/snapdog-os/commit/65b23a3a81cece4743e0408d14879be5989bf985))
* **webui:** expand settings UI with KNX addresses and styling improvements ([ac25040](https://github.com/SnapDogRocks/snapdog-os/commit/ac25040442edfa96652c53ddf313779d8d3a82cb))

## [0.4.0](https://github.com/SnapDogRocks/snapdog-os/compare/snapdog-ctrl-v0.3.0...snapdog-ctrl-v0.4.0) (2026-06-07)


### Features

* **audio:** auto-detect DAC at startup + immediate reboot ([420121f](https://github.com/SnapDogRocks/snapdog-os/commit/420121f4aa24638ccff8d13865e8ba4052865411))
* **audio:** auto-detect DAC from HAT EEPROM ([4afa543](https://github.com/SnapDogRocks/snapdog-os/commit/4afa54353de4b27809f5ed99ca342860202bc001))
* **audio:** auto-detect DAC UX improvements ([24f63af](https://github.com/SnapDogRocks/snapdog-os/commit/24f63af7b6d4ec298a06b795488c1c564d400ec0))
* **audio:** default codec f32lz4 + 32-bit depth ([c731e3d](https://github.com/SnapDogRocks/snapdog-os/commit/c731e3d8959faf7b19f516d466cf72e306f4e8da))
* **auth:** optional password protection for web UI ([4fc0f6d](https://github.com/SnapDogRocks/snapdog-os/commit/4fc0f6dcc41bbbfefab0d0f630f5687daafb0e5d))
* **auth:** unified device password (WebUI + console) ([f541db4](https://github.com/SnapDogRocks/snapdog-os/commit/f541db49b21ac652a266e34c6673758fe996aeab))
* **ctrl:** derive version from git describe ([9147656](https://github.com/SnapDogRocks/snapdog-os/commit/9147656d976dc0eb9b5ce718e7ed8c2ec302c17a))
* **ctrl:** Now Playing mini-player with MPRIS2 D-Bus integration ([c2757f5](https://github.com/SnapDogRocks/snapdog-os/commit/c2757f5f3bd73b7d77f68572576fb4815cae7777))
* **ctrl:** show real IP address in startup log ([64ed85e](https://github.com/SnapDogRocks/snapdog-os/commit/64ed85e112036abb96fa7f0c960ca53d93571752))
* **mdns:** feature-gated mDNS backends (astro-dnssd default, mdns-sd alt) ([a0662c4](https://github.com/SnapDogRocks/snapdog-os/commit/a0662c4b9a41e4f5ebe19d08386c6a50e1bca141))
* preflight health check + warning banner in WebUI ([c6ac7df](https://github.com/SnapDogRocks/snapdog-os/commit/c6ac7df81d1799000cfe5d6d0cb8f5f3d63f045d))
* **rauc:** enterprise-grade OTA via RAUC ([ff030f1](https://github.com/SnapDogRocks/snapdog-os/commit/ff030f1c7a174ab7b44955985070a46fc5b304e1))
* **rauc:** Phase 3 — snapdog-ctrl D-Bus integration ([36143f5](https://github.com/SnapDogRocks/snapdog-os/commit/36143f5c05d4229112858544aa104d3058af5d9e))
* reboot confirmation after manual update + raw flash escape hatch ([8d33f0f](https://github.com/SnapDogRocks/snapdog-os/commit/8d33f0f76ffbcb4ac482c3e87fd89a77b5269cc2))
* **server:** add name, advertise_snapcast, airplay.mode, subsonic.format, client.icon, client.max_volume ([f020f7c](https://github.com/SnapDogRocks/snapdog-os/commit/f020f7c2005bd87037e28399a993f5ff796ca7e7))
* **server:** API keys management in WebUI ([1e57ad9](https://github.com/SnapDogRocks/snapdog-os/commit/1e57ad939f8359c5147b6500d841ed914262445a))
* **server:** backend — toml_edit config module + buildroot package + API ([108be99](https://github.com/SnapDogRocks/snapdog-os/commit/108be99e17987e01b4f54d94f616a97754720e55))
* **settings:** export/import device settings as tar.gz ([f031922](https://github.com/SnapDogRocks/snapdog-os/commit/f03192269e910e23167592f6a1b2e9cada1ba46a))
* show component versions in Dashboard ([f700a42](https://github.com/SnapDogRocks/snapdog-os/commit/f700a42ffcf36d0c286c4f95fd21d4e8faa47e60))
* snapdog-ctrl manages all optional services ([a22fd91](https://github.com/SnapDogRocks/snapdog-os/commit/a22fd918a1d195cb20e2c36db6f5d49d61cf8904))
* **snapdog-ctrl:** Rust device config service ([a033567](https://github.com/SnapDogRocks/snapdog-os/commit/a033567ed8cd23756bdfacc7058b7c6db3df7faa))
* **softap:** configurable enable + password via ctrl.toml ([857b4c7](https://github.com/SnapDogRocks/snapdog-os/commit/857b4c7482f93186952de61f502fa7212f1789b6))
* **softap:** derive unique SSID from WiFi MAC address ([f4163e1](https://github.com/SnapDogRocks/snapdog-os/commit/f4163e190e83df7fdb40bc34e00d3b8b7b8384ec))
* **tuning:** add accessible hover-and-tap tooltips for tuning options ([d800c2a](https://github.com/SnapDogRocks/snapdog-os/commit/d800c2aa00682da1b98740bbbe4728c3d2abb5f4))
* **tuning:** implement device-agnostic hardware tuning HAL ([a5dc600](https://github.com/SnapDogRocks/snapdog-os/commit/a5dc6009913843a8947514cf3a4c8e3fa541c621))
* **tuning:** write full RT scheduling overrides in systemd drop-in ([9e4333f](https://github.com/SnapDogRocks/snapdog-os/commit/9e4333ff5099d6ca3c692b49a5dae53c69135db1))
* **ui:** device name, emoji picker, volume slider, airplay mode, subsonic format ([6dc0848](https://github.com/SnapDogRocks/snapdog-os/commit/6dc0848ce223a7e273dc745b82df3dff42c9a811))
* **update:** add interval setting (daily/weekly/monthly) ([2e07fb7](https://github.com/SnapDogRocks/snapdog-os/commit/2e07fb719df085729a52aba3ad2e4e690c3f1d8f))
* **update:** auto-update scheduler ([28780b5](https://github.com/SnapDogRocks/snapdog-os/commit/28780b5727d03a2a54e967042a75ec65e9c434cd))
* **webui:** Next.js 16 static UI with 7 tabs ([77ed96e](https://github.com/SnapDogRocks/snapdog-os/commit/77ed96eeccde36c32146687b6ea678447e2bb231))
* **webui:** Server tab with sub-tabs + Client enable/disable ([0b1b53b](https://github.com/SnapDogRocks/snapdog-os/commit/0b1b53b55d23b68dba2e4f35f0e763f70782ee82))
* **webui:** upgrade Next.js 15→16 (Turbopack) ([00a1a95](https://github.com/SnapDogRocks/snapdog-os/commit/00a1a9558fbde9f07a4eef4dfe0828d89535ed32))


### Bug Fixes

* address code review findings ([0be27bd](https://github.com/SnapDogRocks/snapdog-os/commit/0be27bdc313bae6867b58e08e04a9ae7e8c0887d))
* address remaining code review findings ([fe05f98](https://github.com/SnapDogRocks/snapdog-os/commit/fe05f98a7749e17baea5804162c38b4cb1f28346))
* **ci:** add x86_64 native optional deps for Next.js Turbopack ([632e219](https://github.com/SnapDogRocks/snapdog-os/commit/632e219b53656811b63eb83a9a874ceaf681ee00))
* **ci:** downgrade Next.js 16→15 (removes Turbopack native dep requirement) ([51c3cd6](https://github.com/SnapDogRocks/snapdog-os/commit/51c3cd6ce2439593a569145d2b436aea4bae7d8e))
* **ci:** update sanity checks for RAUC + auto-reboot after update ([ed553a7](https://github.com/SnapDogRocks/snapdog-os/commit/ed553a7d77c569b0070d5a75cf1acbc6b1aa63e8))
* **ci:** use npm install instead of npm ci (resolves platform-specific native deps) ([393161a](https://github.com/SnapDogRocks/snapdog-os/commit/393161a8228156183e1a26acece038acd5d65f71))
* **config:** set subsonic cache to tmpfs, remove managed=true ([20665e6](https://github.com/SnapDogRocks/snapdog-os/commit/20665e673a95589343a6c1d0cd84af3ae685aa77))
* correct update URL (updates.snapdog.cc, not update.snapdog.cc) ([dc49338](https://github.com/SnapDogRocks/snapdog-os/commit/dc49338c224089ce9d0388018380db6a0774cf93))
* derive inactive partition from cmdline (supports NVMe + eMMC) ([6122eec](https://github.com/SnapDogRocks/snapdog-os/commit/6122eec7bde5dc4524226091a0449a929d2cc4ff))
* **dev:** add @parcel/watcher dependency (fixes pre-push hook on macOS) ([f9dd825](https://github.com/SnapDogRocks/snapdog-os/commit/f9dd825fbc6753de15a1665bbe24c5083f33fd93))
* don't invite users to manually edit managed config file ([b0d22bc](https://github.com/SnapDogRocks/snapdog-os/commit/b0d22bc37811c51f155d2b2c430036345c4d9ade))
* downgrade eslint to v9 (v10 incompatible with eslint-config-next) ([3d794f2](https://github.com/SnapDogRocks/snapdog-os/commit/3d794f2ba09bbfd2b56935cf7a1ea73e4a9c5e4f))
* extract magic time constants in auto_update ([2b753ec](https://github.com/SnapDogRocks/snapdog-os/commit/2b753ecc763a28c2481cd624ba5545d2d75e798d))
* harden system operations and partition handling ([bd5441e](https://github.com/SnapDogRocks/snapdog-os/commit/bd5441e7d5fbdd7d4b6ba1fc38cf29133fa65c80))
* **mock:** complete mock coverage for all API endpoints ([f24b1bf](https://github.com/SnapDogRocks/snapdog-os/commit/f24b1bfb034bc339b75560ed864443dda8eb8783))
* **network:** kernel panic in brcmfmac P2P during AP start ([953dbc8](https://github.com/SnapDogRocks/snapdog-os/commit/953dbc8a925b4caee6c198dfdc4e9da686616fb6))
* **network:** only start AP if no network at all, auto-close on connect ([f15c7ca](https://github.com/SnapDogRocks/snapdog-os/commit/f15c7cae65bcbbb9828c2db92f6d4e19008fdb4a))
* **network:** stop resolved before starting dnsmasq in AP mode ([3c06e08](https://github.com/SnapDogRocks/snapdog-os/commit/3c06e08efb9e6531d5250642c3e9090974e519b9))
* **network:** validate setup SSID derivation ([25141aa](https://github.com/SnapDogRocks/snapdog-os/commit/25141aa8256230d36db837a4701b46a10fc367a4))
* never panic on critical health issues, show error screen instead ([3df22c7](https://github.com/SnapDogRocks/snapdog-os/commit/3df22c710cce5cb2908bb141b2fb1ebd0dde8d12))
* regenerate lockfile with Node 22.13.0 (matches CI) ([3ec4807](https://github.com/SnapDogRocks/snapdog-os/commit/3ec48075c9f37e739bc7b0857a895b7baa1eecdc))
* regenerate package-lock.json (sync with package.json) ([ae80c73](https://github.com/SnapDogRocks/snapdog-os/commit/ae80c73908afdd3eefbc277e0103d43dbbc6ed8f))
* remove core dump from repo, add to gitignore ([70ced5e](https://github.com/SnapDogRocks/snapdog-os/commit/70ced5e627cfa8c30efe87c5e9501d9b0e836063))
* remove core dump, add to gitignore ([5804820](https://github.com/SnapDogRocks/snapdog-os/commit/5804820e8d4fc9931eadecb08a625dd3fd520a45))
* replace all metaneutrons references with SnapDogRocks org ([431bf49](https://github.com/SnapDogRocks/snapdog-os/commit/431bf49df7aa5435fd5286572501536f556bfc2b))
* resolve all lint issues + upgrade deps ([67a2fca](https://github.com/SnapDogRocks/snapdog-os/commit/67a2fca65ada68e8f664f3e1024efce1939a98e3))
* resolve all TODOs, dead code, and unjustified allows ([e2ca66a](https://github.com/SnapDogRocks/snapdog-os/commit/e2ca66a65d819640003e78a684f5dd0bcb230ab8))
* resolve clippy warnings for closures and map_or in config_txt.rs ([7a10f25](https://github.com/SnapDogRocks/snapdog-os/commit/7a10f2533fb720690ba8f6d6f41ff4498c7b622f))
* resolve mpris_client compilation and clippy errors in release builds ([aa5a5d2](https://github.com/SnapDogRocks/snapdog-os/commit/aa5a5d2884a33012830828275ec37969509385ae))
* **softap:** restart resolved + bind dnsmasq to wlan0 only ([f1129d7](https://github.com/SnapDogRocks/snapdog-os/commit/f1129d7391c2a62c0b7a51dba3af0df94b95217d))
* suppress dead_code warnings for unused RAUC helpers ([bce8fa7](https://github.com/SnapDogRocks/snapdog-os/commit/bce8fa79c54809e61dd2a41e63628fa155f5189d))
* suppress unused variable warning in trigger_update ([828488d](https://github.com/SnapDogRocks/snapdog-os/commit/828488d9430d7efae42b409cb183a8e9d2d0eda1))
* **tuning:** make config.txt parsing robust to spacing, inline comments, and arguments ([703ccf0](https://github.com/SnapDogRocks/snapdog-os/commit/703ccf0869945ed4705d3c17ad5ceb1909373ebb))
* **tuning:** resolve clippy compiler and lint warnings ([b95844f](https://github.com/SnapDogRocks/snapdog-os/commit/b95844f9f1b7f9175e369ebcdbeafe0afa0a70b5))
* **ui:** About modal 50px narrower (398px) ([3ed6a1d](https://github.com/SnapDogRocks/snapdog-os/commit/3ed6a1d00023bf2d4620c46c102ac85bae8b7ce9))
* **ui:** About modal grid 2/3 + 1/3 (model/source wider, version/license narrower) ([a59c153](https://github.com/SnapDogRocks/snapdog-os/commit/a59c1534758dcc2f67fdd6f27cffe4c2a22cff10))
* **ui:** add placeholder hint to AirPlay password field (i18n) ([b2f484b](https://github.com/SnapDogRocks/snapdog-os/commit/b2f484b0a33257b5b5b03dc0289197283016338e))
* **ui:** codec-aware bit depth constraints ([a09800a](https://github.com/SnapDogRocks/snapdog-os/commit/a09800a1a7eabfad3addf80fa31dadbfbdb02c76))
* **ui:** remove translate-x overflow on About modal cards ([b662ff8](https://github.com/SnapDogRocks/snapdog-os/commit/b662ff8010db5217140d381ea4f5a7f6c87bb7ea))
* **ui:** update AutoUpdateSettings to match RAUC API (channel instead of interval) ([f8aa6d5](https://github.com/SnapDogRocks/snapdog-os/commit/f8aa6d51fe2167a0fa4a7adcaf8304c3f3dda1a5))
* **ui:** widen About modal (max-w-sm → max-w-md) ([c4447d6](https://github.com/SnapDogRocks/snapdog-os/commit/c4447d6e95411bbb883977dcb767a9fb72e3e7ea))
* **update:** extend raw flash challenge ttl ([0ecd2e5](https://github.com/SnapDogRocks/snapdog-os/commit/0ecd2e554b49ae52f1b391d974fbe26fef7eb91e))
* **webui:** replace all direct fetch() with api client ([62351d3](https://github.com/SnapDogRocks/snapdog-os/commit/62351d361a91b7addf04d3972d7961183ac02f61))
* **webui:** resolve ESLint cascading setState error in auth effect ([caa1832](https://github.com/SnapDogRocks/snapdog-os/commit/caa18322731adfd3e11c002f58de18b58d0a1351))
* write OS version to /etc/snapdog-os.version during build ([bca0208](https://github.com/SnapDogRocks/snapdog-os/commit/bca0208087e2495d72cd05f358e4def07a783295))
* **ws:** exempt /api/ws from authentication and resolve all-features mDNS type mismatch ([d27f24e](https://github.com/SnapDogRocks/snapdog-os/commit/d27f24e3ebc8d19696c7222b592d05b4bdef7d8c))
