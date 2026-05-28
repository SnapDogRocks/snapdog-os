################################################################################
#
# configtxt
#
################################################################################

CONFIGTXT_DEPENDENCIES = rpi-firmware

define CONFIGTXT_INSTALL_TARGET_CMDS
	sed -i '/dtparam=i2c/d' $(BINARIES_DIR)/rpi-firmware/config.txt
	sed -i '/dtparam=spi/d' $(BINARIES_DIR)/rpi-firmware/config.txt
	echo "dtparam=i2c=on" >> $(BINARIES_DIR)/rpi-firmware/config.txt
	echo "dtparam=spi=on" >> $(BINARIES_DIR)/rpi-firmware/config.txt
	echo "dtparam=audio=off" >> $(BINARIES_DIR)/rpi-firmware/config.txt
endef

define CONFIGTXT_BASE
	sed -i '/kernel=/d' $(BINARIES_DIR)/rpi-firmware/config.txt
	echo "kernel=Image" >> $(BINARIES_DIR)/rpi-firmware/config.txt
	echo "dtoverlay=dwc2" >> $(BINARIES_DIR)/rpi-firmware/config.txt
endef

define CONFIGTXT_REMOVESTUFF
	sed -i '/imx219/d' $(BINARIES_DIR)/rpi-firmware/config.txt
	sed -i '/ov5647/d' $(BINARIES_DIR)/rpi-firmware/config.txt
endef

define CONFIGTXT_QUIET_INSTALL_TARGET_CMDS
	sed 's|@SNAPDOG_ROOT_DEV@|$(SNAPDOG_ROOT_DEV)|' \
		$(BR2_EXTERNAL_SNAPDOG_PATH)/package/configtxt/cmdline.quiet.in \
		> $(BINARIES_DIR)/rpi-firmware/cmdline.txt
endef


define CONFIGTXT_EEPROM_WORKAROUND
	sed -i '/force_eeprom_read/d' $(BINARIES_DIR)/rpi-firmware/config.txt
	echo "force_eeprom_read=0" >> $(BINARIES_DIR)/rpi-firmware/config.txt
endef

define CONFIGTXT_ENABLE_EEPROM_I2C
	sed -i '/dtoverlay=i2c-gpio/d' $(BINARIES_DIR)/rpi-firmware/config.txt
	echo "dtoverlay=i2c-gpio,i2c_gpio_sda=0,i2c_gpio_scl=1" >> $(BINARIES_DIR)/rpi-firmware/config.txt
endef

define CONFIGTXT_DAC_OVERLAY
	$(if $(BR2_PACKAGE_CONFIGTXT_DAC_OVERLAY),\
		echo "dtoverlay=$(BR2_PACKAGE_CONFIGTXT_DAC_OVERLAY)" >> $(BINARIES_DIR)/rpi-firmware/config.txt)
endef

CONFIGTXT_POST_INSTALL_TARGET_HOOKS += CONFIGTXT_REMOVESTUFF CONFIGTXT_BASE

ifeq ($(BR2_PACKAGE_CONFIGTXT_QUIET),y)
CONFIGTXT_POST_INSTALL_TARGET_HOOKS += CONFIGTXT_QUIET_INSTALL_TARGET_CMDS
endif

ifeq ($(BR2_PACKAGE_CONFIGTXT_EEPROM),y)
CONFIGTXT_POST_INSTALL_TARGET_HOOKS += CONFIGTXT_EEPROM_WORKAROUND
endif

CONFIGTXT_POST_INSTALL_TARGET_HOOKS += CONFIGTXT_ENABLE_EEPROM_I2C
CONFIGTXT_POST_INSTALL_TARGET_HOOKS += CONFIGTXT_DAC_OVERLAY

$(eval $(generic-package))
