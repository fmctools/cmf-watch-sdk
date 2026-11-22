# Wrappers around scripts/ and the CLI. Everything targets the emulator.
# ponytail: phony targets only, cargo and the scripts already skip unchanged work.

CMF_WATCH_UTILS ?= ../cmf-watch-utils
STOCK_NOR ?= $(CMF_WATCH_UTILS)/fixtures/nor.bin
NOR ?= target/nor-wp2.bin
APPS ?= hello vibration-lab motor-hog motor-hold agent-glow api-demo herdr-agents ui-gallery kit-gallery lv-demo
E2E_APPS := hello vibration-lab motor-hog motor-hold
VERSION ?= 1
HCI_PORT ?=
CLI := uv run --locked wp2apps

.PHONY: help fw fw-ota fw-ota-wp3 fw-motor apps nor emu test e2e install list remove clean

help:
	@echo 'fw          stub firmware and loader: target/app-wp2.bin, target/loader.wp2app'
	@echo 'fw-ota      fw + stub as a full firmware image: target/fw-wp2.bin (VERSION_CODE=0x10003)'
	@echo 'fw-ota-wp3  Watch 3 Pro 1.0.1.77 with the watch face guard only: target/fw-wp3.bin'
	@echo 'fw-motor    stock firmware image with only the motor patch: target/fw-motor.bin (MOTOR=350)'
	@echo 'apps        all example apps, or one with app-<name> (VERSION=$(VERSION))'
	@echo 'nor         $(NOR) from $(STOCK_NOR) with the patched app.bin'
	@echo 'emu         boot $(NOR) in the emulator, extra args in EMU_ARGS'
	@echo 'test        unit tests: Python, SDK, wp2-core'
	@echo 'e2e         emulator tests, about half an hour'
	@echo 'install     APP=<name|loader> HCI_PORT=P [SLOT=N]'
	@echo 'list        HCI_PORT=P'
	@echo 'remove      SLOT=N HCI_PORT=P'
	@echo 'clean       remove target/'

fw:
	nix-shell -p llvm --run 'scripts/build-fw'

fw-ota: fw
	rm -f target/fw-wp2.bin
	$(CLI) ota-image --app target/app-wp2.bin --output target/fw-wp2.bin $(if $(VERSION_CODE),--version-code $(VERSION_CODE))

fw-ota-wp3: fw
	rm -f target/fw-wp3.bin
	$(CLI) ota-image --model wp3 --app target/app-wp3.bin --output target/fw-wp3.bin $(if $(VERSION_CODE),--version-code $(VERSION_CODE))

fw-motor:
	rm -f target/app-motor.bin target/fw-motor.bin
	$(CLI) firmware --motor-only --motor $(or $(MOTOR),350) --output target/app-motor.bin
	$(CLI) ota-image --app target/app-motor.bin --output target/fw-motor.bin

app-%:
	nix-shell -p llvm --run 'scripts/build-app $* $(VERSION)'

apps: $(addprefix app-,$(APPS))

nor: fw
	$(CLI) nor $(STOCK_NOR) --app target/app-wp2.bin --output $(NOR)

emu: nor
	scripts/emu --nor $(NOR) $(EMU_ARGS)

test:
	uv run --locked python -m unittest discover -s tests -v
	cd sdk && cargo test --target aarch64-apple-darwin
	cd fw && cargo test -p wp2-core --target aarch64-apple-darwin

e2e: fw $(addprefix app-,$(E2E_APPS))
	WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e -v'

install:
	$(CLI) install target/$(APP).wp2app --hci-port $(HCI_PORT) $(if $(SLOT),--slot $(SLOT))

list:
	$(CLI) list --hci-port $(HCI_PORT)

remove:
	$(CLI) remove --slot $(SLOT) --hci-port $(HCI_PORT)

clean:
	rm -rf target
