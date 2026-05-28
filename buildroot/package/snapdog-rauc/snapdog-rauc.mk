################################################################################
#
# snapdog-rauc
#
# RAUC configuration: system.conf, custom boot handler, keyring, services.
#
################################################################################

SNAPDOG_ROOT_DEV ?= /dev/mmcblk0p
SNAPDOG_RAUC_DEPENDENCIES = rauc

define SNAPDOG_RAUC_INSTALL_TARGET_CMDS
	# system.conf (substitute board compatible)
	mkdir -p $(TARGET_DIR)/etc/rauc
	sed -e 's/@SNAPDOG_PI_VERSION@/pi$(SNAPDOG_PI_VERSION)/' \
		-e 's|@SNAPDOG_ROOT_DEV@|$(SNAPDOG_ROOT_DEV)|' \
		$(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-rauc/system.conf.in \
		> $(TARGET_DIR)/etc/rauc/system.conf

	# Keyring (CA certificate for bundle verification)
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/keys/rauc-ca.cert.pem \
		$(TARGET_DIR)/etc/rauc/ca.cert.pem

	# Custom bootloader backend
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-rauc/boot-handler.sh \
		$(TARGET_DIR)/usr/lib/rauc/boot-handler.sh
endef

define SNAPDOG_RAUC_INSTALL_INIT_SYSTEMD
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-rauc/rauc-mark-good.service \
		$(TARGET_DIR)/usr/lib/systemd/system/rauc-mark-good.service
	mkdir -p $(TARGET_DIR)/etc/systemd/system/multi-user.target.wants
	ln -sf /usr/lib/systemd/system/rauc-mark-good.service \
		$(TARGET_DIR)/etc/systemd/system/multi-user.target.wants/rauc-mark-good.service
endef

$(eval $(generic-package))
