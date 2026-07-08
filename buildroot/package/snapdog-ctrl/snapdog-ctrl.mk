################################################################################
#
# snapdog-ctrl
#
# Binary is cross-compiled externally (CI or local Docker build) and placed
# at $(BINARIES_DIR)/snapdog-ctrl before the image build runs.
#
################################################################################

define SNAPDOG_CTRL_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(BINARIES_DIR)/snapdog-ctrl \
		$(TARGET_DIR)/usr/bin/snapdog-ctrl
endef

define SNAPDOG_CTRL_INSTALL_INIT_SYSTEMD
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-ctrl/snapdog-ctrl.service \
		$(TARGET_DIR)/usr/lib/systemd/system/snapdog-ctrl.service
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-ctrl/snapdog-boot-remount@.service \
		$(TARGET_DIR)/usr/lib/systemd/system/snapdog-boot-remount@.service
	mkdir -p $(TARGET_DIR)/etc/systemd/system/multi-user.target.wants
	ln -sf /usr/lib/systemd/system/snapdog-ctrl.service \
		$(TARGET_DIR)/etc/systemd/system/multi-user.target.wants/snapdog-ctrl.service
endef

$(eval $(generic-package))
