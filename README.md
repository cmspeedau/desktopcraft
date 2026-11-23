# DesktopCraft

**DesktopCraft** is a Windows desktop organization and productivity application that helps transform cluttered desktops into clean, customizable workspaces.

This repository is the **official public download and release hub** for DesktopCraft.

## Download

Download the latest available installer from [GitHub Releases](https://github.com/tanyapruck/DesktopCraft/releases).

> Releases are being migrated to this repository. If no download is listed yet, the migration has not been completed.

## Installation

1. Open the [Releases page](https://github.com/tanyapruck/DesktopCraft/releases).
2. Download the Windows x64 installer for the desired version.
3. Follow the setup instructions.

## Release verification

When a release includes `SHA256SUMS.txt`, verify the installer before running it:

```powershell
Get-FileHash .\DesktopCraft-v0.2.0-Setup-x64.exe -Algorithm SHA256
```

Compare the output with the matching hash in the release's `SHA256SUMS.txt`.

## About this repository

This public repository is for **installers, release notes, and download information**. Application source code and development history are maintained privately; they are **not distributed through this repository**.

## Updates

For release announcements and downloads, visit [GitHub Releases](https://github.com/tanyapruck/DesktopCraft/releases).

## License

This public repository does not grant a source-code license for the DesktopCraft application. Check the license and notices accompanying each distributed release for applicable terms.
