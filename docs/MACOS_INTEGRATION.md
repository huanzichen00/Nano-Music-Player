# macOS integration

The desktop status bar displays native computer battery/charging state and a headphone indicator. Click the battery for the current system output and available AirPods battery levels, shown in the approved device-card design. Media-command diagnostics remain in the playback log. See [desktop devices and battery](DESKTOP_DEVICES.md) for the data sources, design references and the AirPods automatic-switching investigation.

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

On macOS, the desktop player registers **Option + R** as a native global shortcut at startup using Carbon's `RegisterEventHotKey`. It works while another application is active and restores a minimized or hidden player before bringing it forward, preserving the current screen. **Option + Shift + R** is a separate global shortcut that hides the player using AppKit's Hide command. Playback, the event loop and shortcut registration remain active; Option + R restores the player. Pressing Option + R again keeps the player focused.

The player must be running. Both registrations are released on exit and do not require AeroSpace or Accessibility/Input Monitoring permission. They are separate from the configurable in-app keymap. Each shortcut registers independently: a conflict on one leaves the other and playback available, with an error in the startup notice and stderr.

Remove any existing `alt-r` binding for the player from AeroSpace and reload its configuration to avoid competing shortcuts. The workspace-follow callback and floating rule can remain enabled independently.

For older player builds without the native shortcut, [`scripts/aerospace-focus-player.sh`](../scripts/aerospace-focus-player.sh) remains available as an optional fallback. Copy it to `~/.config/aerospace/nano-focus-player.sh`, then add this binding to the existing `[mode.main.binding]` section of `~/.aerospace.toml`:

```toml
alt-r = 'exec-and-forget /bin/bash "$HOME/.config/aerospace/nano-focus-player.sh"'
```

Validate with `aerospace reload-config --dry-run --no-gui`, then run `aerospace reload-config --no-gui`. This fallback requires AeroSpace in main mode and does nothing if the player is closed.

## Follow AeroSpace workspace changes

AeroSpace 0.21.2 does not provide native sticky windows; its [sticky-window feature issue](https://github.com/nikitabobko/AeroSpace/issues/2) is still open. Floating layout and Always on Top affect layout/stacking, not workspace membership. The optional [`scripts/aerospace-follow-player.sh`](../scripts/aerospace-follow-player.sh) helper makes the player follow workspace changes on its monitor. It remains managed by AeroSpace and may briefly disappear while the asynchronous callback moves it.

Copy the helper into `~/.config/aerospace/nano-follow-workspace.sh`. Keep the floating-window rule above, and add this root-level setting before any TOML section header:

```toml
exec-on-workspace-change = ['/bin/bash', '-c', 'exec "$HOME/.config/aerospace/nano-follow-workspace.sh"']
```

If an existing workspace callback is present, combine the helper with that callback rather than replacing it. Validate with `aerospace reload-config --dry-run --no-gui`, then reload the configuration. The helper uses the [documented callback](https://nikitabobko.github.io/AeroSpace/guide#exec-on-workspace-change) and explicit window-ID commands. It targets only floating `org.nano.music-player` windows on the focused monitor, queries the current workspace to avoid stale callback events, skips windows already there, and does not focus or launch the app. A player on another monitor stays there. No polling process is installed.

At the user's explicit request, the local configuration was backed up to `~/.aerospace.toml.nano-workspace-backup-20261013-081655`, the helper installed and the callback enabled. Validation/reload succeeded. Native switches through workspaces 4, 5 and back to 3 confirmed the player followed, remained floating/visible, and retained focus on the existing Music, Codex and terminal windows. Other window assignments were unchanged. Five isolated command-fixture tests cover a closed player, layout/current-workspace filtering, quoted names, stale event data and command failures.

To disable following, remove only the Nano workspace callback and reload AeroSpace. The installed helper can then be removed. Keep the floating rule and Always on Top if desired. The application itself never edits AeroSpace configuration.

## Preserve size during backing-scale changes

The controller keeps the last requested logical window size when the native backing scale changes, resets pending aspect-ratio measurements, and reapplies that size at the new scale. A physical-size notification that arrives late must not become a user zoom or be mistaken for extra menu-bar height. Ordinary edge resizing and the zoom commands remain available.

On 2026-10-21, the same floating window and process grew from 243 × 470 to 539 × 1041 points after an AeroSpace workspace switch. Repeated scripted switches did not reproduce it reliably. A separate regression test models asynchronous native DPI/resize notifications, verifies stable dimensions and zoom through repeated scale changes, and then checks that a real size adjustment still works. The pre-fix controller fails this test. This fixes the demonstrated DPI-handling defect; a backing-scale transition during the observed AeroSpace incident remains a hypothesis, and the reported workspace symptom still needs verification after restarting with the updated build.
