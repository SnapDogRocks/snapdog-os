################################################################################
#
# snapdog-server
#
# Binary comes from the same release tarball as snapdog-client.
#
################################################################################

SNAPDOG_SERVER_VERSION = 0.24.1
SNAPDOG_SERVER_SOURCE = snapdog-v$(SNAPDOG_SERVER_VERSION)-aarch64-unknown-linux-gnu.tar.gz
SNAPDOG_SERVER_SITE = https://github.com/SnapDogRocks/snapdog/releases/download/v$(SNAPDOG_SERVER_VERSION)
SNAPDOG_SERVER_LICENSE = GPL-3.0-only

define SNAPDOG_SERVER_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/snapdog \
		$(TARGET_DIR)/usr/bin/snapdog
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-server/snapdog.toml \
		$(TARGET_DIR)/etc/snapdog/snapdog.toml.default
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-server/snapdog-data-init \
		$(TARGET_DIR)/usr/bin/snapdog-data-init
endef

define SNAPDOG_SERVER_INSTALL_INIT_SYSTEMD
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-server/snapdog.service \
		$(TARGET_DIR)/usr/lib/systemd/system/snapdog.service
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-server/snapdog-data-init.service \
		$(TARGET_DIR)/usr/lib/systemd/system/snapdog-data-init.service
	mkdir -p $(TARGET_DIR)/etc/systemd/system/sysinit.target.wants
	ln -sf /usr/lib/systemd/system/snapdog-data-init.service \
		$(TARGET_DIR)/etc/systemd/system/sysinit.target.wants/snapdog-data-init.service
endef

$(eval $(generic-package))
