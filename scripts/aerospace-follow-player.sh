#!/bin/bash
# Optional AeroSpace workspace-change hook. Never focuses or starts the player.
set -u

nano_aerospace=${NANO_AEROSPACE_BIN:-}
if [ -z "$nano_aerospace" ]; then
    nano_aerospace=$(command -v aerospace 2>/dev/null || true)
    if [ -z "$nano_aerospace" ]; then
        for nano_candidate in /opt/homebrew/bin/aerospace /usr/local/bin/aerospace; do
            if [ -x "$nano_candidate" ]; then nano_aerospace=$nano_candidate; break; fi
        done
    fi
fi
[ -n "$nano_aerospace" ] && [ -x "$nano_aerospace" ] || exit 0

# Keep a player on another monitor where it is already visible.
nano_windows=$("$nano_aerospace" list-windows --monitor focused \
    --app-bundle-id org.nano.music-player \
    --format '%{window-id}|%{workspace}|%{window-layout}' 2>/dev/null) || exit 0

while IFS='|' read -r nano_id nano_previous_workspace nano_layout; do
    case "$nano_id" in ''|*[!0-9]*) continue ;; esac
    [ "$nano_layout" = floating ] || continue
    # An earlier callback can run after another switch. Query the current target.
    nano_workspace=$("$nano_aerospace" list-workspaces --focused 2>/dev/null) || continue
    [ -n "$nano_workspace" ] && [ "$nano_workspace" != "$nano_previous_workspace" ] || continue
    "$nano_aerospace" move-node-to-workspace --window-id "$nano_id" -- "$nano_workspace" \
        </dev/null >/dev/null 2>&1 || true
done <<< "$nano_windows"
