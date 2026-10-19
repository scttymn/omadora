import QtQuick
import Quickshell
import Quickshell.Io

// The widget's view of omadora-engine: radio stations, on-demand albums,
// playlists, and tracks, plus the catalog. Talks newline-delimited JSON over the engine's
// socket, and starts the engine when nothing answers there.
Item {
  id: root

  readonly property string socketPath: Quickshell.env("XDG_RUNTIME_DIR") + "/omadora/engine.sock"
  // The engine to start, as scripts/fetch-engine reports it: a `make` build
  // in the plugin's bin/, or the pinned GitHub release it downloaded into
  // ~/.local/share/omadora/engine (see README).
  property string binary: ""
  readonly property string fetchScript: Qt.resolvedUrl("scripts/fetch-engine").toString().replace(/^file:\/\//, "")
  // The engine binary is in place (fetched, or already there).
  property bool fetched: false
  // Why there is no engine to start, when there is none.
  property string startError: ""

  property var state: ({})
  property bool online: false
  property int clock: 0
  property var pending: ({})
  property int nextReq: 1
  property int launches: 0

  readonly property string status: state.status || "idle"
  readonly property bool signedIn: state.signedIn === true
  // No account is saved; the full player shows the sign-in form.
  readonly property bool needsAccount: online && state.needsAccount === true
  readonly property string user: state.user || ""
  readonly property var account: state.account || null
  readonly property bool active: status === "playing" || status === "paused" || status === "loading"
  readonly property bool playing: status === "playing"
  readonly property bool loading: status === "loading"
  readonly property var track: state.track || null
  readonly property var queue: state.queue || []
  readonly property int index: state.index === undefined || state.index === null ? -1 : state.index
  readonly property var source: state.source || ({ id: "", name: "" })
  readonly property string error: state.error || (online ? "" : startError)
  // Another device has the account; listenHere() takes it over.
  readonly property bool busy: state.busy === true
  readonly property double length: track ? Number(track.duration) || 0 : 0
  readonly property double position: {
    clock
    var base = Number(state.position) || 0
    if (!playing)
      return base
    var at = Number(state.positionAt) || Date.now()
    return Math.min(length || Infinity, base + Math.max(0, Date.now() - at) / 1000)
  }

  // ---- requests

  function request(cmd, args, done) {
    var msg = Object.assign({ cmd: cmd, req: nextReq++ }, args || {})
    if (done)
      pending[msg.req] = done
    if (!online) {
      if (done)
        done(false, "The player engine is starting.")
      delete pending[msg.req]
      ensureRunning()
      return
    }
    socket.write(JSON.stringify(msg) + "\n")
    socket.flush()
  }

  function play(sourceId, trackId) {
    var args = { source: sourceId }
    if (trackId)
      args.track = trackId
    request("play", args)
  }

  function toggle() { request("toggle") }
  function pause() { request("pause") }
  function resume() { request("resume") }
  function next() { request("next") }
  function previous() { request("previous") }
  function replay() { request("replay") }
  function stop() { request("stop") }
  function seek(seconds) { request("seek", { position: seconds }) }
  function listenHere() { request("listenHere") }
  function signIn(user, password, done) { request("signIn", { user: user, password: password }, done) }
  function signOut() { request("signOut") }

  function handle(line) {
    var msg
    try {
      msg = JSON.parse(line)
    } catch (e) {
      return
    }
    if (msg.event === "state") {
      state = msg.state || {}
      return
    }
    var done = pending[msg.req]
    if (done) {
      delete pending[msg.req]
      done(msg.ok === true, msg.ok ? msg.result : msg.error)
    }
  }

  // ---- connection and lifecycle

  function ensureRunning() {
    if (online || launchTimer.running)
      return
    if (!fetched) {
      fetchEngine()
      return
    }
    // Back off if it keeps dying, so a broken engine cannot spin.
    if (launches >= 5)
      return
    launches++
    Quickshell.execDetached([binary])
    launchTimer.restart()
  }

  // Make sure the pinned engine release is in bin/ before starting it.
  function fetchEngine() {
    if (fetcher.running || fetchRetry.running)
      return
    fetcher.running = true
  }

  Process {
    id: fetcher
    command: ["bash", root.fetchScript]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var line = text.trim().split("\n").pop() || ""
        if (line.indexOf("error: ") === 0) {
          root.startError = line.slice(7)
          fetchRetry.restart()
          return
        }
        // "ok PATH", "updated PATH", or "warning PATH: why".
        var word = line.split(" ")[0]
        var rest = line.slice(word.length + 1)
        root.binary = word === "warning" ? rest.split(": ")[0] : rest
        root.startError = ""
        root.fetched = root.binary !== ""
        // The script stopped an engine that wasn't this one (the release
        // before `omarchy plugin update`, say); start this one once the old
        // one's socket is gone.
        if (word === "updated")
          retryTimer.restart()
        else
          root.ensureRunning()
      }
    }
  }

  // After a failed download, wait before trying again.
  Timer {
    id: fetchRetry
    interval: 60000
    onTriggered: root.ensureRunning()
  }

  Socket {
    id: socket
    path: root.socketPath
    connected: true
    parser: SplitParser {
      onRead: function (data) { root.handle(data) }
    }
    onConnectionStateChanged: {
      root.online = connected
      if (connected) {
        root.launches = 0
        return
      }
      // Fail any waiting requests; the engine is gone.
      var waiting = root.pending
      root.pending = ({})
      for (var k in waiting)
        waiting[k](false, "The player engine stopped.")
      root.state = ({})
      retryTimer.restart()
    }
    onError: retryTimer.restart()
  }

  // Reconnect quietly; launch the engine if a reconnect finds nothing.
  Timer {
    id: retryTimer
    interval: 1000
    onTriggered: {
      if (root.online)
        return
      socket.connected = false
      socket.connected = true
      if (!root.online)
        root.ensureRunning()
    }
  }

  Timer {
    id: launchTimer
    interval: 1500
    onTriggered: {
      socket.connected = false
      socket.connected = true
      if (!root.online)
        retryTimer.restart()
    }
  }

  Timer {
    interval: 500
    repeat: true
    running: root.playing
    onTriggered: root.clock++
  }

  // The first connect can fail without an error signal; check once. Check
  // for a newer pinned engine at the same time (after `omarchy plugin update`).
  Component.onCompleted: {
    fetchEngine()
    retryTimer.restart()
  }
}
