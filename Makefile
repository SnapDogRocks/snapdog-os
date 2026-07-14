BOARD ?= pi4
# Single source of truth: the release-please manifest (root package ".").
# CI overrides VERSION= explicitly; this is the default for local builds.
VERSION := $(shell jq -r '."."' .release-please-manifest.json 2>/dev/null || sed -n 's/.*"\.":[[:space:]]*"\([^"]*\)".*/\1/p' .release-please-manifest.json)
SNAPDOG_CTRL_BINARY ?= snapdog-ctrl-binary
SNAPDOG_ROOT_DEV ?= /dev/mmcblk0p
BRDIR := ../buildroot-$(BOARD)
BRSRC := ../buildroot

.PHONY: setup prepare-ctrl build config clean all

setup: ## Download and prepare buildroot
	@git config core.hooksPath .githooks
	@echo "Fetching buildroot 2025.02.15..."
	@if [ ! -d ../buildroot-src/.git ]; then \
		cd .. && git clone --depth 1 --branch 2025.02.15 https://github.com/buildroot/buildroot buildroot-src; \
	else \
		cd ../buildroot-src && git fetch --depth 1 origin tag 2025.02.15 && git checkout --detach 2025.02.15; \
	fi
	@rm -f ../buildroot && ln -s buildroot-src ../buildroot
	@buildroot/scripts/patch-buildroot ../buildroot

prepare-ctrl:
	@if [ ! -f "$(SNAPDOG_CTRL_BINARY)" ]; then \
		echo "Missing $(SNAPDOG_CTRL_BINARY). Build snapdog-ctrl for aarch64 first or pass SNAPDOG_CTRL_BINARY=/path/to/snapdog-ctrl."; \
		exit 1; \
	fi
	@mkdir -p $(BRDIR)/images
	@cp "$(SNAPDOG_CTRL_BINARY)" "$(BRDIR)/images/snapdog-ctrl"
	@chmod 755 "$(BRDIR)/images/snapdog-ctrl"

build: prepare-ctrl ## Build image for $(BOARD)
	@echo $(VERSION) > buildroot/VERSION
	@cd $(BRSRC) && make O=$(abspath $(BRDIR)) BR2_EXTERNAL=$(abspath buildroot) SNAPDOG_BOARD=$(BOARD) SNAPDOG_ROOT_DEV=$(SNAPDOG_ROOT_DEV) olddefconfig
	@cd $(BRSRC) && make O=$(abspath $(BRDIR)) BR2_EXTERNAL=$(abspath buildroot) SNAPDOG_BOARD=$(BOARD) SNAPDOG_ROOT_DEV=$(SNAPDOG_ROOT_DEV)

config: ## Configure for $(BOARD)
	@mkdir -p $(BRDIR)
	@if [ "$(BOARD)" = "pi5" ]; then cd $(BRSRC) && make raspberrypi5_defconfig; \
	elif [ "$(BOARD)" = "pi4" ]; then cd $(BRSRC) && make raspberrypi4_64_defconfig; \
	elif [ "$(BOARD)" = "pi3" ]; then cd $(BRSRC) && make raspberrypi3_64_defconfig; \
	elif [ "$(BOARD)" = "zero2w" ]; then cd $(BRSRC) && make raspberrypizero2w_64_defconfig; \
	else \
		if [ -f "$(BRSRC)/configs/$(BOARD)_defconfig" ]; then \
			cd $(BRSRC) && make $(BOARD)_defconfig; \
		else \
			echo "Unknown BOARD=$(BOARD) and no defconfig found in Buildroot."; \
			exit 1; \
		fi; \
	fi
	@mv $(BRSRC)/.config $(BRDIR)/.config
	@buildroot/scripts/apply-config-overrides \
		$(BRDIR)/.config buildroot/configs/override.conf BR2_PACKAGE_SNAPDOG_OS_ALL
	@if [ "$(BOARD)" = "pi3" ]; then \
		sed -i.bak 's|BR2_LINUX_KERNEL_INTREE_DTS_NAME=.*|BR2_LINUX_KERNEL_INTREE_DTS_NAME="broadcom/bcm2710-rpi-3-b broadcom/bcm2710-rpi-3-b-plus broadcom/bcm2710-rpi-cm3 broadcom/bcm2710-rpi-zero-2-w"|' $(BRDIR)/.config && rm -f $(BRDIR)/.config.bak; \
	fi

menuconfig: ## Open menuconfig for $(BOARD)
	@cd $(BRSRC) && make O=$(abspath $(BRDIR)) BR2_EXTERNAL=$(abspath buildroot) menuconfig

clean: ## Clean build output for $(BOARD)
	rm -rf $(BRDIR)

all: ## Build all Pi variants
	@$(MAKE) BOARD=pi3 config build
	@$(MAKE) BOARD=pi4 config build
	@$(MAKE) BOARD=pi5 config build

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-15s\033[0m %s\n", $$1, $$2}'

.DEFAULT_GOAL := help
