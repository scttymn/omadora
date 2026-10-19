import QtQuick
import Quickshell.Services.Pipewire

// What is playing and what can be done about it. omadora-engine does the
// playing, radio and on demand alike; this shapes its state for the views
// and keeps what the engine does not: the station list, the current
// station's modes, the sleep timer, and the system volume.
QtObject {
  id: root

  required property var engine

  readonly property string mode: engine.state.mode || ""
  readonly property bool onDemand: mode === "ondemand"
  readonly property bool isRadio: mode === "radio"

  readonly property var track: engine.track
  readonly property string title: track ? track.title || "" : ""
  readonly property string artist: track ? track.artist || "" : ""
  readonly property string album: track ? track.album || "" : ""
  readonly property string artUrl: track ? track.art || "" : ""
  readonly property string artistId: track ? track.artistId || "" : ""
  readonly property string albumId: track ? track.albumId || "" : ""
  readonly property string trackId: track ? track.id || "" : ""

  readonly property bool playing: engine.playing
  readonly property bool loading: engine.loading
  readonly property bool hasPlayer: engine.online && engine.signedIn
  readonly property double position: engine.position
  readonly property double length: engine.length
  readonly property string notice: engine.error
  // Another device has the account. Taking it over is the listener's call,
  // as with Pandora's own "Let me listen".
  readonly property bool busy: engine.busy
  readonly property bool needsAccount: engine.needsAccount
  // Pandora Free plays an ad every few songs; this is one.
  readonly property bool isAd: track !== null && track.kind === "ad"
  function listenHere() { engine.listenHere() }

  // What the player is playing from: a station, album, playlist, or song.
  readonly property string sourceKind: {
    if (isRadio)
      return "station"
    var id = engine.source.id || ""
    if (id.indexOf("AL:") === 0)
      return "album"
    if (id.indexOf("PL:") === 0)
      return "playlist"
    return id ? "song" : ""
  }
  readonly property string sourceId: engine.source.id || ""
  readonly property string sourceLabel: engine.source.name || ""
  readonly property string station: isRadio ? sourceLabel : ""

  // Radio tracks carry their own rights; on demand allows everything.
  function allows(right) {
    return onDemand || (isRadio && track !== null && track[right] === true)
  }
  readonly property bool canReplay: length > 0 && allows("canReplay")
  readonly property bool canSeek: canReplay
  readonly property bool canGoBack: onDemand
  readonly property bool canGoNext: onDemand ? engine.index + 1 < engine.queue.length : allows("canSkip")
  readonly property bool canRate: isRadio && allows("canRate")
  readonly property bool canTire: isRadio && allows("canTire")
  // 1 thumbs up, 2 thumbs down, as the views expect.
  readonly property int rating: track && track.rating === 1 ? 1 : (track && track.rating === -1 ? 2 : 0)
  readonly property int skipsLeft: -1

  readonly property var upcoming: engine.queue.slice(engine.index + 1)
  readonly property var history: engine.state.history || []

  function formatTime(seconds) {
    var total = Math.max(0, Math.floor(Number(seconds) || 0))
    var m = Math.floor(total / 60)
    var s = total % 60
    return m + ":" + (s < 10 ? "0" : "") + s
  }

  // ---- transport

  function playPause() {
    if (sourceId === "" && !engine.active)
      playStation("shuffle", "Shuffle")
    else
      engine.toggle()
  }
  function next() { engine.next() }
  function previous() { engine.previous() }
  function replay() { engine.replay() }
  function seekFraction(fraction) {
    if (canSeek)
      engine.seek(fraction * length)
  }
  // Thumbs up: on radio, feedback to the station; anywhere else, the song
  // goes into (or comes out of) the listener's collection.
  readonly property bool liked: isRadio ? rating === 1 : (track ? track.collected === true : false)
  readonly property bool canLike: isRadio ? canRate : (trackId !== "" && engine.signedIn)
  function thumbUp() {
    if (isRadio)
      engine.request("thumbUp")
    else
      engine.request("collect", { id: trackId, add: !liked })
  }
  function thumbDown() { engine.request("thumbDown") }
  function tired() { engine.request("tired") }

  function playOnDemand(sourceId, trackId) {
    engine.play(sourceId, trackId)
  }

  // ---- stations

  property var stations: []
  property bool stationsLoaded: false

  function refreshStations() {
    if (!engine.signedIn)
      return
    engine.request("stations", {}, function (ok, list) {
      if (ok) {
        root.stations = list
        root.stationsLoaded = true
      }
    })
  }

  // Remove a station from the account; done(ok, error).
  function removeStation(id, done) {
    engine.request("removeStation", { id: id }, function (ok, result) {
      if (ok)
        root.stations = result
      if (done)
        done(ok, ok ? "" : String(result))
    })
  }

  function stationNamed(name) {
    var wanted = String(name || "").toLowerCase()
    for (var i = 0; i < stations.length; i++) {
      if ((stations[i].name || "").toLowerCase() === wanted)
        return stations[i]
    }
    return null
  }

  function playStation(id, name) {
    engine.request("station", { id: id, name: name || "" })
  }

  // Radio from a song or artist: the listener's station for it if they
  // have one, else a new one.
  function radioFrom(seed, name) {
    var existing = stationNamed(name + " Radio")
    if (existing)
      playStation(existing.id, existing.name)
    else
      createStation(seed)
  }

  // A station from an artist or song id. Pandora reuses one the listener
  // already has.
  property string creating: ""
  signal stationCreated()

  function createStation(seed) {
    if (!seed || creating !== "")
      return
    creating = seed
    engine.request("createStation", { seed: seed })
  }

  property Connections creationWatch: Connections {
    target: root.engine
    function onStatusChanged() {
      if (root.creating === "" || root.engine.status === "loading")
        return
      var made = root.isRadio && root.engine.status === "playing"
      root.creating = ""
      if (made) {
        root.refreshStations()
        root.stationCreated()
      }
    }
  }

  // ---- the current station's modes

  property var modes: []
  property string modesFor: ""
  property string pendingMode: ""
  readonly property string activeMode: {
    if (pendingMode !== "")
      return pendingMode
    for (var i = 0; i < modes.length; i++) {
      if (modes[i].active)
        return modes[i].name
    }
    return ""
  }
  readonly property bool canSetMode: isRadio && modes.length > 0 && pendingMode === ""

  function refreshModes() {
    if (!isRadio || sourceId === "" || modesFor === sourceId)
      return
    var station = sourceId
    modesFor = station
    modes = []
    engine.request("modes", { station: station }, function (ok, result) {
      if (ok && root.sourceId === station)
        root.modes = result.modes || []
      else if (!ok)
        root.modesFor = ""
    })
  }

  function setMode(name) {
    if (!canSetMode || name === activeMode)
      return
    var id = null
    for (var i = 0; i < modes.length; i++) {
      if (modes[i].name === name)
        id = modes[i].id
    }
    if (id === null)
      return
    pendingMode = name
    var station = sourceId
    engine.request("setMode", { station: station, mode: id }, function (ok, result) {
      root.pendingMode = ""
      if (ok && root.sourceId === station)
        root.modes = result.modes || []
    })
  }

  onSourceIdChanged: {
    modes = []
    modesFor = ""
  }

  property Connections signInWatch: Connections {
    target: root.engine
    function onSignedInChanged() {
      if (root.engine.signedIn)
        root.refreshStations()
      else if (root.engine.needsAccount)
        root.stations = []
    }
  }

  // ---- sleep timer: each press steps through these minutes, then off.

  readonly property var sleepSteps: [15, 30, 60]
  property int sleepMinutes: 0
  property double sleepAt: 0
  property double sleepNow: Date.now()
  readonly property int sleepLeft: sleepAt ? Math.max(0, Math.ceil((sleepAt - sleepNow) / 60000)) : 0

  function cycleSleep() {
    var at = sleepSteps.indexOf(sleepMinutes)
    sleepMinutes = at + 1 < sleepSteps.length ? sleepSteps[at + 1] : 0
    sleepAt = sleepMinutes > 0 ? Date.now() + sleepMinutes * 60000 : 0
    sleepNow = Date.now()
  }

  property Timer sleepTimer: Timer {
    interval: 15000
    repeat: true
    running: root.sleepAt > 0
    onTriggered: {
      root.sleepNow = Date.now()
      if (root.sleepNow < root.sleepAt)
        return
      root.sleepAt = 0
      root.sleepMinutes = 0
      root.engine.pause()
    }
  }

  // ---- system volume

  readonly property var sink: Pipewire.defaultAudioSink
  readonly property bool volumeReady: sink !== null && sink.audio !== null
  readonly property real volume: volumeReady ? sink.audio.volume : 0

  function setVolume(fraction) {
    if (volumeReady)
      sink.audio.volume = Math.max(0, Math.min(1, fraction))
  }

  property PwObjectTracker sinkTracker: PwObjectTracker {
    objects: root.sink ? [root.sink] : []
  }
}
