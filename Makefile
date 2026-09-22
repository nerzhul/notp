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

ANDROID_DIR := android
ANDROID_APK_DIR := $(ANDROID_DIR)/app/build/outputs/apk/release
ANDROID_APK := $(ANDROID_APK_DIR)/app-release.apk
ANDROID_APK_DEST := $(BUILD_DIR)/notp-$(VERSION)-android.apk

GRADLE ?= ./gradlew
NDK_VERSION ?= 26.1.10909125
ANDROID_ABIS ?= arm64-v8a armeabi-v7a x86_64 x86
ANDROID_API ?= 26

.PHONY: all build build-release release debug clean install uninstall \
        test test-android rust-test rust-build \
        android-build android-release android-clean android-install android-uninstall

all: build

build debug:
	$(CARGO) build $(CARGO_FLAGS)

build-release release:
	$(CARGO) build --release $(CARGO_FLAGS)

rust-test:
	$(CARGO) test --workspace

rust-build:
	$(CARGO) build --workspace --release

# Android: build the UniFFI native library for every ABI via cargo-ndk,
# generate Kotlin scaffolding, then assemble a debug APK with Gradle.
android-build:
	cd $(ANDROID_DIR) && $(GRADLE) :app:assembleDebug --no-daemon

# Android: same, with the release variant. Requires the keystore
# environment variables documented in .github/workflows/release.yml.
android-release:
	cd $(ANDROID_DIR) && $(GRADLE) :app:assembleRelease --no-daemon

# Convenience target that only rebuilds the Rust .so files without invoking
# Gradle. Useful when iterating on the core crate.
android-rust:
	cargo ndk \
		--target $(firstword $(ANDROID_ABIS)) \
		--platform android-$(ANDROID_API) \
		-o $(ANDROID_DIR)/app/src/main/jniLibs \
		build -p notp-android --release --locked

# Compile every ABI into the Android jniLibs directory. The Gradle build
# runs cargo-ndk itself; this target exists for quick local iteration.
android-rust-all:
	@for abi in $(ANDROID_ABIS); do \
		echo "Building notp-android for $$abi"; \
		cargo ndk \
			--target $$abi \
			--platform android-$(ANDROID_API) \
			-o $(ANDROID_DIR)/app/src/main/jniLibs \
			build -p notp-android --release --locked; \
	done

# Copy the freshly-built APK out of the Gradle output tree so it sits next
# to the desktop tarballs under target/.
android-apk:
	@mkdir -p $(BUILD_DIR)
	@test -f $(ANDROID_APK) || (echo "APK not found at $(ANDROID_APK); run 'make android-release' first" && exit 1)
	cp $(ANDROID_APK) $(ANDROID_APK_DEST)
	@echo "APK staged at $(ANDROID_APK_DEST)"

android-clean:
	cd $(ANDROID_DIR) && $(GRADLE) clean --no-daemon
	rm -rf $(ANDROID_DIR)/build $(ANDROID_DIR)/app/build $(ANDROID_DIR)/.gradle
	rm -rf $(ANDROID_DIR)/app/src/main/jniLibs
	rm -f $(ANDROID_APK_DEST)

# Install / uninstall the APK via adb on the connected device.
android-install: android-release
	adb install -r $(ANDROID_APK)

android-uninstall:
	adb uninstall $(APP_ID)

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