################################################################################
#
# raspi-wifi
#
# Pulls in hostapd, wpa_supplicant, and firmware.
# Installs the hostapd and wpa_supplicant@ systemd units (neither enabled by
# default — snapdog-ctrl starts them on demand: hostapd for SoftAP mode,
# wpa_supplicant@<iface> for WiFi client mode).
#
################################################################################

define RASPI_WIFI_INSTALL_INIT_SYSTEMD
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/raspi-wifi/hostapd.service \
		$(TARGET_DIR)/usr/lib/systemd/system/hostapd.service
	$(INSTALL) -D -m 0644 $(BR2_EXTERNAL_SNAPDOG_PATH)/package/raspi-wifi/wpa_supplicant@.service \
		$(TARGET_DIR)/usr/lib/systemd/system/wpa_supplicant@.service
endef

$(eval $(generic-package))
