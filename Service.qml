import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Mpris

// The bar's view of Pandora. Sound still comes from the site, running in its
// own Chrome window. This item only finds that player on MPRIS and asks it
// to play, pause, or skip.
Item {
  id: root

  readonly property string script: {
    var path = Qt.resolvedUrl("bin/omadora").toString()
    return path.replace(/^file:\/\//, "")
  }

  readonly property var players: Mpris.players ? Mpris.players.values : []
  readonly property var player: findPlayer()

  readonly property bool hasPlayer: player !== null
  readonly property bool playing: player ? player.isPlaying : false
  readonly property string title: player && player.trackTitle ? player.trackTitle : ""
  readonly property string artist: player && player.trackArtist ? player.trackArtist : ""
  readonly property string artUrl: player && player.trackArtUrl ? player.trackArtUrl : ""

  function playerText(candidate) {
    var url = ""
    if (candidate.metadata)
      url = candidate.metadata["xesam:url"] || ""
    return [
      candidate.identity || "",
      candidate.desktopEntry || "",
      candidate.dbusName || "",
      url
    ].join(" ").toLowerCase()
  }

  function isPandora(candidate) {
    return playerText(candidate).indexOf("pandora") >= 0
  }

  function findPlayer() {
    var fallback = null
    for (var i = 0; i < players.length; i++) {
      var candidate = players[i]
      if (!candidate || !isPandora(candidate))
        continue
      if (candidate.isPlaying)
        return candidate
      if (!fallback)
        fallback = candidate
    }
    return fallback
  }

  function playPause() {
    var target = player
    if (!target)
      return false
    if (target.canTogglePlaying) {
      target.togglePlaying()
      return true
    }
    if (target.isPlaying && target.canPause) {
      target.pause()
      return true
    }
    if (target.canPlay) {
      target.play()
      return true
    }
    return false
  }

  function next() {
    if (player && player.canGoNext) {
      player.next()
      return true
    }
    return false
  }

  function previous() {
    if (player && player.canGoPrevious) {
      player.previous()
      return true
    }
    return false
  }

  function engine(action) {
    engineProc.command = [script, action]
    engineProc.running = true
  }

  Process {
    id: engineProc
  }
}
