# Android APK Build Makefile for Tauri Wrapper

.PHONY: all build build-debug build-release clean assembleRelease assembleDebug

all: build-release

build: build-release

build-debug: assembleDebug

build-release: assembleRelease

assembleDebug:
	./gradlew assembleDebug

assembleRelease:
	./gradlew assembleRelease

clean:
	./gradlew clean
