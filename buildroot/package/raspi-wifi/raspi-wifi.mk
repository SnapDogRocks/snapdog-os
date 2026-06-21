################################################################################
#
# raspi-wifi
#
# Pulls in hostapd, wpa_supplicant, and firmware.
# Installs hostapd systemd service (not enabled by default — snapdog-ctrl
# starts it on demand for SoftAP mode).
#
################################################################################

define RASPI_WIFI_INSTALL_INIT_SYSTEMD
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/raspi-wifi/hostapd.service \
		$(TARGET_DIR)/usr/lib/systemd/system/hostapd.service
endef

$(eval $(generic-package))
