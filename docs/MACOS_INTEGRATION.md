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
