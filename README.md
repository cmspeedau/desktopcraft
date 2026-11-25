<h1 align="center">DesktopCraft</h1>

<p align="center"><strong>Your desktop, crafted for productivity.</strong></p>

<p align="center">
  <a href="https://github.com/tanyapruck/DesktopCraft/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/tanyapruck/DesktopCraft?style=flat-square&amp;label=Release"></a>
  <img alt="Windows x64" src="https://img.shields.io/badge/Platform-Windows%20x64-0078D4?style=flat-square">
  <img alt="Free to use" src="https://img.shields.io/badge/Price-Free-2ea44f?style=flat-square">
</p>

<p align="center">
  <a href="https://github.com/tanyapruck/DesktopCraft/releases/download/v0.3.1/DesktopCraft-v0.3.1-Setup-x64.exe"><strong>Download DesktopCraft</strong></a> ·
  <a href="https://github.com/tanyapruck/DesktopCraft/releases">GitHub releases</a> ·
  <a href="https://github.com/tanyapruck/DesktopCraft/issues">Feedback</a>
</p>

DesktopCraft is a free Windows desktop organization and productivity app designed to transform cluttered desktops into clean, customizable workspaces. Build a desktop around the way you work, with Boxes, Tabs, Pages, Drawers and Portals, while keeping familiar Windows interactions.

Our goal is to bring everyday desktop productivity tools together in one modular workspace, for people and AI assistants alike.

## Craft your workspace

Keep the things you need within reach, without filling your desktop with folders and shortcuts.

| Feature | What you can do |
| --- | --- |
| **Boxes** | Create customizable containers for your desktop icons. Move, resize and arrange them around your workspace. |
| **Tabs** | Organize related categories inside a single Box and switch between them without taking up extra space. |
| **Pages** | Keep separate desktop workspaces for different projects, activities or workflows. |
| **Drawers** | Turn screen-edge Boxes into space-saving, expandable areas for frequently used items. |
| **Roll-up** | Collapse a Box into its title and reveal it when needed. |
| **Portals** | Bring a real Windows folder into a Box and access its contents directly from the desktop. |
| **Sorting Rules** | Organize icons with configurable rules and run Sort Now when you want to apply them. |
| **Saved Layouts** | Save a workspace arrangement and restore it later. |

## Make the desktop your own

| Feature | What you can do |
| --- | --- |
| **Quick Hide** | Hide or reveal the desktop workspace with a double-click on an empty desktop area. |
| **Peek** | Bring Boxes into view above other windows using a configurable keyboard shortcut. |
| **Appearance** | Personalize colors, transparency, icon sizes and Box layouts. |
| **Multi-monitor** | Arrange Boxes across displays and choose where the Page Switcher appears. |
| **Native Windows experience** | Use Explorer icons, context menus, drag and drop, multi-selection, F2 Rename and Alt+Enter Properties. |

## AI Tools & Automation

DesktopCraft is designed to be more than a graphical desktop organizer. Its local AI integration lets compatible AI agents inspect and organize a workspace using structured tools, while keeping the user in control.

DesktopCraft v0.3.1 includes 28 local AI tools, available through MCP and JSON CLI. Workspace mutations use Native Transactions with Explorer acknowledgement, explicit user approval and verified persistence.

| Tool category | Capabilities |
| --- | --- |
| **Workspace discovery** | Inspect workspace status, Pages, Boxes, Tabs, virtual pins, linked folder paths, rules and saved layouts. |
| **Workspace organization** | Create or rename Boxes and Pages, switch Pages or Tabs, merge Boxes, and detach Tabs. |
| **Virtual pin management** | Pin, move, batch-move or unpin items without moving their original files. |
| **Sorting automation** | Preview sorting rules before applying them, or add new rules. |
| **Portals and layouts** | Link or unlink folders, save a named layout and undo supported workspace changes. |

### Connect your AI assistant

The local integration exposes a structured JSON CLI and a Model Context Protocol (MCP) stdio server. Compatible MCP clients can launch the installed DesktopCraft executable with the `--mcp` argument while the desktop app is running.

Discover the tools and inspect workspace status from PowerShell:

```powershell
$desktopcraft = "$env:LOCALAPPDATA\Programs\DesktopCraft\DesktopCraft.exe"
& $desktopcraft --tools-list
& $desktopcraft --tool desktopcraft_status '{}'
```

Read-only requests can inspect workspace metadata. Requests that change the workspace require user approval through DesktopCraft. Virtual organization tools do not move or delete the original files, and the tools do not read file contents. Linked-folder paths and optional pin paths may be exposed when requested.

AI integration is local; a compatible AI client is separate from DesktopCraft. No built-in AI model or hosted AI service is included.

## What's next

DesktopCraft is the first part of a broader desktop productivity toolkit. The aim is to build useful, independent modules that work together rather than crowding everything into one feature.

| Module | Direction | Status |
| --- | --- | --- |
| **DesktopCraft** | Desktop organization, native desktop interaction and workspace automation. | Available |
| **SnapGrab** | Screenshot capture and editing tools for everyday work. | In development |
| **Desktop Widgets** | At-a-glance information and useful desktop utilities. | Planned |
| **Productivity Dashboard** | A central place to access productivity modules and workflows. | Planned |

Future modules are not included in the current installer.

## Download & install

**Latest public release: v0.3.1** · Windows x64

[**Download DesktopCraft v0.3.1**](https://github.com/tanyapruck/DesktopCraft/releases/download/v0.3.1/DesktopCraft-v0.3.1-Setup-x64.exe) · [Release notes](https://github.com/tanyapruck/DesktopCraft/releases/tag/v0.3.1) · [SHA-256 checksums](https://github.com/tanyapruck/DesktopCraft/releases/download/v0.3.1/SHA256SUMS.txt)

Install for your Windows account without administrator privileges. Choose your startup preferences and start organizing your workspace.

> The installer is currently unsigned, so Windows SmartScreen may display a publisher or reputation warning. Download from the official releases page, verify the published checksum, and keep security software enabled.

DesktopCraft checks for updates and asks before installing a new version. Version 0.3.1 improves native AI tool compatibility, preserves existing workspaces when reinstalling, and improves uninstaller cleanup reliability. See the [release notes](https://github.com/tanyapruck/DesktopCraft/releases/tag/v0.3.1) for details.

<details>
<summary><strong>Verify the download and understand file operations</strong></summary>

```powershell
Get-FileHash .\DesktopCraft-v0.3.1-Setup-x64.exe -Algorithm SHA256
```

Compare the hash with the published [SHA256SUMS.txt](https://github.com/tanyapruck/DesktopCraft/releases/download/v0.3.1/SHA256SUMS.txt).

Regular Boxes organize desktop items without needing a physical folder for every Box. Portals display real folders; moving items into or out of them may move actual files. Windows Rename and Delete are real filesystem actions. Keep backups of important files and workspace settings.

</details>

## Build from source

The sanitized v0.3.1 source snapshot is available at [v0.3.1-source](https://github.com/tanyapruck/DesktopCraft/tree/v0.3.1-source). The existing v0.3.1 installer release remains unchanged.

Requirements: Windows x64, stable Rust with the x86_64-pc-windows-msvc target, Visual Studio C++ Build Tools and Windows SDK. From a clean Git checkout in PowerShell:

```powershell
$manifest = ./tools/build-native-candidate.ps1
$build = Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json
$env:DESKTOPCRAFT_NATIVE_FILTER_DLL = $build.artifact.native_dll
cargo test --locked --features native-fast-tabs
cargo test --locked --manifest-path native-filter/Cargo.toml --features native-fast-tabs
```

The build script compiles and verifies the matching native DLL and executable. Close any running DesktopCraft instance normally before launching a local build. Do not load the raw DLL directly into Explorer. Source builds are not claimed to be byte-identical to the published installer.

## Help shape DesktopCraft

Found a bug, have an idea, or want to suggest the next productivity tool? [Open an issue](https://github.com/tanyapruck/DesktopCraft/issues).

When reporting a bug, include your DesktopCraft version, Windows version, reproduction steps and expected behavior. Remove personal information from screenshots and logs before sharing.

This repository contains the public source snapshot, official downloads and feedback. The project source is distributed under the [MIT License](LICENSE); artwork and third-party components retain their separate terms. See [NOTICE.md](NOTICE.md), [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) and [assets/README.md](assets/README.md).

<p align="center"><strong>Less clutter. More room to work.</strong></p>
