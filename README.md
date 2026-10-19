# Omadora

Pandora in the Omarchy bar.

The bar shows the Pandora mark. Left-click it for the mini player (art, progress, transport). The expand control opens the full player as a regular window that Hyprland tiles like any other app: stations and playlists on the left, the track in the middle, up next and history on the right. The shrink control returns to the mini player. Colors come from the active Omarchy theme.

`omadora-engine` (in `engine/`) plays everything: your radio stations, Shuffle, and albums, playlists, and songs on demand. It also serves the catalog: search, artist pages, albums, and playlists. On demand, the controls are previous, replay, play, and next. On radio they are thumbs, replay, play, skip, tired, and the station's modes, within what Pandora allows for each song. Drag the progress bar to scrub wherever replays are allowed. Volume is the system output.

Pandora lets one device play an account at a time. When another one is playing, the player says so and offers Listen here, which takes the account over the way Pandora's own "Let me listen" does.

Typing in search filters your stations; Enter searches Pandora for songs, artists, albums, and playlists. Artist pages have the bio, popular songs, the discography, and similar artists. Album and playlist pages list every track; picking one plays from there. Clicking the artist, the album, or the album or playlist name under the track opens its page, and Back retraces the way you came. Songs and albums your Pandora plan cannot play on demand say so, and start a station instead. Clicking a song in Up next jumps to it.

The mode chips are the current station's own modes (My Station, Crowd Faves, Deep Cuts, and whatever else Pandora offers for it); a new mode starts with the next song. The moon sets a sleep timer (15, 30, 60 minutes, off) that pauses whatever is playing, and the overflow menu marks a radio song as tired.

The widget starts the engine on its own and restarts it if it dies; the engine keeps its Pandora session across restarts, so it stays the one device listening. Left-click opens and closes the player. Middle-click and the wheel skip forward.

## Sign in

The engine signs in with the Pandora account in a file that stays on this machine:

```text
~/.config/omadora/pianobar/config
```

The file is mode 600. It is not part of this checkout. Put your email and password there:

```text
user = you@example.com
password = your-password
```

A password command works in place of `password`:

```text
password_command = secret-tool lookup service pandora
```

The engine listens on `$XDG_RUNTIME_DIR/omadora/engine.sock` and keeps its session beside it; [engine/README.md](engine/README.md) has its protocol and how it signs in.

## Install

The engine needs a Rust toolchain to build, and ffmpeg (part of Omarchy) to decode radio. From this checkout:

```bash
make install
omarchy-shell shell rescanPlugins
omarchy plugin enable scttymn.omadora right
```

`make install` builds the engine into `bin/`, copies the plugin to `~/.config/omarchy/plugins/scttymn.omadora` (Omarchy will not load a symlinked plugin), and stops a running engine so the widget starts the new one. Saving a file under that plugin directory reloads it, though a changed QML type sometimes needs `omarchy-restart-shell`.
