# Xavier Windows Installer

This directory contains the source files for creating a professional Windows installer for Xavier.

## Prerequisites

To build the installer, you need one of the following:

- **WiX Toolset v3.11+**: [Download WiX](https://wixtoolset.org/releases/)
- **Inno Setup v6+**: [Download Inno Setup](https://jrsoftware.org/isdl.php)

Additionally, you need:
- **Rust**: To build the Xavier binaries.
- **Node.js**: To build the Panel UI.

## Build Instructions

1.  Open PowerShell as Administrator (if needed for tool access).
2.  Navigate to the `installer` directory:
    ```powershell
    cd installer
    ```
3.  Run the build script:
    ```powershell
    .\build-installer.ps1
    ```

The script will:
- Verify that the Xavier binaries (`xavier.exe`, `xavier-tui.exe`, `xavier-gui.exe`) are built.
- Verify that the Panel UI is built in `panel-ui/build`.
- Detect if `candle.exe` (WiX) or `iscc.exe` (Inno Setup) is in your PATH.
- Compile the installer.

## Output

- **WiX**: Generates `XavierInstaller.msi` in the `installer` directory.
- **Inno Setup**: Generates `XavierSetup.exe` in the `installer/Output` directory.

## Installer Features

- Installs to `Program Files\SouthWest AI Labs\Xavier`.
- Adds Xavier to the system `PATH`.
- Creates Start Menu shortcuts for both the GUI and TUI versions.
- Bundles the Panel UI for the web dashboard.
