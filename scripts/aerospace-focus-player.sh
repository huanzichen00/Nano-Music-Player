#!/bin/bash
# Optional AeroSpace shortcut helper. Focuses a running player on any monitor.
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

nano_windows=$("$nano_aerospace" list-windows --monitor all \
    --app-bundle-id org.nano.music-player --format '%{window-id}' 2>/dev/null) || exit 0

while IFS= read -r nano_id; do
    case "$nano_id" in ''|*[!0-9]*) continue ;; esac
    # A window can close after the query; try the next one if focusing fails.
    if "$nano_aerospace" focus --window-id "$nano_id" </dev/null >/dev/null 2>&1; then
        exit 0
    fi
done <<< "$nano_windows"
exit 0
