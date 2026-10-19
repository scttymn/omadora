# omadora-engine

The Pandora engine behind the Omadora widget. It signs in, browses the
catalog, plays radio stations, and plays albums, playlists, and tracks on
demand, with seek and replay. The widget talks to it over a Unix socket.

```bash
cargo build --release
./target/release/omadora-engine            # run it
./target/release/omadora-engine send '{"cmd":"search","query":"ella langley"}'
```

`OMADORA_AUDIO=null` plays to a silent sink at real-time speed, for testing.

## How it signs in

Pandora's web login sits behind a bot check, so the engine signs in through
the device API instead, the way pianobar does, with the credentials in
`~/.config/omadora/pianobar/config`. The web API at `www.pandora.com/api`
accepts that sign-in's token, and it has the catalog, playlists, and
on-demand audio. Nothing here pretends to be a browser. This relies on
Pandora's unofficial APIs, which they can change. Plays are reported the
way Pandora's own apps do: the receipt URL, and on radio `trackStarted`,
`playbackPaused`, and `playbackResumed`.

Pandora treats each sign-in as a device and lets one device play an account
at a time; another gets `STREAM_VIOLATION`, reported as `busy` with
Pandora's message. `listenHere` takes over with `playbackResumed`
`forceActive`, as Pandora's "Let me listen" does. The session is kept in
`$XDG_RUNTIME_DIR/omadora/session.json` (mode 600, gone at reboot) so a
restarted engine is the same device.

Where the listener was (source, song, position, radio history) is saved to
`$XDG_STATE_HOME/omadora/resume.json` (mode 600) as it changes and every few
seconds while playing. On start the engine restores it paused, without any
playback request, so it never takes the stream from another device; play
fetches the song and seeks to the saved spot. A radio song whose audio link
has expired starts its station fresh instead.

Radio comes as HE-AAC, decoded with ffmpeg (no Rust decoder reconstructs
its high band); on-demand MP3 decodes in-process.

## Protocol

Newline-delimited JSON on `$XDG_RUNTIME_DIR/omadora/engine.sock`. Each
request may carry a `req` value that its reply echoes. State is pushed as
`{"event": "state", "state": {...}}` on connect and on every change; its
`position` is stamped with `positionAt` (ms since the epoch) so clients can
extrapolate.

| Command | Arguments | Result |
|---|---|---|
| `status` | | the current state |
| `search` | `query` | `artists`, `albums`, `tracks`, `playlists` |
| `artist` | `id` | bio, image, traits, top tracks, discography, similar artists |
| `album` | `id` | the album with its tracks |
| `playlists` | | the listener's playlists |
| `playlist` | `id` | the playlist with its tracks |
| `play` | `source`, optional `track` or `index` | plays an `AL:`, `PL:`, or `TR:` id |
| `toggle`, `pause`, `resume`, `next`, `previous`, `replay`, `stop` | | |
| `seek` | `position` (seconds) | |
| `volume` | `level` (0 to 1) | the engine's own level |
| `stations` | | the listener's radio stations |
| `station` | `id`, optional `name` | plays a station; `id` `shuffle` for Shuffle |
| `createStation` | `seed` | makes a station from an `AR:` or `TR:` id and plays it |
| `thumbUp`, `thumbDown`, `tired` | | feedback on the radio song playing |
| `modes` | `station` | the station's modes and which is on |
| `setMode` | `station`, `mode` | switches it; the next song is in the new mode |
| `listenHere` | | takes the account over from another device |

`previous` restarts the track after its first three seconds; on radio it
only replays. Every item carries `playable`, which is false when the
account has no on-demand rights to it. Radio tracks carry `canSkip`,
`canRate`, `canTire`, and `canReplay`, Pandora's rights for that play, and
the engine enforces them. State `mode` is `radio` or `ondemand`; `history`
lists radio songs already played.

`probe call PATH [JSON]` signs in the same way and prints a redacted
response from any web API endpoint, for finding new ones.
