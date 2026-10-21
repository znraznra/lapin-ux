#!/usr/bin/env bash
# Removes Lapin UX and puts your desktop settings back the way they were.
#   ./uninstall.sh                  everything (asks before each step)
#   ./uninstall.sh --dry-run        only show what it would do
#   ./uninstall.sh --settings-only  only restore your settings (if you already
#                                   removed the packages with pacman)
# Same as the installed lapin-ux-uninstall; this copy works without it.
exec bash "$(dirname "$0")/tools/lapin-ux-uninstall" "$@"
