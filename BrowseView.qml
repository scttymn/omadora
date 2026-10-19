import QtQuick
import qs.Commons

// The full player's pages: Music, Browse, search results, an
// artist, an album, or a playlist, from Pandora's catalog through the
// engine. Albums, playlists, and songs play on demand when the account
// allows; otherwise they start a radio station.
Item {
  id: root

  required property var engine
  required property var player
  required property var theme
  property string fontFamily: Style.font.family

  // {kind: "collection" | "browse" | "search" | "artist" | "album" | "playlist",
  //  id, name, artist, query, path}. A browse page with neither path nor id
  //  is Browse itself; a path is a section's full list, an id a mood.
  property var page: ({ kind: "search" })
  property var localStations: []
  property bool canGoBack: false

  signal back()
  signal open(var page)
  signal stationPicked(var station)

  // ---- loading

  property var cache: ({})
  property var loaded: null
  property string loadedKey: ""
  property string loadError: ""
  readonly property string key: page.kind + "|" + (page.id || page.path || page.query || ((page.artist || "") + "/" + (page.name || "")))
  // Only data fetched for this page; never the last page's while it loads.
  readonly property var info: loadedKey === key ? loaded : null
  readonly property bool loading: info === null && loadError === ""

  onKeyChanged: load()
  onVisibleChanged: if (visible && info === null) load()
  Component.onCompleted: load()

  // The engine answers before it has signed in; wait for that.
  Connections {
    target: root.engine
    function onSignedInChanged() {
      if (root.engine.signedIn && root.visible && root.info === null)
        root.load()
    }
  }

  function lower(v) {
    return String(v || "").toLowerCase()
  }

  // Take an album or song out of "In your collection": out of the
  // collection, its station thumbs removed, or both, as it was listed.
  property string uncollectError: ""
  // Music's filter: "all", "stations", "playlists", or "artists".
  property string collectionFilter: "all"
  function uncollect(item) {
    uncollectError = ""
    engine.request("uncollect", { id: item.id, from: item.from || ["collection"] }, function (ok, result) {
      if (!ok)
        root.uncollectError = String(result)
    })
  }

  // The collection or thumbs changed: artist pages' "In your collection" is
  // out of date. Forget them, and read the one on screen again.
  Connections {
    target: root.engine
    function onCollectionVersionChanged() {
      var next = {}
      for (var k in root.cache) {
        if (!root.cache[k] || root.cache[k].collected === undefined)
          next[k] = root.cache[k]
      }
      root.cache = next
      if (root.page && (root.page.kind === "artist" || root.page.kind === "collection"))
        root.load()
    }
  }

  function load() {
    if (!visible)
      return
    var k = key
    loadError = ""
    // Music shows what it had at once, and reads it again: it
    // changes from everywhere.
    if (cache[k]) {
      loaded = cache[k]
      loadedKey = k
      if (page.kind !== "collection")
        return
    }
    if (!engine.signedIn)
      return
    var done = function (ok, result) {
      if (k !== root.key)
        return
      if (!ok) {
        root.loadError = String(result)
        return
      }
      var next = Object.assign({}, root.cache)
      next[k] = result
      root.cache = next
      root.loaded = result
      root.loadedKey = k
    }
    var p = page
    if (p.kind === "collection") {
      loadCollection(done)
    } else if (p.kind === "browse") {
      engine.request("browse", { path: p.path || "", id: p.id || "" }, done)
    } else if (p.kind === "search") {
      engine.request("search", { query: p.query }, done)
    } else if (p.id) {
      engine.request(p.kind, { id: p.id }, done)
    } else {
      // Radio tracks come with names only; find the id by searching.
      resolve(p, function (id) {
        if (!id)
          return done(false, "There is no " + p.kind + " called " + p.name + ".")
        engine.request(p.kind, { id: id }, done)
      })
    }
  }

  // The playlists and the collection's artists; the stations are the
  // player's own list, always up to date.
  function loadCollection(done) {
    var got = {}
    var failed = ""
    var waiting = 2
    var finish = function () {
      if (--waiting > 0)
        return
      if (failed !== "" && !got.playlists && !got.artists)
        done(false, failed)
      else
        done(true, { playlists: got.playlists || [], artists: got.artists || [] })
    }
    engine.request("playlists", {}, function (ok, result) {
      if (ok) got.playlists = result
      else failed = String(result)
      finish()
    })
    engine.request("collectionArtists", {}, function (ok, result) {
      if (ok) got.artists = result
      else failed = String(result)
      finish()
    })
  }

  function resolve(p, then) {
    var query = p.kind === "album" ? (p.artist + " " + p.name) : p.name
    engine.request("search", { query: query }, function (ok, found) {
      if (!ok)
        return then("")
      var list = p.kind === "album" ? found.albums : found.artists
      for (var i = 0; i < list.length; i++) {
        var item = list[i]
        if (lower(item.name) !== lower(p.name))
          continue
        if (p.kind === "album" && p.artist && lower(item.artist) !== lower(p.artist))
          continue
        return then(item.id)
      }
      then(list.length ? list[0].id : "")
    })
  }

  // ---- actions

  function formatTime(seconds) {
    return player.formatTime(seconds)
  }

  function totalTime(tracks) {
    var total = 0
    for (var i = 0; i < (tracks || []).length; i++)
      total += Number(tracks[i].duration) || 0
    var minutes = Math.round(total / 60)
    return minutes >= 60 ? Math.floor(minutes / 60) + " hr " + (minutes % 60) + " min" : minutes + " min"
  }

  // On demand when the plan allows it, else a station from the song.
  function playTrack(track, sourceId) {
    if (track.playable)
      player.playOnDemand(sourceId || track.albumId || track.id, track.id)
    else
      startRadio(track.id, track.title)
  }

  // Play the station if the listener has it, else make one from the artist
  // or song id (Pandora reuses an existing one for the same seed).
  function startRadio(seed, name) {
    var existing = player.stationNamed(name + " Radio")
    if (existing) {
      root.stationPicked(existing)
      return
    }
    player.createStation(seed)
  }

  readonly property bool radioBusy: player.creating !== ""

  // A tile from Browse or Music, by what it is.
  function pick(item) {
    if (item.kind === "album")
      root.open({ kind: "album", id: item.id, name: item.title, artist: item.artist || "" })
    else if (item.kind === "artist")
      root.open({ kind: "artist", id: item.id, name: item.title })
    else if (item.kind === "playlist")
      root.open({ kind: "playlist", id: item.id, name: item.title })
    else if (item.kind === "category")
      root.open({ kind: "browse", id: item.id, name: item.title })
    else if (item.kind === "mine")
      root.stationPicked(item.station)
    else if (item.kind === "shuffle")
      root.stationPicked({ id: "shuffle", name: "Shuffle" })
    else if (item.kind === "station") {
      // A genre station: the listener's own if they have it, else a new one.
      var existing = player.stationNamed(item.title)
      if (existing)
        root.stationPicked(existing)
      else
        player.createStation(item.id)
    }
  }

  readonly property bool shuffling: player.isRadio && engine.state.station && engine.state.station.shuffle === true

  // Whether a tile is what's playing.
  function playing(item) {
    if (item.kind === "shuffle")
      return shuffling
    if (item.kind === "mine")
      return player.isRadio && !shuffling && item.id === player.sourceId
    if (item.kind === "playlist" || item.kind === "album")
      return player.onDemand && item.id === player.sourceId
    return false
  }

  // A station out of the account, from Music; its thumbs go too.
  property string removeError: ""
  function removeStation(item) {
    removeError = ""
    player.removeStation(item.id, function (ok, error) {
      if (!ok)
        root.removeError = error
    })
  }

  function count(n, one, many) {
    return n + " " + (n === 1 ? one : many)
  }

  // ---- pieces

  component Label: Text {
    textFormat: Text.PlainText
    font.family: root.fontFamily
    elide: Text.ElideRight
  }

  component Heading: Label {
    width: parent ? parent.width : 0
    topPadding: Style.space(24)
    bottomPadding: Style.space(8)
    color: root.theme.faint
    font.pixelSize: Style.font.bodySmall
    font.capitalization: Font.AllUppercase
    font.letterSpacing: 1.3
  }

  component Note: Label {
    width: parent ? parent.width : 0
    wrapMode: Text.WordWrap
    color: root.theme.muted
    font.pixelSize: Style.font.bodySmall
  }

  // "Remove?" over a row or tile, until answered.
  component RemoveConfirm: Rectangle {
    id: confirm
    property string question: "Remove?"
    signal confirmed()
    signal cancelled()
    radius: Style.cornerRadius
    color: root.theme.surface
    border.color: root.theme.accent
    border.width: 1

    // Clicks here don't reach what's underneath.
    MouseArea { anchors.fill: parent }

    Flow {
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.leftMargin: Style.space(12)
      anchors.rightMargin: Style.space(12)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(12)

      Label {
        text: confirm.question
        color: root.theme.strong
        font.pixelSize: Style.font.bodySmall
      }
      Link {
        text: "Remove"
        idle: root.theme.accent
        font.pixelSize: Style.font.bodySmall
        font.bold: true
        onClicked: confirm.confirmed()
      }
      Link {
        text: "Cancel"
        font.pixelSize: Style.font.bodySmall
        onClicked: confirm.cancelled()
      }
    }
  }

  component Link: Label {
    property color idle: root.theme.fg
    signal clicked()
    color: linkArea.containsMouse ? root.theme.accent : idle
    font.underline: linkArea.containsMouse

    MouseArea {
      id: linkArea
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: parent.clicked()
    }
  }

  component Button: Rectangle {
    id: button
    property string label: ""
    property string glyph: ""
    property bool primary: true
    signal activated()

    width: buttonRow.implicitWidth + Style.space(32)
    height: Style.space(38)
    radius: Style.cornerRadius
    opacity: enabled ? 1 : 0.45
    color: primary
      ? (buttonArea.containsMouse && enabled ? Qt.lighter(root.theme.accent, 1.08) : root.theme.accent)
      : (buttonArea.containsMouse && enabled ? root.theme.raised : "transparent")
    border.color: primary ? "transparent" : root.theme.border
    border.width: primary ? 0 : 1

    Row {
      id: buttonRow
      anchors.centerIn: parent
      spacing: Style.space(8)

      Label {
        visible: button.glyph !== ""
        text: button.glyph
        color: button.primary ? root.theme.accentText : root.theme.fg
        font.pixelSize: Style.font.title
        anchors.verticalCenter: parent.verticalCenter
      }

      Label {
        text: button.label
        color: button.primary ? root.theme.accentText : root.theme.fg
        font.pixelSize: Style.font.body
        font.bold: button.primary
        anchors.verticalCenter: parent.verticalCenter
      }
    }

    MouseArea {
      id: buttonArea
      anchors.fill: parent
      hoverEnabled: true
      enabled: button.enabled
      cursorShape: Qt.PointingHandCursor
      onClicked: button.activated()
    }
  }

  // A song: number or art, title, a second line, and its length.
  component TrackRow: Rectangle {
    id: trackRow
    property var track: ({})
    property string lead: ""
    property bool showArt: false
    property string detail: ""
    readonly property bool current: root.player.onDemand && root.player.trackId !== "" && root.player.trackId === trackRow.track.id
    // The song a Discover chip pointed at.
    readonly property bool focused: root.page.track !== undefined && root.page.track === trackRow.track.id
    // In "In your collection": a × to take it out, after a "Remove?".
    property bool removable: false
    property bool confirming: false
    signal picked()
    signal removeRequested()

    width: parent ? parent.width : 0
    height: Style.space(50)
    radius: Style.cornerRadius
    color: current ? root.theme.raised
      : focused ? Util.alpha(root.theme.accent, 0.14)
      : (trackArea.containsMouse ? Util.alpha(root.theme.raised, 0.6) : "transparent")
    // Radio-only songs are dimmed: their art and text, not the row, so the
    // "Remove?" over it stays solid.
    readonly property real dim: track.playable === false ? 0.6 : 1

    Rectangle {
      visible: trackRow.focused
      width: Style.space(3)
      height: parent.height
      color: root.theme.accent
    }

    Label {
      id: leadLabel
      opacity: trackRow.confirming ? 0 : trackRow.dim
      visible: !trackRow.showArt
      anchors.left: parent.left
      anchors.leftMargin: Style.space(10)
      anchors.verticalCenter: parent.verticalCenter
      width: Style.space(26)
      text: trackRow.current && root.player.playing ? "󰝚" : (trackArea.containsMouse ? "󰐊" : trackRow.lead)
      color: trackRow.current ? root.theme.accent : root.theme.muted
      font.pixelSize: Style.font.body
    }

    ArtMark {
      id: rowArt
      opacity: trackRow.confirming ? 0 : trackRow.dim
      visible: trackRow.showArt
      anchors.left: parent.left
      anchors.leftMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      width: Style.space(36)
      height: width
      rings: false
      art: trackRow.track.color || root.theme.art
      artUrl: trackRow.track.art || ""
    }

    Column {
      opacity: trackRow.confirming ? 0 : trackRow.dim
      anchors.left: trackRow.showArt ? rowArt.right : leadLabel.right
      anchors.leftMargin: Style.space(12)
      anchors.right: lengthLabel.left
      anchors.rightMargin: Style.space(12)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(2)

      Row {
        width: parent.width
        spacing: Style.space(6)

        Label {
          width: Math.min(implicitWidth, parent.width - (explicitTag.visible ? explicitTag.width + Style.space(6) : 0))
          text: trackRow.track.title || ""
          color: trackRow.current ? root.theme.accent : root.theme.strong
          font.pixelSize: Style.font.body
          font.bold: trackRow.current
        }

        Rectangle {
          id: explicitTag
          visible: trackRow.track.explicit === true
          width: Style.space(14)
          height: width
          radius: Style.cornerRadius
          color: root.theme.border
          anchors.verticalCenter: parent.verticalCenter

          Label {
            anchors.centerIn: parent
            text: "E"
            color: root.theme.bg
            font.pixelSize: Style.font.caption
            font.bold: true
          }
        }
      }

      Label {
        width: parent.width
        visible: text !== ""
        text: trackRow.detail
        color: root.theme.muted
        font.pixelSize: Style.font.caption
      }
    }

    Label {
      id: lengthLabel
      opacity: trackRow.confirming ? 0 : trackRow.dim
      anchors.right: parent.right
      anchors.rightMargin: Style.space(12)
      anchors.verticalCenter: parent.verticalCenter
      visible: !rowHover.hovered || !trackRow.track.id
      text: trackRow.track.playable === false ? "Radio only" : root.formatTime(trackRow.track.duration)
      color: root.theme.muted
      font.pixelSize: Style.font.bodySmall
    }

    MouseArea {
      id: trackArea
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: trackRow.picked()
    }

    // Over the row: radio from this song, in place of its length.
    HoverHandler {
      id: rowHover
    }

    IconButton {
      id: radioButton
      visible: rowHover.hovered && !!trackRow.track.id
      anchors.right: parent.right
      anchors.rightMargin: Style.space(6)
      anchors.verticalCenter: parent.verticalCenter
      width: Style.space(30)
      theme: root.theme
      icon: "󰀃"
      iconSize: Style.font.body
      fontFamily: root.fontFamily
      enabled: !root.radioBusy
      onActivated: root.startRadio(trackRow.track.id, trackRow.track.title)
    }

    IconButton {
      visible: trackRow.removable && rowHover.hovered && !trackRow.confirming
      anchors.right: radioButton.left
      anchors.verticalCenter: parent.verticalCenter
      width: Style.space(30)
      theme: root.theme
      icon: "󰅖"
      iconSize: Style.font.body
      fontFamily: root.fontFamily
      onActivated: trackRow.confirming = true
    }

    RemoveConfirm {
      visible: trackRow.confirming
      anchors.fill: parent
      question: "Remove " + (trackRow.track.title || "it") + "?"
      onConfirmed: {
        trackRow.confirming = false
        trackRow.removeRequested()
      }
      onCancelled: trackRow.confirming = false
    }
  }

  // An album, artist, or playlist as a square tile with a caption.
  component Tile: Item {
    id: tile
    property string title: ""
    property string detail: ""
    property string art: ""
    property color swatch: root.theme.art
    property string glyph: ""
    // Playing now: the title in the accent.
    property bool current: false
    property bool removable: false
    property bool confirming: false
    signal picked()
    signal removeRequested()

    width: Style.space(148)
    height: tileArt.height + tileText.height + Style.space(10)

    ArtMark {
      id: tileArt
      width: parent.width
      height: width
      rings: false
      art: tile.swatch
      artUrl: tile.art
      opacity: tileArea.containsMouse ? 0.85 : 1
    }

    Label {
      visible: tile.glyph !== ""
      anchors.centerIn: tileArt
      text: tile.glyph
      color: root.theme.accentText
      font.pixelSize: Style.font.displayLarge
    }

    Column {
      id: tileText
      anchors.top: tileArt.bottom
      anchors.topMargin: Style.space(8)
      width: parent.width
      spacing: Style.space(2)

      Label {
        width: parent.width
        text: tile.title
        color: tileArea.containsMouse || tile.current ? root.theme.accent : root.theme.strong
        font.bold: tile.current
        font.pixelSize: Style.font.body
        wrapMode: Text.WordWrap
        maximumLineCount: 2
      }

      Label {
        width: parent.width
        visible: text !== ""
        text: tile.detail
        color: root.theme.muted
        font.pixelSize: Style.font.caption
      }
    }

    MouseArea {
      id: tileArea
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: tile.picked()
    }

    HoverHandler {
      id: tileHover
    }

    IconButton {
      visible: tile.removable && tileHover.hovered && !tile.confirming
      anchors.top: tileArt.top
      anchors.right: tileArt.right
      anchors.margins: Style.space(6)
      width: Style.space(28)
      theme: root.theme
      icon: "󰅖"
      iconSize: Style.font.body
      fontFamily: root.fontFamily
      color: root.theme.surface
      onActivated: tile.confirming = true
    }

    RemoveConfirm {
      visible: tile.confirming
      anchors.fill: tileArt
      question: "Remove " + (tile.title || "it") + "?"
      onConfirmed: {
        tile.confirming = false
        tile.removeRequested()
      }
      onCancelled: tile.confirming = false
    }
  }

  // Tiles under a heading: `rows` rows of them with "View all" to the rest
  // (Browse's own page, and Music's), or all of them, a screenful
  // at a time.
  component Shelf: Column {
    id: shelf
    property string title: ""
    property var items: []
    property int rows: 0
    // A full list elsewhere: "View all" even when every tile fits.
    property string more: ""
    property int shown: 48
    readonly property real gap: Style.space(18)
    readonly property int perRow: Math.max(1, Math.floor((width + gap) / (Style.space(148) + gap)))
    readonly property int limit: rows > 0 ? perRow * rows : shown
    signal moreRequested()

    width: parent ? parent.width : 0
    visible: items.length > 0

    Item {
      width: parent.width
      height: shelf.title !== "" ? shelfHeading.height : Style.space(8)

      Heading {
        id: shelfHeading
        visible: shelf.title !== ""
        width: parent.width - moreLink.width - Style.space(12)
        text: shelf.title
      }

      Link {
        id: moreLink
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Style.space(8)
        visible: shelf.more !== "" || (shelf.rows > 0 && shelf.items.length > shelf.limit)
        text: "View all"
        idle: root.theme.muted
        font.pixelSize: Style.font.bodySmall
        onClicked: shelf.moreRequested()
      }
    }

    Flow {
      width: parent.width
      spacing: shelf.gap

      Repeater {
        model: shelf.items.slice(0, shelf.limit)
        delegate: Tile {
          required property var modelData
          title: modelData.title
          detail: root.player.creating !== "" && root.player.creating === modelData.id ? "Starting the station…" : (modelData.detail || "")
          art: modelData.art || ""
          swatch: modelData.color || root.theme.art
          glyph: modelData.glyph || ""
          current: root.playing(modelData)
          removable: modelData.removable === true
          onPicked: root.pick(modelData)
          onRemoveRequested: root.removeStation(modelData)
        }
      }
    }

    Link {
      visible: shelf.rows === 0 && shelf.items.length > shelf.limit
      topPadding: Style.space(16)
      text: "Show more"
      idle: root.theme.muted
      font.pixelSize: Style.font.bodySmall
      onClicked: shelf.shown += 48
    }
  }

  // Big art, a caption, a title, and whatever is declared inside it.
  component Header: Row {
    id: header
    property string caption: ""
    property string title: ""
    property string art: ""
    property color swatch: root.theme.art
    default property alias extra: headerExtra.data

    width: parent ? parent.width : 0
    spacing: Style.space(24)

    ArtMark {
      id: headerArt
      width: Style.space(176)
      height: width
      rings: header.art === ""
      art: header.swatch
      ring: root.theme.ring
      artUrl: header.art
    }

    Column {
      width: parent.width - headerArt.width - parent.spacing
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(8)

      Label {
        text: header.caption
        color: root.theme.accent
        font.pixelSize: Style.font.bodySmall
        font.capitalization: Font.AllUppercase
        font.letterSpacing: 1.3
      }

      Label {
        width: parent.width
        text: header.title
        color: root.theme.strong
        font.pixelSize: Style.font.displayLarge
        font.bold: true
        wrapMode: Text.WordWrap
        maximumLineCount: 2
      }

      Column {
        id: headerExtra
        width: parent.width
        spacing: Style.space(10)
      }
    }
  }

  // ---- the page

  Flickable {
    id: scroller
    anchors.fill: parent
    contentHeight: content.height + Style.space(40)
    clip: true
    boundsBehavior: Flickable.StopAtBounds

    Connections {
      target: root
      function onKeyChanged() { scroller.contentY = 0 }
    }

    Column {
      id: content
      x: Style.space(28)
      y: Style.space(20)
      width: scroller.width - Style.space(56)

      Link {
        visible: root.canGoBack
        text: "󰁍  Back"
        idle: root.theme.muted
        font.pixelSize: Style.font.bodySmall
        onClicked: root.back()
      }

      Item { visible: root.canGoBack; width: 1; height: Style.space(18) }

      Note {
        visible: root.loading
        text: root.engine.signedIn ? "Loading…" : (root.engine.online ? "Signing in…" : "Starting the player engine…")
      }

      Note {
        visible: root.loadError !== ""
        text: root.loadError
      }


      // ---- Music: everything, a row or two of each, or one kind

      Loader {
        width: parent.width
        active: root.page.kind === "collection" && root.info !== null
        visible: active
        sourceComponent: Column {
          id: collectionPage
          width: parent ? parent.width : 0
          readonly property var mine: (root.player.stations || []).map(function (s) {
            return { kind: "mine", id: s.id, title: s.name || "", detail: s.thumbprint ? "Your thumbs up" : "Station", art: s.art || "", station: s, removable: s.canRemove === true }
          })
          // Shuffle comes first, once there is something to shuffle.
          readonly property var stations: mine.length === 0 ? [] : [{ kind: "shuffle", id: "shuffle", title: "Shuffle all", detail: "All your stations", glyph: "󰒝", color: root.theme.accent }].concat(mine)
          readonly property var playlists: (root.info.playlists || []).map(function (p) {
            return { kind: "playlist", id: p.id, title: p.name || "", detail: root.count(p.trackCount || 0, "song", "songs"), art: p.art || "", color: p.color || "" }
          })
          readonly property var artists: (root.info.artists || []).map(function (a) {
            var parts = []
            if (a.albums)
              parts.push(root.count(a.albums, "album", "albums"))
            if (a.songs)
              parts.push(root.count(a.songs, "song", "songs"))
            return { kind: "artist", id: a.id, title: a.name || "", detail: parts.join(" · "), art: a.art || "", color: a.color || "" }
          })
          readonly property var kinds: [
            { key: "all", label: "All" },
            { key: "stations", label: "Stations", items: stations },
            { key: "playlists", label: "Playlists", items: playlists },
            { key: "artists", label: "Artists", items: artists }
          ]

          Label {
            width: parent.width
            text: "Music"
            color: root.theme.strong
            font.pixelSize: Style.font.display
            font.bold: true
          }

          Row {
            topPadding: Style.space(16)
            spacing: Style.space(8)

            Repeater {
              model: collectionPage.kinds
              delegate: Rectangle {
                required property var modelData
                readonly property bool selected: root.collectionFilter === modelData.key
                // As Discover's filters.
                width: chipText.implicitWidth + Style.space(24)
                height: Style.space(32)
                radius: Style.cornerRadius
                color: selected ? root.theme.accent : (chipArea.containsMouse ? root.theme.raised : "transparent")
                border.color: selected ? root.theme.accent : root.theme.border
                border.width: 1

                Label {
                  id: chipText
                  anchors.centerIn: parent
                  text: modelData.label
                  color: parent.selected ? root.theme.accentText : root.theme.fg
                  font.pixelSize: Style.font.bodySmall
                }

                MouseArea {
                  id: chipArea
                  anchors.fill: parent
                  hoverEnabled: true
                  cursorShape: Qt.PointingHandCursor
                  onClicked: root.collectionFilter = modelData.key
                }
              }
            }
          }

          Note {
            visible: root.removeError !== ""
            topPadding: Style.space(12)
            text: root.removeError
          }

          Repeater {
            model: collectionPage.kinds.slice(1)
            delegate: Shelf {
              required property var modelData
              visible: (root.collectionFilter === "all" || root.collectionFilter === modelData.key) && items.length > 0
              title: root.collectionFilter === "all" ? modelData.label : ""
              items: modelData.items
              rows: root.collectionFilter === "all" ? 2 : 0
              onMoreRequested: root.collectionFilter = modelData.key
            }
          }

          Note {
            visible: collectionPage.stations.length + collectionPage.playlists.length + collectionPage.artists.length === 0
            topPadding: Style.space(16)
            text: "Nothing here yet. Like a song, or start a station from Browse."
          }
        }
      }

      // ---- Browse: the page itself, a section's full list, or a mood

      Loader {
        width: parent.width
        active: root.page.kind === "browse" && root.info !== null
        visible: active
        sourceComponent: Column {
          id: browsePage
          width: parent ? parent.width : 0
          readonly property bool front: !root.page.path && !root.page.id

          Label {
            width: parent.width
            text: root.page.name || "Browse"
            color: root.theme.strong
            font.pixelSize: Style.font.display
            font.bold: true
          }

          Repeater {
            model: root.info.sections || []
            delegate: Shelf {
              required property var modelData
              title: modelData.title || ""
              items: modelData.items || []
              rows: browsePage.front ? 1 : 0
              more: browsePage.front ? (modelData.more || "") : ""
              onMoreRequested: root.open({ kind: "browse", path: modelData.more, name: modelData.title })
            }
          }

          Note {
            visible: (root.info.sections || []).length === 0
            topPadding: Style.space(16)
            text: "Nothing to browse here right now."
          }
        }
      }

      // ---- search

      Loader {
        width: parent.width
        active: root.page.kind === "search" && root.info !== null
        visible: active
        sourceComponent: Column {
          id: searchPage
          width: parent ? parent.width : 0
          readonly property var found: root.info

          Label {
            width: parent.width
            text: "Results for “" + root.page.query + "”"
            color: root.theme.strong
            font.pixelSize: Style.font.display
            font.bold: true
          }

          Heading { visible: root.localStations.length > 0; text: "Your stations" }

          Repeater {
            model: root.localStations
            delegate: ResultRow {
              required property var modelData
              required property int index
              width: searchPage.width
              theme: root.theme
              fontFamily: root.fontFamily
              title: modelData.name || ""
              subtitle: "Station"
              artUrl: modelData.art || ""
              swatch: root.theme.swatch(index)
              current: root.player.isRadio && modelData.id === root.player.sourceId
              onPicked: root.stationPicked(modelData)
            }
          }

          Heading { visible: searchPage.found.tracks.length > 0; text: "Songs" }

          Repeater {
            model: searchPage.found.tracks.slice(0, 8)
            delegate: TrackRow {
              required property var modelData
              track: modelData
              showArt: true
              detail: modelData.artist + (modelData.album ? "  ·  " + modelData.album : "")
              onPicked: root.playTrack(modelData)
            }
          }

          Heading { visible: searchPage.found.artists.length > 0; text: "Artists" }

          Flow {
            width: parent.width
            spacing: Style.space(18)

            Repeater {
              model: searchPage.found.artists.slice(0, 8)
              delegate: Tile {
                required property var modelData
                title: modelData.name
                detail: "Artist"
                art: modelData.art
                swatch: modelData.color || root.theme.art
                onPicked: root.open({ kind: "artist", id: modelData.id, name: modelData.name })
              }
            }
          }

          Heading { visible: searchPage.found.albums.length > 0; text: "Albums" }

          Flow {
            width: parent.width
            spacing: Style.space(18)

            Repeater {
              model: searchPage.found.albums.slice(0, 8)
              delegate: Tile {
                required property var modelData
                title: modelData.name
                detail: modelData.artist + (modelData.year ? " · " + modelData.year : "")
                art: modelData.art
                swatch: modelData.color || root.theme.art
                onPicked: root.open({ kind: "album", id: modelData.id, name: modelData.name, artist: modelData.artist })
              }
            }
          }

          Heading { visible: searchPage.found.playlists.length > 0; text: "Playlists" }

          Repeater {
            model: searchPage.found.playlists
            delegate: ResultRow {
              required property var modelData
              width: searchPage.width
              theme: root.theme
              fontFamily: root.fontFamily
              title: modelData.name
              subtitle: (modelData.trackCount || 0) + " songs"
              artUrl: modelData.art
              swatch: modelData.color || root.theme.art
              onPicked: root.open({ kind: "playlist", id: modelData.id, name: modelData.name })
            }
          }

          Note {
            visible: searchPage.found.tracks.length + searchPage.found.artists.length
              + searchPage.found.albums.length + searchPage.found.playlists.length === 0
              && root.localStations.length === 0
            topPadding: Style.space(16)
            text: "Nothing matches “" + root.page.query + "”."
          }
        }
      }

      // ---- artist

      Loader {
        width: parent.width
        active: root.page.kind === "artist" && root.info !== null
        visible: active
        sourceComponent: Column {
          id: artistPage
          readonly property var mineAlbums: (artistPage.artist.collected || {}).albums || []
          readonly property var mineSongs: (artistPage.artist.collected || {}).songs || []
          width: parent ? parent.width : 0
          readonly property var artist: root.info
          readonly property var station: root.player.stationNamed(artist.name + " Radio")
          readonly property bool canPlay: artist.topTracks.length > 0 && artist.topTracks[0].playable
          property bool bioOpen: false

          Header {
            caption: "Artist"
            title: artistPage.artist.name
            art: artistPage.artist.image || artistPage.artist.art
            swatch: artistPage.artist.color || root.theme.art

            Label {
              width: parent.width
              visible: text !== ""
              text: (artistPage.artist.traits || []).slice(0, 4).join("  ·  ")
              color: root.theme.muted
              font.pixelSize: Style.font.bodySmall
            }

            Row {
              spacing: Style.space(10)

              Button {
                visible: artistPage.canPlay
                label: "Play"
                glyph: "󰐊"
                onActivated: root.playTrack(artistPage.artist.topTracks[0])
              }

              Button {
                primary: !artistPage.canPlay
                label: root.radioBusy ? "Starting…" : (artistPage.station ? "Play station" : "Start radio")
                glyph: "󰀃"
                enabled: !root.radioBusy
                onActivated: root.startRadio(artistPage.artist.id, artistPage.artist.name)
              }
            }
          }

          // What of theirs is in the listener's collection: why they're in
          // the Artists tab.
          Heading { visible: artistPage.mineAlbums.length + artistPage.mineSongs.length > 0; text: "In your collection" }

          Flow {
            width: parent.width
            visible: artistPage.mineAlbums.length > 0
            spacing: Style.space(18)

            Repeater {
              model: artistPage.mineAlbums
              delegate: Tile {
                required property var modelData
                title: modelData.name
                detail: modelData.year || ""
                art: modelData.art
                swatch: modelData.color || root.theme.art
                removable: true
                onPicked: root.open({ kind: "album", id: modelData.id, name: modelData.name, artist: modelData.artist })
                onRemoveRequested: root.uncollect(modelData)
              }
            }
          }

          Repeater {
            model: artistPage.mineSongs
            delegate: TrackRow {
              required property var modelData
              track: modelData
              showArt: true
              detail: modelData.album
              removable: true
              onPicked: root.playTrack(modelData)
              onRemoveRequested: root.uncollect(modelData)
            }
          }

          Label {
            visible: root.uncollectError !== ""
            width: parent.width
            wrapMode: Text.WordWrap
            text: root.uncollectError
            color: root.theme.accent
            font.pixelSize: Style.font.bodySmall
          }

          Heading { visible: artistPage.artist.topTracks.length > 0; text: "Popular" }

          Repeater {
            model: artistPage.artist.topTracks.slice(0, 5)
            delegate: TrackRow {
              required property var modelData
              track: modelData
              showArt: true
              detail: modelData.album
              onPicked: root.playTrack(modelData)
            }
          }

          Heading { visible: artistPage.artist.bio !== ""; text: "About" }

          Text {
            width: parent.width
            visible: artistPage.artist.bio !== ""
            textFormat: Text.StyledText
            text: artistPage.artist.bio
            wrapMode: Text.WordWrap
            maximumLineCount: artistPage.bioOpen ? 1000 : 4
            elide: Text.ElideRight
            color: root.theme.fg
            linkColor: root.theme.accent
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            lineHeight: 1.3
            onLinkActivated: function (link) {
              if (link.indexOf("AR:") === 0)
                root.open({ kind: "artist", id: link })
              else if (link.indexOf("AL:") === 0)
                root.open({ kind: "album", id: link })
            }

            HoverHandler {
              cursorShape: parent.hoveredLink !== "" ? Qt.PointingHandCursor : Qt.ArrowCursor
            }
          }

          Link {
            visible: artistPage.artist.bio !== ""
            topPadding: Style.space(6)
            text: artistPage.bioOpen ? "Less" : "More"
            idle: root.theme.accent
            font.pixelSize: Style.font.bodySmall
            onClicked: artistPage.bioOpen = !artistPage.bioOpen
          }

          Heading { visible: artistPage.artist.albums.length > 0; text: "Discography" }

          Flow {
            width: parent.width
            spacing: Style.space(18)

            Repeater {
              model: artistPage.artist.albums
              delegate: Tile {
                required property var modelData
                title: modelData.name
                detail: [modelData.year, modelData.type === "OriginalAlbum" ? "Album" : modelData.type]
                  .filter(function (s) { return s }).join(" · ")
                art: modelData.art
                swatch: modelData.color || root.theme.art
                onPicked: root.open({ kind: "album", id: modelData.id, name: modelData.name, artist: modelData.artist })
              }
            }
          }

          Heading { visible: artistPage.artist.similar.length > 0; text: "Fans also like" }

          Flow {
            width: parent.width
            spacing: Style.space(18)

            Repeater {
              model: artistPage.artist.similar.slice(0, 8)
              delegate: Tile {
                required property var modelData
                title: modelData.name
                detail: "Artist"
                art: modelData.art
                swatch: modelData.color || root.theme.art
                onPicked: root.open({ kind: "artist", id: modelData.id, name: modelData.name })
              }
            }
          }
        }
      }

      // ---- album

      Loader {
        width: parent.width
        active: root.page.kind === "album" && root.info !== null
        visible: active
        sourceComponent: Column {
          id: albumPage
          width: parent ? parent.width : 0
          readonly property var album: root.info
          readonly property bool isPlaying: root.player.onDemand && root.player.sourceId === album.id

          Header {
            caption: !albumPage.album.type || albumPage.album.type === "OriginalAlbum" ? "Album" : albumPage.album.type
            title: albumPage.album.name
            art: albumPage.album.art
            swatch: albumPage.album.color || root.theme.art

            Link {
              width: Math.min(implicitWidth, parent.width)
              text: albumPage.album.artist
              font.pixelSize: Style.font.subtitle
              onClicked: root.open({ kind: "artist", id: albumPage.album.artistId, name: albumPage.album.artist })
            }

            Label {
              width: parent.width
              text: [albumPage.album.year, albumPage.album.tracks.length + " songs", root.totalTime(albumPage.album.tracks)]
                .filter(function (s) { return s }).join("  ·  ")
              color: root.theme.muted
              font.pixelSize: Style.font.bodySmall
            }

            Row {
              spacing: Style.space(10)

              Button {
                visible: albumPage.album.playable
                label: albumPage.isPlaying && root.player.playing ? "Pause" : "Play"
                glyph: albumPage.isPlaying && root.player.playing ? "󰏤" : "󰐊"
                onActivated: {
                  if (albumPage.isPlaying)
                    root.player.playPause()
                  else
                    root.player.playOnDemand(albumPage.album.id, "")
                }
              }

              Button {
                primary: !albumPage.album.playable
                label: root.radioBusy ? "Starting…" : "Artist radio"
                glyph: "󰀃"
                enabled: !root.radioBusy
                onActivated: root.startRadio(albumPage.album.artistId, albumPage.album.artist)
              }
            }

            Note {
              visible: !albumPage.album.playable
              text: "Your plan can't play this album on demand. Its songs can start stations."
            }
          }

          Item { width: 1; height: Style.space(20) }

          Repeater {
            model: albumPage.album.tracks
            delegate: TrackRow {
              required property var modelData
              required property int index
              track: modelData
              lead: String(modelData.number || index + 1)
              detail: modelData.artist !== albumPage.album.artist ? modelData.artist : ""
              onPicked: root.playTrack(modelData, albumPage.album.id)
            }
          }
        }
      }

      // ---- playlist

      Loader {
        width: parent.width
        active: root.page.kind === "playlist" && root.info !== null
        visible: active
        sourceComponent: Column {
          id: playlistPage
          width: parent ? parent.width : 0
          readonly property var playlist: root.info
          readonly property bool isPlaying: root.player.onDemand && root.player.sourceId === playlist.id
          readonly property bool anyPlayable: {
            var tracks = playlist.tracks || []
            for (var i = 0; i < tracks.length; i++) {
              if (tracks[i].playable)
                return true
            }
            return false
          }

          Header {
            caption: "Playlist"
            title: playlistPage.playlist.name
            art: playlistPage.playlist.art || (playlistPage.playlist.tracks.length ? playlistPage.playlist.tracks[0].art : "")

            Label {
              text: playlistPage.playlist.trackCount + " songs  ·  " + root.totalTime(playlistPage.playlist.tracks)
              color: root.theme.muted
              font.pixelSize: Style.font.subtitle
            }

            Button {
              visible: playlistPage.anyPlayable
              label: playlistPage.isPlaying && root.player.playing ? "Pause" : "Play"
              glyph: playlistPage.isPlaying && root.player.playing ? "󰏤" : "󰐊"
              onActivated: {
                if (playlistPage.isPlaying)
                  root.player.playPause()
                else
                  root.player.playOnDemand(playlistPage.playlist.id, "")
              }
            }

            Note {
              visible: !playlistPage.anyPlayable
              text: "Your plan can't play playlists on demand."
            }
          }

          Item { width: 1; height: Style.space(20) }

          Repeater {
            model: playlistPage.playlist.tracks
            delegate: TrackRow {
              required property var modelData
              track: modelData
              showArt: true
              detail: modelData.artist + (modelData.album ? "  ·  " + modelData.album : "")
              onPicked: root.playTrack(modelData, playlistPage.playlist.id)
            }
          }
        }
      }
    }
  }
}
