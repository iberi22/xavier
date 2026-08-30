{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    # Rust toolchain: provided by the system (nixpkgs rustc was too old).
    # Only C/native deps come from nix here.

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

    # WebKit/Soup — needed for webkit2gtk-4.1 (Tauri 2 webview)
    libsoup_3.dev
    libsoup_3
    webkitgtk_4_1
    webkitgtk_4_1.dev

    # System tray / app indicator — needed for tray-icon feature
    libayatana-appindicator
    libayatana-appindicator.dev
    libnotify
    libnotify.dev

    # Tauri runtime extras
    librsvg
    librsvg.dev
    openssl.dev
    openssl

    # X11 libs (xorg scope in current nixpkgs)
    xorg.libxcb.dev
    xorg.libX11.dev
    xorg.libXrandr.dev
    xorg.libXrender.dev
    xorg.libXft.dev
    xorg.libXext.dev
    libxkbcommon
    xorg.xcbutil
    xorg.xcbutilwm
    xorg.xcbutilimage
    xorg.xcbutilrenderutil
    xorg.xcbutilcursor
    xorg.xcbutilkeysyms

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
    # --- NixOS tmpfs Mount Reference Configuration ---
    # To mount /build as a 16GB tmpfs ramdisk in NixOS, add the following to hardware-configuration.nix:
    # fileSystems."/build" = {
    #   device = "tmpfs";
    #   fsType = "tmpfs";
    #   options = [ "size=16G" "mode=755" "noswap" ];
    # };

    export LIBCLANG_PATH="${pkgs.llvmPackages.libclang}/lib"
    export PKG_CONFIG_PATH="${pkgs.gtk3.dev}/lib/pkgconfig:${pkgs.pango.dev}/lib/pkgconfig:${pkgs.cairo.dev}/lib/pkgconfig:${pkgs.gdk-pixbuf.dev}/lib/pkgconfig:${pkgs.libsoup_3.dev}/lib/pkgconfig:${pkgs.webkitgtk_4_1.dev}/lib/pkgconfig:${pkgs.libayatana-appindicator.dev}/lib/pkgconfig:${pkgs.libnotify.dev}/lib/pkgconfig:${pkgs.librsvg.dev}/lib/pkgconfig:${pkgs.atk.dev}/lib/pkgconfig:${pkgs.harfbuzz.dev}/lib/pkgconfig:${pkgs.freetype.dev}/lib/pkgconfig:${pkgs.fontconfig.dev}/lib/pkgconfig:${pkgs.xorg.libxcb.dev}/lib/pkgconfig:${pkgs.xorg.libX11.dev}/lib/pkgconfig:${pkgs.glib.dev}/lib/pkgconfig:${pkgs.libpng.dev}/lib/pkgconfig:${pkgs.openssl.dev}/lib/pkgconfig:${pkgs.sqlite.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
    export C_INCLUDE_PATH="${pkgs.glib.dev}/include:${pkgs.gtk3.dev}/include:${pkgs.pango.dev}/include:${pkgs.cairo.dev}/include:${pkgs.gdk-pixbuf.dev}/include:$C_INCLUDE_PATH"

    # Mold Linker configuration (optional/commented out due to Bus error in CI runners)
    # mold is extremely fast but may crash in some CI environments.
    # To enable:
    # export RUSTFLAGS="-C link-arg=-fuse-ld=mold $RUSTFLAGS"

    # Ensure RAM disk build directory exists and is writable, otherwise fall back to local target
    if mkdir -p /build/rust-target 2>/dev/null && [ -w /build/rust-target ]; then
        export CARGO_TARGET_DIR=/build/rust-target
        echo "✅ Nix shell ready — CARGO_TARGET_DIR=/build/rust-target (RAM tmpfs 16GB)"
    else
        export CARGO_TARGET_DIR="$(pwd)/target"
        echo "⚠️  Could not write to /build/rust-target. Falling back to local target compilation: $CARGO_TARGET_DIR"
    fi
    echo "✅ PKG_CONFIG_PATH configured for all system deps"

    # Auto-cleanup on shell exit: remove build artifacts to free RAM
    cleanup() {
        if [ "$CARGO_TARGET_DIR" = "/build/rust-target" ]; then
            echo "🧹 Cleaning up /build/rust-target..."
            rm -rf /build/rust-target 2>/dev/null && echo "✅ Build artifacts removed" || echo "⚠️ Could not clean (files may be in use)"
        fi
    }
    trap cleanup EXIT
  '';
}
