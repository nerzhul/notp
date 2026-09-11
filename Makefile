BINARY := notp
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
RELEASE_BIN := $(BUILD_DIR)/release/$(BINARY)
DEBUG_BIN := $(BUILD_DIR)/debug/$(BINARY)

INSTALL_BIN := $(DESTDIR)$(BINDIR)/$(BINARY)
INSTALL_DESKTOP := $(DESTDIR)$(APPLICATIONSDIR)/$(BINARY).desktop

.PHONY: all build build-release release debug clean install uninstall

all: build

build debug:
	$(CARGO) build $(CARGO_FLAGS)

build-release release:
	$(CARGO) build --release $(CARGO_FLAGS)

clean:
	$(CARGO) clean

install: build-release
	install -Dm0755 $(RELEASE_BIN) $(INSTALL_BIN)
	install -Dm0644 $(BINARY).desktop $(INSTALL_DESKTOP)

uninstall:
	rm -f $(INSTALL_BIN) $(INSTALL_DESKTOP)