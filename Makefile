BINARIES := notp-cli notp-gui
APP_ID := com.nerzhul.notp
VERSION := $(shell grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)

PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
DATADIR ?= $(PREFIX)/share
APPLICATIONSDIR ?= $(DATADIR)/applications
ICONDIR ?= $(DATADIR)/icons/hicolor

CARGO ?= cargo
DESTDIR ?= 
FEATURES ?= camera

CARGO_FLAGS ?= $(if $(FEATURES),--features $(FEATURES),)

BUILD_DIR := target
RELEASE_BIN := $(BUILD_DIR)/release
DEBUG_BIN := $(BUILD_DIR)/debug

INSTALL_BIN := $(patsubst %,$(DESTDIR)$(BINDIR)/%,$(BINARIES))
INSTALL_DESKTOP := $(DESTDIR)$(APPLICATIONSDIR)/notp.desktop

.PHONY: all build build-release release debug clean install uninstall

all: build

build debug:
	$(CARGO) build $(CARGO_FLAGS)

build-release release:
	$(CARGO) build --release $(CARGO_FLAGS)

clean:
	$(CARGO) clean

install: build-release
	$(foreach bin,$(BINARIES),$(call install_bin,$(bin)))
	install -Dm0644 notp.desktop $(INSTALL_DESKTOP)

uninstall:
	$(foreach bin,$(BINARIES),rm -f $(DESTDIR)$(BINDIR)/$(bin))
	rm -f $(INSTALL_DESKTOP)

define install_bin
	install -Dm0755 $(RELEASE_BIN)/$(1) $(DESTDIR)$(BINDIR)/$(1)
endef
