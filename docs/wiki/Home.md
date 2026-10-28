# Lapin UX Wiki

**Lapin UX** is a Linux desktop environment written in Rust that feels like Windows 11, with no plugins and as little command-line use as possible. It is built with libcosmic/iced on top of the COSMIC compositor, Arch Linux first.

> Working name: Lapin UX (earlier "Lapin", originally "Atrium").
> Rule: match Windows 11's behaviour, layout and look. No Microsoft logos, Windows artwork or proprietary fonts. Open-source third-party assets are used under their own licences and credited.

Repository: https://github.com/znraznra/lapin-ux (branch `main`)

## Start here

| Page | What it covers |
|---|---|
| [Whitepaper](Whitepaper) | Why Lapin UX exists, how it is built, what has been learned |
| [Roadmap](Roadmap) | Every phase, status, what is left, suggested order |
| [Feature Set](Feature-Set) | Everything built, by area |
| [Architecture](Architecture) | Crates, forks, patches, pinned upstream commits |
| [Challenges](Challenges) | Roadblocks and how they were solved |
| [Real-Hardware Issues](Real-Hardware-Issues) | GitHub issues #1-#30 from Arch/CachyOS testing |
| [Coexistence and Uninstall](Coexistence-and-Uninstall) | Rules that keep other desktops (incl. real COSMIC) untouched |
| [Design Decisions](Design-Decisions) | Standing decisions from review rounds |
| [COSMIC Upgrades](COSMIC-Upgrades) | Moving from epoch-1.10.0 to 1.11.0 |
| [Building and Testing](Building-and-Testing) | Build, test harness, sandbox limits |
| [Review Process](Review-Process) | The screenshot gallery workflow and round history |
| [Credits and Licensing](Credits-and-Licensing) | Third-party assets |

## Status at a glance (2026-10-28, build `dca07a2`)

| Phase | Scope | Done | Partial | Not started |
|---|---|---|---|---|
| 1 Core desktop | Taskbar, Start, notifications, tray, wallpaper, theme | 12 | 0 | 0 |
| 2 Settings and multitasking | Settings app, quick settings, Alt+Tab/Task view, snapping | 9 | 0 | 0 |
| 3 System apps | File manager, Update, Control Panel | 4 | 1 | 1 |
| 4 Road to 1:1 Windows 11 | Everything else | 16 | 1 | 2 |

Open: File Explorer (fork of COSMIC Files), Store (fork of COSMIC Store), snip markup editor, file dialog navigation pane, snap-group shared-edge resize, #23/#24/#28.

## Ground rules for contributors

1. Never undo the coexistence and clean-uninstall fixes ([details](Coexistence-and-Uninstall)); read `CLAUDE.md` in the repo root.
2. Every fork keeps COSMIC's own behaviour outside a Lapin session.
3. Update docs after each piece of work: README, `docs/TECHNICAL.md`, this wiki.
