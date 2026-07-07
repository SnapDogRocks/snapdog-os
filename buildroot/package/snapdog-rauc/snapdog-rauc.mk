################################################################################
#
# snapdog-rauc
#
# RAUC configuration: system.conf, custom boot handler, keyring, services.
#
################################################################################

SNAPDOG_ROOT_DEV ?= /dev/mmcblk0p
SNAPDOG_RAUC_DEPENDENCIES = rauc

# Small helper that issues reboot(2) RESTART2 "0 tryboot" (systemd cannot set the
# RPi tryboot flag on this image).
define SNAPDOG_RAUC_BUILD_CMDS
	$(TARGET_CC) $(TARGET_CFLAGS) $(TARGET_LDFLAGS) -Os \
		-o $(@D)/tryboot-reboot \
		$(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-rauc/tryboot-reboot.c
endef

define SNAPDOG_RAUC_INSTALL_TARGET_CMDS
	# The RAUC compatible must be board-specific (snapdog-os-<board>) so a bundle
	# for one Pi model can never install on another. Fail loudly rather than bake a
	# board-less "snapdog-os-" that silently accepts any board's bundle. Every build
	# path sets it: release.yml (per matrix board) + dev/docker-compose.yml (pi4).
	test -n "$(SNAPDOG_BOARD)" || { echo "snapdog-rauc: SNAPDOG_BOARD is empty — set it (e.g. SNAPDOG_BOARD=pi4); see dev/docker-compose.yml and .github/workflows/release.yml" >&2; exit 1; }

	# system.conf (substitute board compatible)
	mkdir -p $(TARGET_DIR)/etc/rauc
	sed -e 's/@SNAPDOG_BOARD@/$(SNAPDOG_BOARD)/' \
		-e 's|@SNAPDOG_ROOT_DEV@|$(SNAPDOG_ROOT_DEV)|' \
		$(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-rauc/system.conf.in \
		> $(TARGET_DIR)/etc/rauc/system.conf

	# Keyring (CA certificate for bundle verification)
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/keys/rauc-ca.cert.pem \
		$(TARGET_DIR)/etc/rauc/ca.cert.pem

	# Custom bootloader backend
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-rauc/boot-handler.sh \
		$(TARGET_DIR)/usr/lib/rauc/boot-handler.sh

	# mark-good + tryboot commit/reconcile (run by rauc-mark-good.service)
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-rauc/rauc-commit.sh \
		$(TARGET_DIR)/opt/snapdog/bin/rauc-commit

	# tryboot RESTART2 helper (invoked by snapdog-ctrl, which carries CAP_SYS_BOOT)
	$(INSTALL) -D -m 0755 $(@D)/tryboot-reboot \
		$(TARGET_DIR)/usr/lib/rauc/tryboot-reboot
endef

define SNAPDOG_RAUC_INSTALL_INIT_SYSTEMD
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-rauc/rauc-mark-good.service \
		$(TARGET_DIR)/usr/lib/systemd/system/rauc-mark-good.service
	mkdir -p $(TARGET_DIR)/etc/systemd/system/multi-user.target.wants
	ln -sf /usr/lib/systemd/system/rauc-mark-good.service \
		$(TARGET_DIR)/etc/systemd/system/multi-user.target.wants/rauc-mark-good.service

	# OnFailure target for snapdog-ctrl (reverts a crash-looping tryboot trial).
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-rauc/rauc-trial-failed.service \
		$(TARGET_DIR)/usr/lib/systemd/system/rauc-trial-failed.service
endef

$(eval $(generic-package))
