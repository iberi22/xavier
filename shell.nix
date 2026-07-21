{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    # Rust toolchain
    cargo
    rustc
    rustfmt
    clippy

    # GTK/GDK/Pango/Cairo — needed for gtk-rs, gdk-sys, pango-sys, cairo-sys
    gtk3.dev
    gtk3
    pango.dev
    pango
    cairo.dev
    cairo
    gdk-pixbuf.dev
    gdk-pixbuf
    atk.dev
    atk
    harfbuzz.dev
    harfbuzz
    freetype.dev
    freetype
    fontconfig.dev
    fontconfig
    pkg-config

    # WebKit/Soup — needed for webkit2gtk, libsoup
    libsoup_3.dev
    libsoup_3

    # X11 libs
    libxcb.dev
    libx11.dev
    libxrandr.dev
    libxrender.dev
    libxft.dev
    libxext.dev
    libxkbcommon
    xcbutil
    xcbutilwm
    xcbutilimage
    xcbutilrenderutil
    xcbutilcursor
    xcbutilkeysyms

    # Build tools
    cmake
    openssl.dev
    glib.dev
    glib
    libclang.dev
    clang
    llvmPackages.libclang

    # Needed for rust-analyzer / bindgen
    python3
    libffi.dev
    libffi

    # System libs
    zlib.dev
    bzip2.dev
    libxml2.dev
    sqlite.dev
    libpng.dev
    udev.dev
    libusb1
  ];

  shellHook = ''
    export CARGO_TARGET_DIR=/build/rust-target
    export LIBCLANG_PATH="${pkgs.llvmPackages.libclang}/lib"
    export PKG_CONFIG_PATH="${pkgs.gtk3.dev}/lib/pkgconfig:${pkgs.pango.dev}/lib/pkgconfig:${pkgs.cairo.dev}/lib/pkgconfig:${pkgs.gdk-pixbuf.dev}/lib/pkgconfig:${pkgs.libsoup_3.dev}/lib/pkgconfig:${pkgs.atk.dev}/lib/pkgconfig:${pkgs.harfbuzz.dev}/lib/pkgconfig:${pkgs.freetype.dev}/lib/pkgconfig:${pkgs.fontconfig.dev}/lib/pkgconfig:${pkgs.libxcb.dev}/lib/pkgconfig:${pkgs.libx11.dev}/lib/pkgconfig:${pkgs.glib.dev}/lib/pkgconfig:${pkgs.libpng.dev}/lib/pkgconfig:${pkgs.openssl.dev}/lib/pkgconfig:${pkgs.sqlite.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
    export C_INCLUDE_PATH="${pkgs.glib.dev}/include:${pkgs.gtk3.dev}/include:${pkgs.pango.dev}/include:${pkgs.cairo.dev}/include:${pkgs.gdk-pixbuf.dev}/include:$C_INCLUDE_PATH"

    # Ensure RAM disk build directory exists and is writable
    mkdir -p /build/rust-target
    echo "✅ Nix shell ready — CARGO_TARGET_DIR=/build/rust-target (RAM tmpfs 16GB)"
    echo "✅ PKG_CONFIG_PATH configured for all system deps"

    # Auto-cleanup on shell exit: remove build artifacts to free RAM
    cleanup() {
        echo "🧹 Cleaning up /build/rust-target..."
        rm -rf /build/rust-target 2>/dev/null && echo "✅ Build artifacts removed" || echo "⚠️ Could not clean (files may be in use)"
    }
    trap cleanup EXIT
  '';
}
