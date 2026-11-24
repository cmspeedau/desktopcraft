# DesktopCraft

**Make your Windows desktop a workspace that works for you.**

[![Latest release](https://img.shields.io/github/v/release/tanyapruck/DesktopCraft?label=release)](https://github.com/tanyapruck/DesktopCraft/releases/latest)
[![Windows x64](https://img.shields.io/badge/platform-Windows%20x64-0078D4)](https://github.com/tanyapruck/DesktopCraft/releases/latest)
[![Free to use](https://img.shields.io/badge/price-Free-2ea44f)](https://github.com/tanyapruck/DesktopCraft/releases/latest)

DesktopCraft is a free Windows desktop organization and productivity app designed to transform cluttered desktops into clean, customizable workspaces. Organize files, folders, and shortcuts into Boxes, switch between Tabs and Pages, and keep the familiar Windows desktop experience.

## Download

**Latest public release: v0.3.0**

[**Download DesktopCraft for Windows (x64)**](https://github.com/tanyapruck/DesktopCraft/releases/download/v0.3.0/DesktopCraft-v0.3.0-Setup-x64.exe)

[Release notes](https://github.com/tanyapruck/DesktopCraft/releases/tag/v0.3.0) · [All releases](https://github.com/tanyapruck/DesktopCraft/releases) · [SHA-256 checksums](https://github.com/tanyapruck/DesktopCraft/releases/download/v0.3.0/SHA256SUMS.txt)

## Features

| Feature | What you can do |
| --- | --- |
| Desktop Boxes | Create, move, resize and customize containers for desktop icons. |
| Tabs | Keep related items together in one Box and switch between categories. |
| Pages | Create separate desktop layouts for work, personal use or different projects. |
| Native Windows integration | Use Windows context menus, Rename (F2), Properties (Alt+Enter) and familiar Shell actions. |
| Drag and drop | Move and select items using familiar desktop interactions, including between Boxes and Tabs. |
| Linked folders | Display the contents of real Windows folders within a Box. |
| Sorting rules | Organize items by configurable rules and apply sorting when needed. |
| Docking and roll-up | Attach Boxes to screen edges or collapse them to save desktop space. |
| Quick Hide | Double-click an empty desktop area to hide or show your workspace. |
| Appearance | Customize Box colors, transparency, icon sizes and layout. |
| Multi-monitor support | Arrange Boxes across displays and select the display for the Page Switcher. |
| Workspace persistence | Save desktop arrangements, rules and preferences between sessions. |

## What's new in v0.3.0

- Refactored desktop integration using native Windows Explorer icons and context menus.
- Improved drag and drop and everyday desktop interactions.
- Improved responsiveness and crash recovery.
- Improved Page Switcher display handling and multi-monitor behavior.

See the [v0.3.0 release notes](https://github.com/tanyapruck/DesktopCraft/releases/tag/v0.3.0) for the published release details.

## Getting started

DesktopCraft is distributed as a Windows x64 installer and installs for your Windows account without requiring administrator privileges.

1. Download the setup executable from the official [GitHub Releases](https://github.com/tanyapruck/DesktopCraft/releases/latest) page.
2. Run the installer and choose your preferred startup settings.
3. Create Boxes, organize your desktop icons, and add Tabs or Pages as needed.
4. Open Settings to customize appearance, behavior, display selection and shortcuts.

> **Security notice:** The current installer is unsigned. Windows SmartScreen may display a publisher or reputation warning. Download only from this repository, verify the published SHA-256 checksum, and keep your security software enabled.

To verify the installer in PowerShell:

```powershell
Get-FileHash .\DesktopCraft-v0.3.0-Setup-x64.exe -Algorithm SHA256
```

Compare the output with the published [SHA256SUMS.txt](https://github.com/tanyapruck/DesktopCraft/releases/download/v0.3.0/SHA256SUMS.txt).

## How files are handled

| Box type | File behavior |
| --- | --- |
| Regular Box | Organizes desktop items without requiring a separate physical folder for each Box. |
| Linked folder | Displays a real folder; dragging items into or out of it may move actual files. |

Windows file operations such as Rename and Delete affect real files. Removing an item from an organizational group is not the same as deleting it from disk. Keep backups of important files and workspace settings before changing installations.

## Updates and development

DesktopCraft can check GitHub Releases for newer versions. Updates are user-approved, rather than silently installed.

**Coming next — v0.3.1 (in development):** further responsiveness improvements, refinements to native desktop interactions, multi-monitor handling and installer reliability. This version is still undergoing verification and is not yet available for download.

### Future direction

DesktopCraft aims to grow into a modular desktop productivity suite, with desktop organization as its foundation. Planned ideas include screenshot and capture tools, desktop widgets and productivity dashboards, alongside other useful everyday desktop utilities.

Future modules are not part of the current installer.

## Feedback and support

Found a bug or have a feature suggestion? [Open an issue](https://github.com/tanyapruck/DesktopCraft/issues). For bug reports, include your DesktopCraft version, Windows version, reproduction steps and expected behavior. Remove personal information from any logs or screenshots before sharing.

## Distribution and third-party notices

This repository is the official public download and feedback page. Third-party components retain their applicable licenses and notices; see the [third-party notices for v0.3.0](https://github.com/tanyapruck/DesktopCraft/releases/download/v0.3.0/THIRD-PARTY-NOTICES.md).

---

**Organize your desktop. Make room for your work.**

[Download DesktopCraft](https://github.com/tanyapruck/DesktopCraft/releases/latest) · [Report an issue](https://github.com/tanyapruck/DesktopCraft/issues)
