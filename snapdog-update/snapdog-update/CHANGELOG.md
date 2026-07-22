# Changelog

## [0.4.0](https://github.com/SnapDogRocks/snapdog-os/compare/snapdog-update-v0.3.1...snapdog-update-v0.4.0) (2026-07-22)


### ⚠ BREAKING CHANGES

* In-device updates now accept only signed RAUC bundles; the raw flash API, WebUI, and CLI contract have been removed.

### Features

* remove raw image recovery hatch ([#134](https://github.com/SnapDogRocks/snapdog-os/issues/134)) ([a18f020](https://github.com/SnapDogRocks/snapdog-os/commit/a18f020638006867cd15bc9df66e4c436fa0807a))

## [0.3.1](https://github.com/SnapDogRocks/snapdog-os/compare/snapdog-update-v0.3.0...snapdog-update-v0.3.1) (2026-07-08)


### Bug Fixes

* **snapdog-update:** detect reboot via booted-slot bundle version ([#67](https://github.com/SnapDogRocks/snapdog-os/issues/67)) ([8b6db71](https://github.com/SnapDogRocks/snapdog-os/commit/8b6db71f4620852a22f2853e332809a4360f9f8f))

## [0.3.0](https://github.com/SnapDogRocks/snapdog-os/compare/snapdog-update-v0.2.0...snapdog-update-v0.3.0) (2026-07-08)


### Features

* **rauc:** tryboot A/B rollback and OTA upgrade hardening ([b4122bf](https://github.com/SnapDogRocks/snapdog-os/commit/b4122bff03624e24f5a5379b78a2c78b3e2c4a9e))


### Bug Fixes

* **snapdog-update:** decode install progress as percentage ([4300ca6](https://github.com/SnapDogRocks/snapdog-os/commit/4300ca655177a05d404b0437c5f68f61983639ad))
* **snapdog-update:** require 'installing' before treating idle as install-complete ([7632fd1](https://github.com/SnapDogRocks/snapdog-os/commit/7632fd1398a6e23001d09c89a35e0b25911484d1))
* **snapdog-update:** surface the real cause + a connectivity hint on transport errors ([0d29542](https://github.com/SnapDogRocks/snapdog-os/commit/0d295420859cb1ef69094fcff6192f7fd3d18bcb))
* **snapdog-update:** survive transient status polls and drive the reboot ([c3dbfff](https://github.com/SnapDogRocks/snapdog-os/commit/c3dbfffb7826643ed2e3af8bbf38fb1d1e2953d6))
* SSH, WiFi, DAC auto-detect and soundcard picker on the read-only rootfs ([66c7ee8](https://github.com/SnapDogRocks/snapdog-os/commit/66c7ee83797e55dec4da47c2c6332d30e9faa47f))

## [0.2.0](https://github.com/SnapDogRocks/snapdog-os/compare/snapdog-update-v0.1.0...snapdog-update-v0.2.0) (2026-06-07)


### Features

* **update:** add build script for compile-time git versioning ([460fd2b](https://github.com/SnapDogRocks/snapdog-os/commit/460fd2b6f2494bc133957e0ad774b1168cb41df7))
* **update:** harden update CLI for operators ([78a0c1e](https://github.com/SnapDogRocks/snapdog-os/commit/78a0c1e07c90208237a77b7e6cbb0dc891a7ffaa))
* **update:** implement developer firmware upgrade tool (snapdog-update) ([0527b5e](https://github.com/SnapDogRocks/snapdog-os/commit/0527b5e205bab43f0d5f61779c327731ba6ce07b))


### Bug Fixes

* replace all metaneutrons references with SnapDogRocks org ([431bf49](https://github.com/SnapDogRocks/snapdog-os/commit/431bf49df7aa5435fd5286572501536f556bfc2b))
* **update:** harden reboot verification ([f99b6d7](https://github.com/SnapDogRocks/snapdog-os/commit/f99b6d765ab4411ae04563d983aacbeaa0064a5e))
