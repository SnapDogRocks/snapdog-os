PI ?= pi4
VERSION := $(shell cat VERSION)
BRDIR := ../buildroot-$(PI)
BRSRC := ../buildroot

.PHONY: setup build config clean all

setup: ## Download and prepare buildroot
	@echo "Fetching buildroot 2025.02..."
	@cd .. && git clone https://github.com/buildroot/buildroot buildroot-src || true
	@cd ../buildroot-src && git checkout 2025.02
	@rm -f ../buildroot && ln -s buildroot-src ../buildroot
	@if [ -f $(BRSRC)/board/raspberrypi/genimage.cfg.in ]; then \
		sed -i 's/32M/256M/g' $(BRSRC)/board/raspberrypi/genimage.cfg.in; \
	fi

build: ## Build image for $(PI)
	@mkdir -p $(BRDIR)/target/etc
	@echo $(subst pi,,$(PI)) > $(BRDIR)/target/etc/raspberrypi.version
	@echo $(VERSION) > buildroot/VERSION
	@cd $(BRSRC) && make O=$(abspath $(BRDIR)) BR2_EXTERNAL=$(abspath buildroot) olddefconfig
	@cd $(BRSRC) && make O=$(abspath $(BRDIR)) BR2_EXTERNAL=$(abspath buildroot)

config: ## Configure for $(PI)
	@mkdir -p $(BRDIR)
	@if [ "$(PI)" = "pi5" ]; then cd $(BRSRC) && make raspberrypi5_defconfig; \
	elif [ "$(PI)" = "pi4" ]; then cd $(BRSRC) && make raspberrypi4_64_defconfig; \
	elif [ "$(PI)" = "pi3" ]; then cd $(BRSRC) && make raspberrypi3_64_defconfig; \
	else echo "Use PI=pi3|pi4|pi5"; exit 1; fi
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

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-15s\033[0m %s\n", $$1, $$2}'

.DEFAULT_GOAL := help
