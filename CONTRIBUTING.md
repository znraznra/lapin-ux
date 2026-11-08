# Contributing to Lapin UX

Thanks for wanting to help. Lapin UX is a desktop for people who just want things to work, and you don't have to write code to make it better. Testing it and telling us what broke is just as valuable.

Please read the [Code of Conduct](CODE_OF_CONDUCT.md) first. Security problems follow the [Security Policy](SECURITY.md), not a public issue.

## Ways to help

- **Test it on your own computer** and report what doesn't work. This is the most useful thing right now, because a lot of it can only be checked on real hardware: lock screen, sign-in, multiple monitors, touchpads, graphics cards, sound.
- **Fix or improve the docs**: the README, the [wiki](https://github.com/znraznra/lapin-ux/wiki) pages, the test checklist.
- **Pick up a bug or a feature** from the [issue list](https://github.com/znraznra/lapin-ux/issues).
- **Help others** by answering questions in [Discussions](https://github.com/znraznra/lapin-ux/discussions).

## Reporting a bug

1. Check the [open issues](https://github.com/znraznra/lapin-ux/issues) first. If it's there, add what you know to it.
2. Otherwise open a new issue with the **Bug report** template.
3. Tell us how you ran it. [TESTING.md](TESTING.md) describes two ways: Stage A, in a window, with nothing installed, and Stage B, installed and signed into as a real session.
4. Attach the log the template asks for. It saves a lot of guessing.

If you're not sure whether it's a bug, ask in Discussions.

## Suggesting a feature

Open an issue with the **Feature request** template. Say what you were trying to do and where it got in the way, not only the feature you'd like. A few things are left out on purpose, so check the "Not planned" part of the [roadmap](docs/ROADMAP.md) first.

For anything big, please talk to us in an issue before you spend time on it.

## Sending a change

You'll need Arch Linux (or an Arch-based system), Rust, and a little patience: the first build takes 10 to 20 minutes.

1. Fork the repository and make a branch.
2. Run `sh tools/preflight.sh`. It only reads your system and lists anything you're missing.
3. Build with `cargo build` and try your change (Stage A in [TESTING.md](TESTING.md) runs it in a window without installing anything).
4. Open a pull request and fill in the template.

### Rules that protect other people's computers

These come from real bugs. Please read [CLAUDE.md](CLAUDE.md) in full before changing anything in the areas below. It is written for both people and AI assistants.

- **Lapin UX must never change other desktops.** Its look, font and settings apply only inside the Lapin UX session. If your code writes a COSMIC or GTK setting that other desktops share, it must also be added to `items()` in `tools/lapin-ux-uninstall`.
- **Every change to a COSMIC program is a patch file** in `packaging/lapin-ux-*/`, applied to a pinned upstream commit. Never regenerate a patch from an old checkout: rebuild the branch from the patches first, make your change, then check that `git diff packaging/` shows only what you meant to change.
- **Lapin looks inside COSMIC programs only apply in a Lapin session**, so those programs still behave like stock COSMIC everywhere else.
- **Uninstalling must stay clean.** It must never leave someone without a working login screen.

### Before you open the pull request

- `bash tools/test-uninstall/run.sh "$PWD/tools/lapin-ux-uninstall"` passes (needs `dconf`).
- `shellcheck -S warning tools/lapin-ux-uninstall` is quiet.
- If you touched a COSMIC patch, `cargo check --locked` passes at the pinned commit with all patches applied.
- You tried the change, and you say how in the pull request.
- Screenshots are attached for anything people can see.
- The docs are updated: the README if it's something users notice, and `docs/TECHNICAL.md` for the details. Wiki pages live in `docs/wiki/` and are published from there automatically, so edit them there, not in GitHub's wiki editor.

### Words matter

Lapin UX is described as a desktop you already know how to use, with a zero learning curve. In the README, docs, package descriptions, on-screen text and commit messages, describe what the thing does and don't compare it to another operating system or its version names. Call the key the **Super** key. [CLAUDE.md](CLAUDE.md) lists the few exceptions, such as third-party credits.

### Commits and issues

- Write the commit message in plain words: what changed and why.
- Refer to the issue with `Refs #123`, not `Fixes #123`. Bugs here are closed only after someone confirms the fix on real hardware.
- Keep one change per pull request where you can. Small ones are reviewed faster.

## Licence

Lapin UX's own code is GPL-3.0-only. By sending a change you agree that it is released under that licence. Third-party assets keep their own licences, listed in the README. Please don't add anything you don't have the right to share, and don't add Microsoft logos, artwork or proprietary fonts.

## Questions

Ask in [Discussions](https://github.com/znraznra/lapin-ux/discussions). No question is too basic.
