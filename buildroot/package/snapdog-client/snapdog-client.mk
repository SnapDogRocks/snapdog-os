################################################################################
#
# snapdog-client
#
################################################################################

SNAPDOG_CLIENT_VERSION = 0.11.3
SNAPDOG_CLIENT_SOURCE = snapdog-v$(SNAPDOG_CLIENT_VERSION)-aarch64-unknown-linux-gnu.tar.gz
SNAPDOG_CLIENT_SITE = https://github.com/SnapDogRocks/snapdog/releases/download/v$(SNAPDOG_CLIENT_VERSION)
SNAPDOG_CLIENT_LICENSE = GPL-3.0-only

define SNAPDOG_CLIENT_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(@D)/snapdog-client \
		$(TARGET_DIR)/usr/bin/snapdog-client
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-client/snapdog-client.default \
		$(TARGET_DIR)/etc/default/snapdog-client.default
endef

define SNAPDOG_CLIENT_INSTALL_INIT_SYSTEMD
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-client/snapdog-client.service \
		$(TARGET_DIR)/usr/lib/systemd/system/snapdog-client.service
endef

$(eval $(generic-package))
