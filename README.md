# Omadora

Pandora in the Omarchy bar.

The widget is the player: title, artist, play, pause, skip. The sound comes from pandora.com running in its own Chrome window, which is the engine. There is no WebKitGTK app and no second Pandora interface to keep up with.

Chrome is launched with its own profile and the window class `omadora`, so it stays separate from the browser you are using. Right-click the bar widget to show that window or tuck it onto the `omadora` special workspace.

Track text shows up when that Chrome window publishes itself on MPRIS with "pandora" in its identity or page URL. Until then the widget still starts, shows, and hides the window.

## Install

Omarchy will not load a symlinked plugin. From this checkout:

```bash
rm -rf ~/.config/omarchy/plugins/scttymn.omadora
cp -a . ~/.config/omarchy/plugins/scttymn.omadora
omarchy plugin enable scttymn.omadora right
```

Click the widget to open Pandora the first time. Log in there once. The profile is `~/.local/share/omadora/chrome`.

## Not the Spotify plugin

Omarchy Spotify can play with no browser because Spotify has a Web API and `librespot`. Pandora has neither. This plugin copies the bar-and-panel shape, and keeps the site as a hidden engine.
