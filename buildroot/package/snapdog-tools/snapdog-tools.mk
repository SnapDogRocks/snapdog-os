################################################################################
#
# snapdog-tools
#
################################################################################

define SNAPDOG_TOOLS_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-tools/resize-data-partition \
		$(TARGET_DIR)/opt/snapdog/bin/resize-data-partition
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-tools/snd_soc_core_disable_pm.conf \
		$(TARGET_DIR)/etc/modprobe.d/snd_soc_core_disable_pm.conf
	# Speed up network-online.target
	sed -i "s/ExecStart.*/ExecStart=\/usr\/lib\/systemd\/systemd-networkd-wait-online --timeout=20 --any/" \
		$(TARGET_DIR)/usr/lib/systemd/system/systemd-networkd-wait-online.service
	[ -d $(TARGET_DIR)/boot ] || mkdir $(TARGET_DIR)/boot
endef

define SNAPDOG_TOOLS_INSTALL_INIT_SYSTEMD
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-tools/resize-data-partition.service \
		$(TARGET_DIR)/usr/lib/systemd/system/resize-data-partition.service
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/snapdog-tools/journald.conf \
		$(TARGET_DIR)/etc/systemd/journald.conf
endef

$(eval $(generic-package))
