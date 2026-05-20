################################################################################
#
# snapdog-updater
#
################################################################################

SNAPDOG_UPDATER_DEPENDENCIES = rpi-firmware systemd copy-overlays

define SNAPDOG_UPDATER_INSTALL_TARGET_CMDS
	# Update scripts
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-updater/update \
		$(TARGET_DIR)/opt/snapdog/bin/update
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-updater/extract-update \
		$(TARGET_DIR)/opt/snapdog/bin/extract-update
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-updater/update-firmware \
		$(TARGET_DIR)/opt/snapdog/bin/update-firmware
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-updater/partitions \
		$(TARGET_DIR)/opt/snapdog/bin/partitions
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-updater/reactivate-previous-release \
		$(TARGET_DIR)/opt/snapdog/bin/reactivate-previous-release
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-updater/boot-guard \
		$(TARGET_DIR)/opt/snapdog/bin/boot-guard
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-updater/backup-config \
		$(TARGET_DIR)/opt/snapdog/bin/backup-config
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-updater/restore-config \
		$(TARGET_DIR)/opt/snapdog/bin/restore-config

	# Version and config
	$(INSTALL) -D -m 0444 $(BR2_EXTERNAL_SNAPDOG_PATH)/VERSION \
		$(TARGET_DIR)/etc/snapdog-os.version
	$(INSTALL) -D -m 0444 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-updater/config-files \
		$(TARGET_DIR)/opt/snapdog/etc/config-files

	# Systemd units
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-updater/updater.service \
		$(TARGET_DIR)/usr/lib/systemd/system/updater.service
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-updater/updater.timer \
		$(TARGET_DIR)/usr/lib/systemd/system/updater.timer
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-updater/boot-guard.service \
		$(TARGET_DIR)/usr/lib/systemd/system/boot-guard.service

	# Kernel image for OTA
	$(INSTALL) -D -m 0644 $(BUILD_DIR)/linux-custom/arch/arm64/boot/Image \
		$(TARGET_DIR)/usr/lib/firmware/rpi/Image

	# Firmware for OTA
	mkdir -p $(TARGET_DIR)/usr/lib/firmware/rpi
	cp -v $(BUILD_DIR)/rpi-firmware-$(RPI_FIRMWARE_VERSION)/boot/start*.elf \
		$(TARGET_DIR)/usr/lib/firmware/rpi/ 2>/dev/null || true
	cp -v $(BUILD_DIR)/rpi-firmware-$(RPI_FIRMWARE_VERSION)/boot/fixup*.dat \
		$(TARGET_DIR)/usr/lib/firmware/rpi/ 2>/dev/null || true
endef

$(eval $(generic-package))
