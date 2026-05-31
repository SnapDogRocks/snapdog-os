PI ?= pi4
VERSION := $(shell cat VERSION)
SNAPDOG_CTRL_BINARY ?= snapdog-ctrl-binary
SNAPDOG_ROOT_DEV ?= /dev/mmcblk0p
BRDIR := ../buildroot-$(PI)
BRSRC := ../buildroot

.PHONY: setup prepare-ctrl build config clean all

setup: ## Download and prepare buildroot
	@git config core.hooksPath .githooks
	@echo "Fetching buildroot 2025.02..."
	@if [ ! -d ../buildroot-src/.git ]; then \
		cd .. && git clone --depth 1 --branch 2025.02 https://github.com/buildroot/buildroot buildroot-src; \
	else \
		cd ../buildroot-src && git fetch --depth 1 origin tag 2025.02 && git checkout 2025.02; \
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

build: prepare-ctrl ## Build image for $(PI)
	@echo $(VERSION) > buildroot/VERSION
	@cd $(BRSRC) && make O=$(abspath $(BRDIR)) BR2_EXTERNAL=$(abspath buildroot) SNAPDOG_PI_VERSION=$(subst pi,,$(PI)) SNAPDOG_ROOT_DEV=$(SNAPDOG_ROOT_DEV) olddefconfig
	@cd $(BRSRC) && make O=$(abspath $(BRDIR)) BR2_EXTERNAL=$(abspath buildroot) SNAPDOG_PI_VERSION=$(subst pi,,$(PI)) SNAPDOG_ROOT_DEV=$(SNAPDOG_ROOT_DEV)

config: ## Configure for $(PI)
	@mkdir -p $(BRDIR)
	@if [ "$(PI)" = "pi5" ]; then cd $(BRSRC) && make raspberrypi5_defconfig; \
	elif [ "$(PI)" = "pi4" ]; then cd $(BRSRC) && make raspberrypi4_64_defconfig; \
	elif [ "$(PI)" = "pi3" ]; then cd $(BRSRC) && make raspberrypi3_64_defconfig; \
	elif [ "$(PI)" = "zero2w" ]; then cd $(BRSRC) && make raspberrypizero2w_defconfig; \
	else echo "Use PI=pi3|pi4|pi5|zero2w"; exit 1; fi
	@mv $(BRSRC)/.config $(BRDIR)/.config
	@buildroot/scripts/apply-config-overrides \
		$(BRDIR)/.config buildroot/configs/override.conf BR2_PACKAGE_SNAPDOG_OS_ALL

menuconfig: ## Open menuconfig for $(PI)
	@cd $(BRSRC) && make O=$(abspath $(BRDIR)) BR2_EXTERNAL=$(abspath buildroot) menuconfig

clean: ## Clean build output for $(PI)
	rm -rf $(BRDIR)

all: ## Build all Pi variants
	@$(MAKE) PI=pi3 config build
	@$(MAKE) PI=pi4 config build
	@$(MAKE) PI=pi5 config build
	@$(MAKE) PI=zero2w config build

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-15s\033[0m %s\n", $$1, $$2}'

.DEFAULT_GOAL := help
