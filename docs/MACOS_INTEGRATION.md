# macOS integration

The player starts above other ordinary application windows by default. Settings → Always on Top, or the checked View → Always on Top menu item, toggles this preference and saves it across restarts. This uses Slint's native window level and is independent of AeroSpace's floating layout. It does not override macOS system overlays or other applications' special window levels.

Device Mode uses a transparent frameless window around a rounded SVG shell. Its outer 15-unit and inner 10-unit corner curves scale with the same hardware geometry as the screen and controls. Screen Mode retains an opaque window.

The packaging script creates `Nano Music Player.app` with the stable bundle identifier `org.nano.music-player`. AeroSpace can float this app's window by adding the following entry to the user's own `~/.aerospace.toml`:

```toml
[[on-window-detected]]
if = 'test %{app-bundle-id} = org.nano.music-player'
run = 'layout floating'
```

This is an optional, user-managed window-manager rule. The app does not read, create, or modify AeroSpace configuration. AeroSpace documents `on-window-detected` callbacks and recommends the `if = 'test %{app-bundle-id} = …'` form; the older `if.app-id` form remains supported but is soft deprecated. Its `list-apps` command can confirm the ID of a running app. See the [AeroSpace callback guide](https://nikitabobko.github.io/AeroSpace/guide#on-window-detected) and [command reference](https://nikitabobko.github.io/AeroSpace/commands#list-apps).

The bundle ID is set in [`scripts/package-macos.sh`](../scripts/package-macos.sh). Keep it stable when changing the display name or executable; changing it would require users' app-specific window-manager rules and macOS identity-based settings to be updated.

## Focus with Option + R

Copy [`scripts/aerospace-focus-player.sh`](../scripts/aerospace-focus-player.sh) to `~/.config/aerospace/nano-focus-player.sh`, then add this binding to the existing `[mode.main.binding]` section of `~/.aerospace.toml`:

```toml
alt-r = 'exec-and-forget /bin/bash "$HOME/.config/aerospace/nano-focus-player.sh"'
```

Validate with `aerospace reload-config --dry-run --no-gui`, then run `aerospace reload-config --no-gui`. **Option + R** focuses a running player from another application, including a player on another workspace or monitor. It preserves the current player screen. If the player is closed, the shortcut does nothing. AeroSpace must be running in main mode; this binding is separate from the player's in-app shortcuts.

## Follow AeroSpace workspace changes

AeroSpace 0.21.2 does not provide native sticky windows; its [sticky-window feature issue](https://github.com/nikitabobko/AeroSpace/issues/2) is still open. Floating layout and Always on Top affect layout/stacking, not workspace membership. The optional [`scripts/aerospace-follow-player.sh`](../scripts/aerospace-follow-player.sh) helper makes the player follow workspace changes on its monitor. It remains managed by AeroSpace and may briefly disappear while the asynchronous callback moves it.

Copy the helper into `~/.config/aerospace/nano-follow-workspace.sh`. Keep the floating-window rule above, and add this root-level setting before any TOML section header:

```toml
exec-on-workspace-change = ['/bin/bash', '-c', 'exec "$HOME/.config/aerospace/nano-follow-workspace.sh"']
```

If an existing workspace callback is present, combine the helper with that callback rather than replacing it. Validate with `aerospace reload-config --dry-run --no-gui`, then reload the configuration. The helper uses the [documented callback](https://nikitabobko.github.io/AeroSpace/guide#exec-on-workspace-change) and explicit window-ID commands. It targets only floating `org.nano.music-player` windows on the focused monitor, queries the current workspace to avoid stale callback events, skips windows already there, and does not focus or launch the app. A player on another monitor stays there. No polling process is installed.

At the user's explicit request, the local configuration was backed up to `~/.aerospace.toml.nano-workspace-backup-20261013-081655`, the helper installed and the callback enabled. Validation/reload succeeded. Native switches through workspaces 4, 5 and back to 3 confirmed the player followed, remained floating/visible, and retained focus on the existing Music, Codex and terminal windows. Other window assignments were unchanged. Five isolated command-fixture tests cover a closed player, layout/current-workspace filtering, quoted names, stale event data and command failures.

To disable following, remove only the Nano workspace callback and reload AeroSpace. The installed helper can then be removed. Keep the floating rule and Always on Top if desired. The application itself never edits AeroSpace configuration.
