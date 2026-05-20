################################################################################
#
# copy-overlays
#
################################################################################

COPY_OVERLAYS_DEPENDENCIES = rpi-firmware linux

define COPY_OVERLAYS_INSTALL_TARGET_CMDS
	mkdir -p $(TARGET_DIR)/usr/lib/firmware/rpi/overlays
	# Copy I2S DAC/AMP overlays + common overlays
	for i in allo iqaudio justboom max98357a googlevoicehat \
		 i-sabre fe-pi adau7002 \
		 vc4 i2c-gpio gpio-ir cma dwc disable rpi- spi i2c uart; do \
		cp -v $(BUILD_DIR)/linux-custom/arch/arm64/boot/dts/overlays/$$i*.dtbo \
			$(BINARIES_DIR)/rpi-firmware/overlays 2>/dev/null || true; \
		cp -v $(BUILD_DIR)/linux-custom/arch/arm64/boot/dts/overlays/$$i*.dtbo \
			$(TARGET_DIR)/usr/lib/firmware/rpi/overlays 2>/dev/null || true; \
	done
endef

$(eval $(generic-package))
