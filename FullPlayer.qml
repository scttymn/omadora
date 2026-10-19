import QtQuick
import Quickshell
import qs.Commons

// The expanded player: stations, the track, and what plays next.
Item {
  id: root

  required property var engine
  required property var player
  required property var theme
  property string fontFamily: Style.font.family
  signal shrinkRequested()

  property string queuePage: "next"
  property string sideTab: "stations"
  property string stationQuery: ""
  property bool moreOpen: false
  property point moreAt: Qt.point(0, 0)
  property bool accountOpen: false
  property point accountAt: Qt.point(0, 0)
  // The window is on screen (set by the bar widget).
  property bool shown: true

  // Two places to be: Music (stations, the player, up next) and Discover
  // (the magazine). Pages from either open under Music.
  property string section: "music"

  function showSection(name) {
    section = name
    if (name === "music")
      showNow()
    else
      loadDiscover(false)
  }

  // The middle column shows the track, or a page from the catalog. Pages
  // stack, so Back retraces artist → album → artist.
  property var page: null
  property var pageStack: []
  readonly property string view: page ? page.kind : "now"

  function open(next) {
    if (page)
      pageStack = pageStack.concat([page])
    page = next
    root.player.createError = ""
  }

  function back() {
    if (pageStack.length === 0) {
      showNow()
      return
    }
    page = pageStack[pageStack.length - 1]
    pageStack = pageStack.slice(0, -1)
  }

  function showNow() {
    page = null
    pageStack = []
  }

  function showSearch(query) {
    var q = String(query || "").trim()
    if (q === "")
      return
    section = "music"
    open({ kind: "search", query: q })
  }

  // Ids when the engine is playing; names when the radio is.
  function showArtist(id, name) {
    if (id || name)
      open({ kind: "artist", id: id || "", name: name || "" })
  }

  function showAlbum(id, name, artist) {
    if (id || name)
      open({ kind: "album", id: id || "", name: name || "", artist: artist || "" })
  }

  function showPlaylist(id, name) {
    open({ kind: "playlist", id: id, name: name || "" })
  }

  function playStation(station) {
    root.player.playStation(station.id, station.name)
    showNow()
  }

  Connections {
    target: root.player
    function onStationCreated() { root.showNow() }
  }

  // The listener's playlists, fetched when the Playlists tab first shows.
  property var playlists: []
  property bool playlistsLoaded: false
  property string playlistsError: ""

  function loadPlaylists() {
    if (playlistsLoaded || !root.engine.signedIn)
      return
    playlistsLoaded = true
    root.engine.request("playlists", {}, function (ok, result) {
      if (ok)
        root.playlists = result
      else {
        root.playlistsError = String(result)
        root.playlistsLoaded = false
      }
    })
  }

  // Discover: magazine articles, matched to albums, songs, and artists.
  property var discover: null
  property bool discoverLoading: false

  function loadDiscover(refresh) {
    if (discoverLoading || !root.engine.signedIn)
      return
    discoverLoading = true
    root.engine.request("discover", { refresh: refresh === true }, function (ok, result) {
      root.discoverLoading = false
      if (ok)
        root.discover = result
    })
  }

  // Matching runs in the engine after the feeds load; check back until done.
  Timer {
    interval: 3000
    repeat: true
    running: root.section === "discover" && root.discover !== null && root.discover.pending > 0
    onTriggered: root.loadDiscover(false)
  }

  // The collection's artists, read each time the tab opens: liking a song
  // can add one.
  property var artists: []
  property bool artistsLoaded: false
  property string artistsError: ""

  function loadArtists() {
    if (!root.engine.signedIn)
      return
    root.engine.request("collectionArtists", {}, function (ok, result) {
      if (ok) {
        root.artists = result
        root.artistsError = ""
      } else {
        root.artistsError = String(result)
      }
      root.artistsLoaded = true
    })
  }

  onSideTabChanged: {
    if (sideTab === "playlists")
      loadPlaylists()
    else if (sideTab === "artists")
      loadArtists()
  }
  Connections {
    target: root.engine
    function onSignedInChanged() {
      // Signed out (or a new account): the playlists were someone else's.
      if (!root.engine.signedIn) {
        root.playlists = []
        root.playlistsLoaded = false
        root.playlistsError = ""
        root.artists = []
        root.artistsLoaded = false
        root.artistsError = ""
        return
      }
      if (root.sideTab === "playlists")
        root.loadPlaylists()
      else if (root.sideTab === "artists")
        root.loadArtists()
      if (root.section === "discover")
        root.loadDiscover(false)
    }
  }

  readonly property string sourceCaption: {
    if (!player.onDemand)
      return player.sourceLabel !== "" ? player.sourceLabel : "Omadora"
    var kind = player.sourceKind === "album" ? "Album" : player.sourceKind === "playlist" ? "Playlist" : "Song"
    return player.sourceLabel !== "" ? kind + "  ·  " + player.sourceLabel : kind
  }
  readonly property bool showNotice: player.adSupported || (player.title === "" && player.notice !== "")
  // As a tiled window the player can get narrow. The queue column goes
  // first, then the station list, so the track always has room.
  readonly property bool showQueue: width >= Style.space(940)
  readonly property bool showStations: width >= Style.space(620)
  readonly property real sideWidth: showStations ? Math.min(Style.space(260), width * 0.24) : 0
  readonly property real queueWidth: showQueue ? Math.min(Style.space(300), width * 0.27) : 0

  readonly property var visibleStations: {
    var query = stationQuery.toLowerCase()
    var all = player.stations || []
    if (!query)
      return all
    var matches = []
    for (var i = 0; i < all.length; i++) {
      if ((all[i].name || "").toLowerCase().indexOf(query) >= 0)
        matches.push(all[i])
    }
    return matches
  }
  readonly property bool shuffling: player.isRadio && engine.state.station && engine.state.station.shuffle === true
  // Your stations whose names match the Pandora search, for its results page.
  readonly property var searchStations: {
    var query = page && page.kind === "search" ? String(page.query || "").toLowerCase() : ""
    var all = player.stations || []
    var matches = []
    for (var i = 0; i < all.length && matches.length < 5; i++) {
      if (query && (all[i].name || "").toLowerCase().indexOf(query) >= 0)
        matches.push(all[i])
    }
    return matches
  }
  readonly property var queue: queuePage === "history" ? player.history : player.upcoming

  // ---- the AI tab: music like the song playing, from an AI agent. It asks
  // only while the tab is on screen, a moment after the song changes (so
  // skipping through songs doesn't ask for each), and keeps each answer.

  property var ai: ({})
  property var aiErrors: ({})
  property string aiAsking: ""
  property var agentInfo: null
  // The model or effort menu: { kind, options: [{ value, label }], at }.
  property var aiMenu: null
  readonly property string aiTrack: root.player.trackId
  // Answers are kept per song and per model and effort.
  readonly property string aiKey: aiTrack === "" || !agentInfo ? "" : [aiTrack, agentInfo.agent, agentInfo.model || "", agentInfo.effort || ""].join("|")
  readonly property var aiNow: ai[aiKey] || null
  readonly property bool aiOnScreen: shown && section === "music" && showQueue && queuePage === "ai"
  readonly property bool hasAgent: agentInfo !== null && !!agentInfo.agent
  readonly property string agentName: agentInfo && agentInfo.name ? agentInfo.name : "the agent"

  onAiOnScreenChanged: {
    if (aiOnScreen) {
      loadAgents()
      aiWait.restart()
    }
  }
  onAiKeyChanged: if (aiOnScreen) aiWait.restart()

  Timer {
    id: aiWait
    interval: 2500
    onTriggered: root.askAi(false)
  }

  // An agent chosen or changed elsewhere (Omarchy's menu, settings.toml)
  // shows up here: soon while there is none, and every half minute after.
  Timer {
    interval: root.hasAgent ? 30000 : 5000
    repeat: true
    running: root.aiOnScreen
    onTriggered: root.loadAgents()
  }

  function loadAgents() {
    root.engine.request("agents", {}, function (ok, result) {
      if (!ok)
        return
      var had = root.hasAgent
      root.agentInfo = result
      if (!had && root.hasAgent)
        aiWait.restart()
    })
  }

  function askAi(fresh) {
    var key = aiKey
    if (!aiOnScreen || key === "" || !root.engine.signedIn || !hasAgent)
      return
    if (!fresh && ai[key])
      return
    if (aiAsking === key)
      return
    aiAsking = key
    var errors = Object.assign({}, aiErrors)
    delete errors[key]
    aiErrors = errors
    root.engine.request("recommend", { fresh: fresh === true }, function (ok, result) {
      if (root.aiAsking === key)
        root.aiAsking = ""
      if (ok) {
        var answers = Object.assign({}, root.ai)
        answers[key] = result
        root.ai = answers
      } else {
        var failed = Object.assign({}, root.aiErrors)
        failed[key] = String(result)
        root.aiErrors = failed
      }
    })
  }

  // Omadora's own model or effort for the agent (null: the agent's own).
  // The default agent itself is never changed here.
  function setAi(kind, value) {
    var args = {}
    args[kind] = value
    root.aiMenu = null
    root.engine.request("setAi", args, function (ok, result) {
      if (ok)
        root.agentInfo = result
    })
  }

  function openAiMenu(kind, label) {
    var info = root.agentInfo
    var options = []
    var own = kind === "model" ? info.modelName : info.effort
    options.push({ value: null, label: info.name + "'s own" + (own && !(kind === "model" ? info.modelChosen : info.effortChosen) ? " (" + own + ")" : "") })
    var list = kind === "model" ? info.models : info.efforts
    for (var i = 0; i < list.length; i++) {
      var item = list[i]
      options.push(kind === "model" ? { value: item.id, label: item.name } : { value: item, label: item })
    }
    var current = kind === "model" ? (info.modelChosen ? info.model : null) : (info.effortChosen ? info.effort : null)
    root.aiMenu = { kind: kind, options: options, current: current, at: label.mapToItem(root, 0, label.height) }
  }

  // A suggestion: play the song (or a station from it, when the plan can't
  // play it on demand), or open the artist.
  function pickSuggestion(item) {
    if (item.kind === "artist")
      showArtist(item.id, item.name)
    else if (item.playable)
      root.player.playOnDemand(item.albumId || item.id, item.id)
    else
      root.player.createStation(item.id)
  }

  component Label: Text {
    textFormat: Text.PlainText
    font.family: root.fontFamily
    elide: Text.ElideRight
  }

  component LinkLabel: Label {
    signal clicked()
    color: linkMouse.containsMouse ? root.theme.accent : root.theme.muted
    font.underline: linkMouse.containsMouse

    MouseArea {
      id: linkMouse
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: parent.clicked()
    }
  }

  // A line in the now-playing "more" menu.
  component MoreItem: Rectangle {
    id: moreItem
    property string label: ""
    signal picked()
    width: parent.width
    height: Style.space(34)
    radius: Style.cornerRadius
    color: itemMouse.containsMouse ? root.theme.raised : "transparent"

    Label {
      anchors.left: parent.left
      anchors.leftMargin: Style.space(12)
      anchors.right: parent.right
      anchors.rightMargin: Style.space(12)
      anchors.verticalCenter: parent.verticalCenter
      text: moreItem.label
      color: itemMouse.containsMouse ? root.theme.accent : root.theme.fg
      font.pixelSize: Style.font.body
    }

    MouseArea {
      id: itemMouse
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: {
        root.moreOpen = false
        moreItem.picked()
      }
    }
  }

  component Caption: Label {
    color: root.theme.faint
    font.pixelSize: Style.font.bodySmall
    font.capitalization: Font.AllUppercase
    font.letterSpacing: 1.3
  }

  component ModeChip: Rectangle {
    property string label: ""
    property string description: ""
    readonly property bool selected: root.player.activeMode === label
    readonly property bool hovered: chipMouse.containsMouse

    width: chipLabel.implicitWidth + Style.space(26)
    height: Style.space(32)
    radius: Style.cornerRadius
    color: selected ? root.theme.accent : "transparent"
    border.color: selected ? root.theme.accent : root.theme.border
    border.width: 1

    Label {
      id: chipLabel
      anchors.centerIn: parent
      text: parent.label
      color: parent.selected ? root.theme.accentText : root.theme.fg
      font.pixelSize: Style.font.bodySmall
    }

    MouseArea {
      id: chipMouse
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: root.player.canSetMode && !parent.selected ? Qt.PointingHandCursor : Qt.ArrowCursor
      onClicked: root.player.setMode(parent.label)
    }
  }

  component StationRow: Rectangle {
    id: row
    property string name: ""
    property string detail: ""
    property string glyph: ""
    property color swatch: "transparent"
    property string art: ""
    property bool current: false
    // Removing asks first, in the row: a station's thumbs go with it.
    property bool removable: false
    property bool confirming: false
    property string problem: ""
    signal picked()
    signal removeRequested()

    height: Style.space(52)
    radius: Style.cornerRadius
    color: confirming ? Util.alpha(root.theme.accent, 0.14)
      : current ? root.theme.raised : (stationHover.hovered ? Util.alpha(root.theme.raised, 0.6) : "transparent")

    ArtMark {
      id: chip
      visible: row.glyph === ""
      anchors.left: parent.left
      anchors.leftMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      width: Style.space(32)
      height: width
      rings: false
      art: row.swatch
      artUrl: row.art
    }

    Label {
      visible: row.glyph !== ""
      anchors.horizontalCenter: chip.horizontalCenter
      anchors.verticalCenter: parent.verticalCenter
      text: row.glyph
      color: root.theme.fg
      font.pixelSize: Style.font.title
    }

    Column {
      anchors.left: chip.right
      anchors.leftMargin: Style.space(12)
      anchors.right: parent.right
      anchors.rightMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(2)

      Label {
        width: parent.width
        text: row.name
        color: row.current ? root.theme.accent : root.theme.fg
        font.pixelSize: Style.font.body
        font.bold: row.current
      }

      Label {
        width: parent.width
        text: row.problem !== "" ? row.problem : row.detail
        color: row.problem !== "" ? root.theme.accent : root.theme.muted
        font.pixelSize: Style.font.caption
        visible: text !== ""
      }
    }

    MouseArea {
      id: rowMouse
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      enabled: !row.confirming
      onClicked: row.picked()
    }

    HoverHandler {
      id: stationHover
    }

    IconButton {
      visible: row.removable && stationHover.hovered && !row.confirming
      anchors.right: parent.right
      anchors.rightMargin: Style.space(4)
      anchors.verticalCenter: parent.verticalCenter
      width: Style.space(28)
      theme: root.theme
      icon: "󰅖"
      iconSize: Style.font.body
      fontFamily: root.fontFamily
      onActivated: {
        row.problem = ""
        row.confirming = true
      }
    }

    // "Remove?" in place of the station, until answered or left.
    Rectangle {
      visible: row.confirming
      anchors.fill: parent
      radius: Style.cornerRadius
      color: root.theme.surface
      border.color: root.theme.accent
      border.width: 1

      Label {
        anchors.left: parent.left
        anchors.leftMargin: Style.space(12)
        anchors.right: confirmActions.left
        anchors.rightMargin: Style.space(8)
        anchors.verticalCenter: parent.verticalCenter
        text: "Remove " + row.name + "?"
        color: root.theme.strong
        font.pixelSize: Style.font.bodySmall
      }

      Row {
        id: confirmActions
        anchors.right: parent.right
        anchors.rightMargin: Style.space(10)
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(12)

        LinkLabel {
          text: "Remove"
          color: root.theme.accent
          font.pixelSize: Style.font.bodySmall
          font.bold: true
          onClicked: {
            row.confirming = false
            row.removeRequested()
          }
        }
        LinkLabel {
          text: "Cancel"
          font.pixelSize: Style.font.bodySmall
          onClicked: row.confirming = false
        }
      }
    }

    onVisibleChanged: confirming = false
  }

  Rectangle {
    anchors.fill: parent
    color: root.theme.bg
    clip: true

    // Header: mark, search, volume, shrink.
    Item {
      id: header
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.top: parent.top
      height: Style.space(52)

      OmadoraLogo {
        id: logo
        anchors.left: parent.left
        anchors.leftMargin: Style.space(18)
        anchors.verticalCenter: parent.verticalCenter
        xHeight: Style.space(13)
        markColor: root.theme.accent
        wordColor: root.theme.strong
      }

      Row {
        anchors.left: logo.right
        anchors.leftMargin: Style.space(36)
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        spacing: Style.space(26)

        Repeater {
          model: [{ key: "music", label: "Music" }, { key: "discover", label: "Discover" }]
          delegate: Item {
            required property var modelData
            readonly property bool selected: root.section === modelData.key
            width: navLabel.implicitWidth
            height: parent.height

            Label {
              id: navLabel
              anchors.verticalCenter: parent.verticalCenter
              text: modelData.label
              color: parent.selected ? root.theme.strong : (navArea.containsMouse ? root.theme.fg : root.theme.muted)
              font.pixelSize: Style.font.body
              font.bold: parent.selected
            }

            // The section you are in is underlined in the accent.
            Rectangle {
              visible: parent.selected
              anchors.bottom: parent.bottom
              width: parent.width
              height: Style.space(2)
              color: root.theme.accent
            }

            MouseArea {
              id: navArea
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: root.showSection(modelData.key)
            }
          }
        }
      }

      Rectangle {
        id: searchBox
        visible: !root.engine.needsAccount
        anchors.right: headerTools.left
        anchors.rightMargin: Style.space(22)
        anchors.verticalCenter: parent.verticalCenter
        width: Math.max(Style.space(160), Math.min(Style.space(320), parent.width - Style.space(560)))
        height: Style.space(34)
        radius: Style.cornerRadius
        color: root.theme.surface

        Label {
          id: searchIcon
          anchors.left: parent.left
          anchors.leftMargin: Style.space(12)
          anchors.verticalCenter: parent.verticalCenter
          text: "󰍉"
          color: root.theme.faint
          font.pixelSize: Style.font.body
        }

        TextInput {
          id: stationSearch
          anchors.left: searchIcon.right
          anchors.leftMargin: Style.space(10)
          anchors.right: parent.right
          anchors.rightMargin: Style.space(12)
          anchors.verticalCenter: parent.verticalCenter
          clip: true
          color: root.theme.fg
          selectionColor: root.theme.accent
          selectedTextColor: root.theme.accentText
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          selectByMouse: true
          onTextChanged: root.stationQuery = text
          onAccepted: root.showSearch(text)
          Keys.onEscapePressed: {
            text = ""
            focus = false
            if (root.view === "search")
              root.showNow()
          }

          Label {
            anchors.verticalCenter: parent.verticalCenter
            visible: stationSearch.text.length === 0
            text: "Search artists, songs, stations"
            color: root.theme.faint
            font.pixelSize: Style.font.body
          }
        }

        MouseArea {
          anchors.fill: parent
          enabled: !stationSearch.activeFocus
          cursorShape: Qt.IBeamCursor
          onClicked: stationSearch.forceActiveFocus()
        }
      }

      Row {
        id: headerTools
        anchors.right: parent.right
        anchors.rightMargin: Style.space(18)
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(10)

        Label {
          visible: root.player.volumeReady
          text: "󰖀"
          color: root.theme.muted
          font.pixelSize: Style.font.body
          anchors.verticalCenter: parent.verticalCenter
        }

        Scrubber {
          visible: root.player.volumeReady
          width: Style.space(96)
          anchors.verticalCenter: parent.verticalCenter
          live: true
          fraction: Math.max(0, Math.min(1, root.player.volume))
          track: root.theme.raised
          fill: root.theme.fg
          onSeeked: function (fraction) { root.player.setVolume(fraction) }
        }

        Item { width: Style.space(12); height: 1 }

        IconButton {
          id: accountButton
          visible: root.engine.signedIn
          theme: root.theme
          icon: "󰀄"
          width: Style.space(30)
          iconSize: Style.font.title
          fontFamily: root.fontFamily
          active: root.accountOpen
          anchors.verticalCenter: parent.verticalCenter
          onActivated: {
            root.accountAt = mapToItem(root, width, height)
            root.accountOpen = !root.accountOpen
          }
        }

        IconButton {
          theme: root.theme
          icon: "󰘕"
          width: Style.space(30)
          iconSize: Style.font.title
          fontFamily: root.fontFamily
          anchors.verticalCenter: parent.verticalCenter
          onActivated: root.shrinkRequested()
        }
      }

      Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: 1
        color: root.theme.surface
      }
    }

    // Left: stations.
    Rectangle {
      id: stationCol
      visible: root.section === "music" && root.showStations
      anchors.top: header.bottom
      anchors.bottom: nowBar.top
      width: root.sideWidth
      color: root.theme.surface

      // Stations | Playlists
      Row {
        id: stationHeading
        anchors.left: parent.left
        anchors.leftMargin: Style.space(20)
        anchors.top: parent.top
        anchors.topMargin: Style.space(20)
        spacing: Style.space(18)

        Repeater {
          model: [{ key: "stations", label: "Stations" }, { key: "playlists", label: "Playlists" }, { key: "artists", label: "Artists" }]
          delegate: Caption {
            required property var modelData
            text: modelData.label
            color: root.sideTab === modelData.key ? root.theme.strong : (tabArea.containsMouse ? root.theme.fg : root.theme.faint)
            font.bold: root.sideTab === modelData.key

            MouseArea {
              id: tabArea
              anchors.fill: parent
              anchors.margins: -Style.space(4)
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: root.sideTab = modelData.key
            }
          }
        }
      }

      ListView {
        id: playlistList
        // Room at the end, clear of the bottom player.
        bottomMargin: Style.space(20)
        visible: root.sideTab === "playlists" && count > 0
        anchors.top: stationHeading.bottom
        anchors.topMargin: Style.space(12)
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Style.space(12)
        anchors.rightMargin: Style.space(12)
        clip: true
        model: root.playlists
        boundsBehavior: Flickable.StopAtBounds

        delegate: ResultRow {
          required property var modelData
          width: playlistList.width
          theme: root.theme
          fontFamily: root.fontFamily
          title: modelData.name || ""
          subtitle: (modelData.trackCount || 0) + " songs"
          artUrl: modelData.art || ""
          swatch: modelData.color || root.theme.art
          current: root.player.onDemand && root.player.sourceId === modelData.id
          onPicked: root.showPlaylist(modelData.id, modelData.name)
        }
      }

      Label {
        visible: root.sideTab === "playlists" && playlistList.count === 0
        anchors.top: stationHeading.bottom
        anchors.topMargin: Style.space(16)
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Style.space(20)
        anchors.rightMargin: Style.space(20)
        wrapMode: Text.WordWrap
        text: {
          if (root.playlistsError !== "")
            return root.playlistsError
          if (!root.engine.online)
            return "Starting the player engine…"
          if (!root.engine.signedIn)
            return "Signing in…"
          return root.playlistsLoaded ? "No playlists yet." : "Loading playlists…"
        }
        color: root.theme.muted
        font.pixelSize: Style.font.bodySmall
      }

      // Artists: the ones behind the collected albums and songs, as in
      // Pandora's own My Collection.
      ListView {
        id: artistList
        // Room at the end, clear of the bottom player.
        bottomMargin: Style.space(20)
        visible: root.sideTab === "artists" && count > 0
        anchors.top: stationHeading.bottom
        anchors.topMargin: Style.space(12)
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Style.space(12)
        anchors.rightMargin: Style.space(12)
        clip: true
        model: root.artists
        boundsBehavior: Flickable.StopAtBounds

        delegate: ResultRow {
          required property var modelData
          width: artistList.width
          theme: root.theme
          fontFamily: root.fontFamily
          title: modelData.name || ""
          subtitle: {
            var parts = []
            if (modelData.albums)
              parts.push(modelData.albums + (modelData.albums === 1 ? " album" : " albums"))
            if (modelData.songs)
              parts.push(modelData.songs + (modelData.songs === 1 ? " song" : " songs"))
            return parts.join(" · ")
          }
          artUrl: modelData.art || ""
          swatch: modelData.color || root.theme.art
          onPicked: root.showArtist(modelData.id, modelData.name)
        }
      }

      Label {
        visible: root.sideTab === "artists" && artistList.count === 0
        anchors.top: stationHeading.bottom
        anchors.topMargin: Style.space(16)
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Style.space(20)
        anchors.rightMargin: Style.space(20)
        wrapMode: Text.WordWrap
        text: {
          if (root.artistsError !== "")
            return root.artistsError
          if (!root.engine.online)
            return "Starting the player engine…"
          if (!root.engine.signedIn)
            return "Signing in…"
          return root.artistsLoaded ? "Artists show up here as you add their albums and songs to your collection." : "Loading artists…"
        }
        color: root.theme.muted
        font.pixelSize: Style.font.bodySmall
      }

      StationRow {
        id: shuffleRow
        visible: root.sideTab === "stations" && root.player.stationsLoaded && root.stationQuery === ""
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: Style.space(12)
        anchors.top: stationHeading.bottom
        anchors.topMargin: Style.space(12)
        height: visible ? Style.space(44) : 0
        glyph: "󰒝"
        name: "Shuffle all"
        current: root.shuffling
        onPicked: root.playStation({ id: "shuffle", name: "Shuffle" })
      }

      ListView {
        id: stationList
        // Room at the end, clear of the bottom player.
        bottomMargin: Style.space(20)
        anchors.top: shuffleRow.visible ? shuffleRow.bottom : stationHeading.bottom
        anchors.topMargin: shuffleRow.visible ? Style.space(2) : Style.space(12)
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Style.space(12)
        anchors.rightMargin: Style.space(12)
        clip: true
        visible: root.sideTab === "stations" && count > 0
        model: root.visibleStations
        boundsBehavior: Flickable.StopAtBounds

        function showCurrent() {
          for (var i = 0; i < root.visibleStations.length; i++) {
            if (root.visibleStations[i].id === root.player.sourceId) {
              positionViewAtIndex(i, ListView.Center)
              return
            }
          }
        }
        onCountChanged: Qt.callLater(showCurrent)
        onVisibleChanged: if (visible) Qt.callLater(showCurrent)
        Connections {
          target: root.player
          function onSourceIdChanged() { Qt.callLater(stationList.showCurrent) }
        }

        delegate: StationRow {
          id: stationRow
          required property var modelData
          required property int index
          width: stationList.width
          name: modelData.name || ""
          detail: modelData.thumbprint ? "Your thumbs up" : ""
          art: modelData.art || ""
          swatch: root.theme.swatch(index)
          current: root.player.isRadio && !root.shuffling && modelData.id === root.player.sourceId
          removable: modelData.canRemove === true
          onPicked: root.playStation(modelData)
          onRemoveRequested: {
            root.player.removeStation(modelData.id, function (ok, error) {
              if (!ok)
                stationRow.problem = error
            })
          }
        }
      }

      Label {
        visible: root.sideTab === "stations" && stationList.count === 0
        anchors.top: shuffleRow.visible ? shuffleRow.bottom : stationHeading.bottom
        anchors.topMargin: Style.space(16)
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Style.space(20)
        anchors.rightMargin: Style.space(20)
        wrapMode: Text.WordWrap
        text: {
          if (root.stationQuery !== "")
            return "No station matches."
          if (!root.engine.online)
            return "Starting the player engine…"
          if (!root.engine.signedIn)
            return root.player.notice !== "" ? root.player.notice : "Signing in…"
          return root.player.stationsLoaded ? "No stations yet. Search for an artist or song to start one." : "Loading stations…"
        }
        color: root.theme.muted
        font.pixelSize: Style.font.bodySmall
      }
    }

    // Right: up next and history.
    Item {
      id: queueCol
      visible: root.section === "music" && root.showQueue
      anchors.top: header.bottom
      anchors.bottom: nowBar.top
      anchors.right: parent.right
      width: root.queueWidth

      Rectangle {
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: 1
        color: root.theme.surface
      }

      Row {
        id: queueTabs
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Style.space(17)
        anchors.topMargin: Style.space(18)

        Repeater {
          model: [
            { key: "next", label: "Up next" },
            { key: "history", label: "History" },
            { key: "ai", label: "AI" }
          ]

          delegate: Rectangle {
            required property var modelData
            readonly property bool selected: root.queuePage === modelData.key
            width: queueTabs.width / 3
            height: Style.space(34)
            radius: Style.cornerRadius
            color: selected ? root.theme.surface : "transparent"

            Label {
              anchors.centerIn: parent
              text: modelData.label
              color: parent.selected ? root.theme.strong : root.theme.faint
              font.pixelSize: Style.font.body
              font.bold: parent.selected
            }

            MouseArea {
              anchors.fill: parent
              cursorShape: Qt.PointingHandCursor
              onClicked: root.queuePage = modelData.key
            }
          }
        }
      }

      ListView {
        id: queueList
        // Room at the end, clear of the bottom player.
        bottomMargin: Style.space(20)
        anchors.top: queueTabs.bottom
        anchors.topMargin: Style.space(20)
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Style.space(21)
        anchors.rightMargin: Style.space(17)
        clip: true
        spacing: Style.space(20)
        visible: count > 0 && root.queuePage !== "ai"
        model: root.queue
        boundsBehavior: Flickable.StopAtBounds

        delegate: Item {
          required property var modelData
          required property int index
          width: queueList.width
          height: Style.space(40)

          MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: {
              if (root.player.onDemand && root.queuePage === "next")
                root.player.playOnDemand(root.player.sourceId, modelData.id)
              else
                root.showAlbum(modelData.albumId || "", modelData.album || "", modelData.artist || "")
            }
          }

          ArtMark {
            id: queueArt
            width: Style.space(40)
            height: width
            rings: false
            art: modelData.color || root.theme.swatch(index + 3)
            artUrl: modelData.art || ""
          }

          Column {
            anchors.left: queueArt.right
            anchors.leftMargin: Style.space(12)
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.space(3)

            Label {
              width: parent.width
              text: modelData.title || modelData.name || ""
              color: root.theme.strong
              font.pixelSize: Style.font.body
            }

            Label {
              width: parent.width
              text: modelData.artist || ""
              color: root.theme.muted
              font.pixelSize: Style.font.caption
              visible: text !== ""
            }
          }
        }
      }

      Label {
        visible: queueList.count === 0 && root.queuePage !== "ai"
        anchors.top: queueTabs.bottom
        anchors.topMargin: Style.space(20)
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Style.space(21)
        anchors.rightMargin: Style.space(17)
        wrapMode: Text.WordWrap
        text: {
          if (root.queuePage === "history")
            return "Songs you've heard show up here."
          if (root.player.onDemand)
            return root.player.sourceKind === "playlist" ? "That's the end of the playlist." : "That's the end of the album."
          return "Up next fills in when something is playing."
        }
        color: root.theme.muted
        font.pixelSize: Style.font.bodySmall
      }

      Item {
        id: aiPanel
        visible: root.queuePage === "ai"
        anchors.top: queueTabs.bottom
        anchors.topMargin: Style.space(20)
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Style.space(21)
        anchors.rightMargin: Style.space(17)

        readonly property string mode: {
          if (!root.engine.signedIn)
            return "signedOut"
          if (root.agentInfo === null)
            return "loading"
          if (!root.hasAgent)
            return "noAgent"
          if (root.aiTrack === "")
            return "idle"
          if (root.aiNow)
            return "picks"
          if (root.aiErrors[root.aiKey])
            return "error"
          return "asking"
        }

        // Which agent answers, with which model and effort (each a menu
        // when the agent lists its choices), and ask again on the right.
        Item {
          id: aiHeader
          visible: aiPanel.mode === "picks" || aiPanel.mode === "error" || aiPanel.mode === "asking" || aiPanel.mode === "idle"
          width: parent.width
          height: Style.space(22)

          Row {
            anchors.left: parent.left
            anchors.right: askAgain.left
            anchors.rightMargin: Style.space(8)
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.space(8)
            clip: true

            Caption {
              text: "From " + root.agentName
              font.pixelSize: Style.font.caption
            }
            Caption {
              visible: modelLink.visible || modelText.visible
              text: "·"
              font.pixelSize: Style.font.caption
            }
            LinkLabel {
              id: modelLink
              visible: root.hasAgent && root.agentInfo.models.length > 0
              text: (root.agentInfo && root.agentInfo.modelName) || "default model"
              font.pixelSize: Style.font.caption
              onClicked: root.openAiMenu("model", modelLink)
            }
            Caption {
              id: modelText
              visible: root.hasAgent && root.agentInfo.models.length === 0 && !!root.agentInfo.modelName
              text: (root.agentInfo && root.agentInfo.modelName) || ""
              font.pixelSize: Style.font.caption
            }
            Caption {
              visible: effortLink.visible || effortText.visible
              text: "·"
              font.pixelSize: Style.font.caption
            }
            // An effort the agent reports but offers no list for (Grok).
            Caption {
              id: effortText
              visible: root.hasAgent && root.agentInfo.efforts.length === 0 && !!root.agentInfo.effort
              text: (root.agentInfo && root.agentInfo.effort) || ""
              font.pixelSize: Style.font.caption
            }
            LinkLabel {
              id: effortLink
              visible: root.hasAgent && root.agentInfo.efforts.length > 0
              text: (root.agentInfo && root.agentInfo.effort) || "default effort"
              font.pixelSize: Style.font.caption
              onClicked: root.openAiMenu("effort", effortLink)
            }
          }

          IconButton {
            id: askAgain
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            visible: aiPanel.mode === "picks" || aiPanel.mode === "error"
            theme: root.theme
            icon: "󰑐"
            width: visible ? Style.space(24) : 0
            iconSize: Style.font.body
            fontFamily: root.fontFamily
            onActivated: root.askAi(true)
          }
        }

        // Status: signed out, thinking, nothing playing, or what went wrong.
        Column {
          visible: aiPanel.mode !== "picks" && aiPanel.mode !== "noAgent"
          anchors.top: aiHeader.visible ? aiHeader.bottom : parent.top
          anchors.topMargin: aiHeader.visible ? Style.space(16) : 0
          width: parent.width
          spacing: Style.space(10)

          Label {
            width: parent.width
            wrapMode: Text.WordWrap
            color: root.theme.muted
            font.pixelSize: Style.font.bodySmall
            text: {
              switch (aiPanel.mode) {
              case "signedOut": return "Sign in, and an AI agent will suggest music like what's playing."
              case "loading": return ""
              case "idle": return "Play something, and " + root.agentName + " will suggest music like it."
              case "error": return root.aiErrors[root.aiKey] || ""
              default: return "Asking " + root.agentName + " for music like “" + root.player.title + "”…"
              }
            }
          }

          LinkLabel {
            visible: aiPanel.mode === "error"
            text: "Try again"
            font.pixelSize: Style.font.bodySmall
            onClicked: root.askAi(true)
          }
        }

        // No default agent: point to Omarchy's, which this tab only uses.
        Column {
          visible: aiPanel.mode === "noAgent"
          width: parent.width
          spacing: Style.space(12)

          Label {
            width: parent.width
            wrapMode: Text.WordWrap
            color: root.theme.fg
            font.pixelSize: Style.font.bodySmall
            text: "Set a default agent in Omarchy, and it will suggest music like what's playing while this tab is open."
          }

          LinkLabel {
            text: "Set a default agent"
            font.pixelSize: Style.font.bodySmall
            onClicked: Quickshell.execDetached(["omarchy-menu", "summon", "setup.default.agent"])
          }
        }

        ListView {
          id: aiList
          // Room at the end, clear of the bottom player.
          bottomMargin: Style.space(20)
          visible: aiPanel.mode === "picks"
          anchors.top: aiHeader.bottom
          anchors.topMargin: Style.space(16)
          anchors.bottom: parent.bottom
          width: parent.width
          clip: true
          spacing: Style.space(18)
          boundsBehavior: Flickable.StopAtBounds
          model: root.aiNow ? root.aiNow.picks : []

          footer: Item {
            width: aiList.width
            height: root.aiNow && root.aiNow.picks.length === 0 ? Style.space(40) : 0

            Label {
              visible: parent.height > 0
              width: parent.width
              wrapMode: Text.WordWrap
              color: root.theme.muted
              font.pixelSize: Style.font.bodySmall
              text: "None of " + root.agentName + "'s suggestions are in the catalog."
            }
          }

          delegate: Item {
            required property var modelData
            required property int index
            readonly property var item: modelData.item
            width: aiList.width
            height: Math.max(Style.space(40), pickText.implicitHeight)

            MouseArea {
              anchors.fill: parent
              cursorShape: Qt.PointingHandCursor
              onClicked: root.pickSuggestion(parent.item)
            }

            ArtMark {
              id: pickArt
              width: Style.space(40)
              height: width
              rings: false
              art: parent.item.color || root.theme.swatch(index + 5)
              artUrl: parent.item.art || ""
            }

            Column {
              id: pickText
              anchors.left: pickArt.right
              anchors.leftMargin: Style.space(12)
              anchors.right: parent.right
              spacing: Style.space(3)

              Label {
                width: parent.width
                text: parent.parent.item.name || parent.parent.item.title || ""
                color: root.theme.strong
                font.pixelSize: Style.font.body
              }

              Label {
                width: parent.width
                text: parent.parent.item.kind === "artist" ? "Artist" : (parent.parent.item.artist || "")
                color: root.theme.muted
                font.pixelSize: Style.font.caption
              }

              Label {
                width: parent.width
                visible: text !== ""
                text: parent.parent.modelData.why || ""
                color: root.theme.faint
                font.pixelSize: Style.font.caption
                wrapMode: Text.WordWrap
                maximumLineCount: 3
              }
            }
          }
        }
      }
    }

    // Middle: the track.
    Item {
      id: stage
      visible: root.section === "music"
      anchors.top: header.bottom
      anchors.bottom: nowBar.top
      anchors.left: stationCol.right
      anchors.right: queueCol.left

      // One factor for the whole column: 1 at the size the mockup was drawn
      // for, growing with the window (by whichever of width or height runs
      // out first) so a big window fills instead of leaving a gap.
      readonly property real fit: Math.max(1, Math.min(1.7,
        (width - Style.space(48)) / Style.space(440),
        height / Style.space(660)))
      readonly property real lane: Math.max(Style.space(340), Math.min(Style.space(440) * fit, width - Style.space(48)))
      readonly property real artSize: Math.max(Style.space(96), Math.min(Style.space(260) * fit, height - Style.space(380) * fit))
      // Text grows more gently than the art, so it never shouts.
      readonly property real textFit: Math.min(fit, 1.4)

      BrowseView {
        anchors.fill: parent
        visible: root.view !== "now"
        engine: root.engine
        player: root.player
        theme: root.theme
        fontFamily: root.fontFamily
        page: root.page || ({ kind: "search", query: "" })
        canGoBack: root.pageStack.length > 0
        localStations: root.view === "search" ? root.searchStations : []
        onBack: root.back()
        onHome: root.showNow()
        onOpen: function (next) { root.open(next) }
        onStationPicked: function (station) { root.playStation(station) }
      }

      Column {
        id: center
        visible: root.view === "now"
        // Centered in the column when there is room; never above the top.
        y: Math.max(Style.space(28), (stage.height - height) / 2)
        anchors.horizontalCenter: parent.horizontalCenter
        width: stage.lane
        spacing: 0

        ArtMark {
          anchors.horizontalCenter: parent.horizontalCenter
          width: stage.artSize
          height: stage.artSize
          artUrl: root.player.artUrl
          art: root.player.track && root.player.track.color ? root.player.track.color : root.theme.art
          ring: root.theme.ring
        }

        Item { width: 1; height: Style.space(20) }

        // The station, or the album or playlist this is playing from.
        Caption {
          width: parent.width
          horizontalAlignment: Text.AlignHCenter
          text: root.sourceCaption
          color: root.theme.accent
          font.pixelSize: Math.round(Style.font.bodySmall * stage.textFit)
          font.underline: sourceArea.containsMouse && sourceArea.enabled

          MouseArea {
            id: sourceArea
            anchors.fill: parent
            enabled: root.player.onDemand && (root.player.sourceKind === "album" || root.player.sourceKind === "playlist")
            hoverEnabled: true
            cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
            onClicked: {
              if (root.player.sourceKind === "album")
                root.showAlbum(root.player.sourceId, root.player.sourceLabel, root.player.artist)
              else
                root.showPlaylist(root.player.sourceId, root.player.sourceLabel)
            }
          }
        }

        Item { width: 1; height: Style.space(10) }

        Label {
          width: parent.width
          horizontalAlignment: Text.AlignHCenter
          text: {
            if (root.player.adSupported)
              return root.player.adNote
            if (root.player.title !== "")
              return root.player.title
            if (root.player.notice !== "")
              return root.player.notice
            return root.player.loading ? "Loading…" : "Nothing playing"
          }
          color: root.showNotice ? root.theme.muted : root.theme.strong
          font.pixelSize: root.showNotice ? Style.font.body : Math.round(Style.font.display * stage.textFit)
          font.bold: !root.showNotice
          wrapMode: root.showNotice ? Text.WordWrap : Text.NoWrap
          maximumLineCount: 4
        }

        // Another device has the account; Pandora's apps offer the same.
        Item {
          width: parent.width
          height: listenHere.height + Style.space(14)
          visible: root.player.busy

          Rectangle {
            id: listenHere
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.bottom: parent.bottom
            width: listenLabel.implicitWidth + Style.space(32)
            height: Style.space(38)
            radius: Style.cornerRadius
            color: listenArea.containsMouse ? Qt.lighter(root.theme.accent, 1.08) : root.theme.accent

            Label {
              id: listenLabel
              anchors.centerIn: parent
              text: "Listen here"
              color: root.theme.accentText
              font.pixelSize: Style.font.body
              font.bold: true
            }

            MouseArea {
              id: listenArea
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: root.player.listenHere()
            }
          }
        }

        Item { width: 1; height: Style.space(8) }

        Row {
          id: creditRow
          anchors.horizontalCenter: parent.horizontalCenter
          spacing: Style.space(10)
          visible: root.player.artist !== "" || root.player.album !== ""
          readonly property real cap: (center.width - Style.space(30)) / 2

          LinkLabel {
            visible: text !== ""
            width: Math.min(implicitWidth, creditRow.cap)
            text: root.player.artist
            font.pixelSize: Math.round(Style.font.subtitle * stage.textFit)
            onClicked: root.showArtist(root.player.artistId, root.player.artist)
          }

          Label {
            visible: root.player.artist !== "" && root.player.album !== ""
            text: "·"
            color: root.theme.muted
            font.pixelSize: Math.round(Style.font.subtitle * stage.textFit)
          }

          LinkLabel {
            visible: text !== ""
            width: Math.min(implicitWidth, creditRow.cap)
            text: root.player.album
            font.pixelSize: Math.round(Style.font.subtitle * stage.textFit)
            onClicked: root.showAlbum(root.player.albumId, root.player.album, root.player.artist)
          }
        }

        Item { width: 1; height: Style.space(18) }

        Scrubber {
          id: progress
          width: parent.width
          barHeight: Math.round(Style.space(3) * Math.min(stage.fit, 1.5))
          fraction: root.player.length > 0 ? root.player.position / root.player.length : 0
          enabled: root.player.canSeek
          track: root.theme.raised
          fill: root.theme.accent
          onSeeked: function (fraction) { root.player.seekFraction(fraction) }
        }

        Item {
          width: parent.width
          height: fullElapsed.implicitHeight

          Label {
            id: fullElapsed
            text: root.player.formatTime(progress.dragging || progress.settling ? progress.shown * root.player.length : root.player.position)
            color: root.theme.muted
            font.pixelSize: Math.round(Style.font.bodySmall * stage.textFit)
            anchors.left: parent.left
          }

          Label {
            text: root.player.formatTime(root.player.length)
            color: root.theme.muted
            font.pixelSize: Math.round(Style.font.bodySmall * stage.textFit)
            anchors.right: parent.right
          }
        }

        Item { width: 1; height: Style.space(16) }

        // Seven controls spread across the lane, pause in the middle.
        Item {
          id: transport
          width: parent.width
          height: Style.space(64) * stage.fit

          // Every button scales with the column; the play button stays the
          // big square in the middle.
          readonly property real slot: Style.space(40) * stage.fit
          readonly property real glyph: Math.round(Style.font.heading * stage.fit)
          readonly property real step: (width - slot) / 6

          IconButton {
            theme: root.theme
            width: transport.slot
            iconSize: transport.glyph
            x: transport.step * 0
            anchors.verticalCenter: parent.verticalCenter
            icon: "󰽥"
            active: root.player.sleepAt > 0
            fontFamily: root.fontFamily
            onActivated: root.player.cycleSleep()

            Label {
              visible: root.player.sleepAt > 0
              anchors.top: parent.bottom
              anchors.horizontalCenter: parent.horizontalCenter
              text: root.player.sleepLeft + "m"
              color: root.theme.accent
              font.pixelSize: Style.font.caption
            }
          }

          IconButton {
            theme: root.theme
            width: transport.slot
            iconSize: transport.glyph
            x: transport.step * 1
            anchors.verticalCenter: parent.verticalCenter
            // Radio rates songs; on demand there is a previous track instead.
            icon: root.player.onDemand ? "󰒮" : (root.player.rating === 2 ? "󰔑" : "󰔒")
            active: !root.player.onDemand && root.player.rating === 2
            enabled: root.player.onDemand ? root.player.canGoBack : root.player.canRate
            fontFamily: root.fontFamily
            onActivated: root.player.onDemand ? root.player.previous() : root.player.thumbDown()
          }

          IconButton {
            theme: root.theme
            width: transport.slot
            iconSize: transport.glyph
            x: transport.step * 2
            anchors.verticalCenter: parent.verticalCenter
            icon: "󰦛"
            enabled: root.player.canReplay
            fontFamily: root.fontFamily
            onActivated: root.player.replay()
          }

          IconButton {
            theme: root.theme
            x: transport.step * 3 + (transport.slot - width) / 2
            anchors.verticalCenter: parent.verticalCenter
            width: Style.space(64) * stage.fit
            icon: root.player.loading ? "󰔟" : (root.player.playing ? "󰏤" : "󰐊")
            primary: true
            iconSize: Math.round(Style.font.iconLarge * 1.2 * stage.fit)
            fontFamily: root.fontFamily
            onActivated: root.player.playPause()
          }

          IconButton {
            theme: root.theme
            width: transport.slot
            iconSize: transport.glyph
            x: transport.step * 4
            anchors.verticalCenter: parent.verticalCenter
            icon: "󰒭"
            enabled: root.player.canGoNext
            fontFamily: root.fontFamily
            onActivated: root.player.next()
          }

          IconButton {
            theme: root.theme
            width: transport.slot
            iconSize: transport.glyph
            x: transport.step * 5
            anchors.verticalCenter: parent.verticalCenter
            icon: root.player.liked ? "󰔓" : "󰔔"
            active: root.player.liked
            enabled: root.player.canLike
            fontFamily: root.fontFamily
            onActivated: root.player.thumbUp()
          }

          IconButton {
            theme: root.theme
            width: transport.slot
            iconSize: transport.glyph
            x: transport.step * 6
            anchors.verticalCenter: parent.verticalCenter
            icon: "󰇘"
            active: root.moreOpen
            enabled: root.player.trackId !== ""
            fontFamily: root.fontFamily
            onActivated: {
              root.moreAt = mapToItem(root, width, height)
              root.moreOpen = !root.moreOpen
            }
          }
        }

        Item { width: 1; height: Style.space(18) }

        // Flow cannot center its lines, so center the block it fills.
        Item {
          width: parent.width
          height: modes.height
          visible: root.player.isRadio && root.player.modes.length > 0
          opacity: root.player.pendingMode !== "" ? 0.6 : 1

          Flow {
            id: modes
            width: parent.width
            x: Math.max(0, (parent.width - childrenRect.width) / 2)
            spacing: Style.space(8)

            Repeater {
              id: modeChips
              model: root.player.modes
              delegate: ModeChip {
                required property var modelData
                label: modelData.name || ""
                description: modelData.description || ""
              }
            }
          }
        }

        Item { width: 1; height: Style.space(12); visible: modeHint.visible }

        // What the hovered mode does, or the one that is playing.
        Label {
          id: modeHint
          width: parent.width
          visible: root.player.isRadio && root.player.modes.length > 0
          horizontalAlignment: Text.AlignHCenter
          wrapMode: Text.WordWrap
          maximumLineCount: 2
          color: root.theme.muted
          font.pixelSize: Style.font.bodySmall
          text: {
            var active = ""
            for (var i = 0; i < modeChips.count; i++) {
              var chip = modeChips.itemAt(i)
              if (!chip)
                continue
              if (chip.hovered)
                return chip.description
              if (chip.selected)
                active = chip.description
            }
            return active
          }
        }
      }
    }

    DiscoverView {
      anchors.top: header.bottom
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.bottom: nowBar.top
      visible: root.section === "discover"
      engine: root.engine
      player: root.player
      theme: root.theme
      fontFamily: root.fontFamily
      discover: root.discover
      loading: root.discoverLoading
      onRefresh: root.loadDiscover(true)
      onOpen: function (next) {
        root.section = "music"
        root.open(next)
      }
    }

    // Along the bottom wherever the now-playing view isn't (Discover, or a
    // page under Music), while a song is loaded: a way back to it.
    NowPlayingBar {
      id: nowBar
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.bottom: parent.bottom
      wanted: root.section === "discover" || root.view !== "now"
      player: root.player
      theme: root.theme
      fontFamily: root.fontFamily
      onShowPlayer: {
        root.section = "music"
        root.showNow()
      }
    }

    SignInView {
      anchors.top: header.bottom
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.bottom: parent.bottom
      visible: root.engine.needsAccount
      engine: root.engine
      theme: root.theme
      fontFamily: root.fontFamily
    }

    // The overflow menu under the "more" button.
    MouseArea {
      anchors.fill: parent
      visible: root.moreOpen
      onClicked: root.moreOpen = false
    }

    // The AI tab's model or effort menu.
    MouseArea {
      anchors.fill: parent
      visible: root.aiMenu !== null
      onClicked: root.aiMenu = null
    }

    Rectangle {
      visible: root.aiMenu !== null
      x: root.aiMenu ? Math.max(Style.space(8), Math.min(parent.width - width - Style.space(8), root.aiMenu.at.x)) : 0
      y: root.aiMenu ? root.aiMenu.at.y + Style.space(6) : 0
      width: Style.space(240)
      height: Math.min(aiMenuList.contentHeight + Style.space(12), parent.height - y - Style.space(12))
      radius: Style.cornerRadius
      color: root.theme.surface
      border.color: root.theme.border
      border.width: 1

      ListView {
        id: aiMenuList
        anchors.fill: parent
        anchors.margins: Style.space(6)
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        model: root.aiMenu ? root.aiMenu.options : []

        delegate: Rectangle {
          required property var modelData
          readonly property bool selected: root.aiMenu !== null && root.aiMenu.current === modelData.value
          width: aiMenuList.width
          height: Style.space(32)
          radius: Style.cornerRadius
          color: optionMouse.containsMouse ? root.theme.raised : "transparent"

          Label {
            anchors.left: parent.left
            anchors.leftMargin: Style.space(10)
            anchors.right: parent.right
            anchors.rightMargin: Style.space(10)
            anchors.verticalCenter: parent.verticalCenter
            text: modelData.label
            color: parent.selected ? root.theme.accent : (optionMouse.containsMouse ? root.theme.strong : root.theme.fg)
            font.pixelSize: Style.font.bodySmall
            font.bold: parent.selected
          }

          MouseArea {
            id: optionMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.setAi(root.aiMenu.kind, modelData.value)
          }
        }
      }
    }

    // The account menu: who is signed in, and Sign out.
    MouseArea {
      anchors.fill: parent
      visible: root.accountOpen
      onClicked: root.accountOpen = false
    }

    Rectangle {
      visible: root.accountOpen && root.engine.signedIn
      x: Math.min(parent.width - width - Style.space(8), root.accountAt.x - width)
      y: root.accountAt.y + Style.space(6)
      width: Math.max(accountLabel.implicitWidth, signOutLabel.implicitWidth) + Style.space(28)
      height: accountColumn.implicitHeight + Style.space(16)
      radius: Style.cornerRadius
      color: root.theme.surface
      border.color: root.theme.border
      border.width: 1

      Column {
        id: accountColumn
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter

        Label {
          id: accountLabel
          visible: root.engine.user !== ""
          x: Style.space(14)
          height: Style.space(32)
          verticalAlignment: Text.AlignVCenter
          text: "Signed in as " + root.engine.user
          color: root.theme.muted
          font.pixelSize: Style.font.bodySmall
        }

        Rectangle {
          width: parent.width
          height: Style.space(34)
          color: signOutMouse.containsMouse ? root.theme.raised : "transparent"

          Label {
            id: signOutLabel
            x: Style.space(14)
            anchors.verticalCenter: parent.verticalCenter
            text: "Sign out"
            color: signOutMouse.containsMouse ? root.theme.accent : root.theme.fg
            font.pixelSize: Style.font.body
          }

          MouseArea {
            id: signOutMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: {
              root.accountOpen = false
              root.showNow()
              root.engine.signOut()
            }
          }
        }
      }
    }

    // The "more" menu: radio from this song or its artist, and on radio,
    // tired of this song.
    Rectangle {
      visible: root.moreOpen
      x: Math.min(parent.width - width - Style.space(8), root.moreAt.x - width)
      y: root.moreAt.y + Style.space(6)
      width: Math.min(Style.space(320), Math.max(songRadio.implicitWidth, artistRadio.implicitWidth) + Style.space(40))
      height: moreColumn.implicitHeight + Style.space(12)
      radius: Style.cornerRadius
      color: root.theme.surface
      border.color: root.theme.border
      border.width: 1

      Label {
        id: songRadio
        visible: false
        text: "Start radio from this song"
        font.pixelSize: Style.font.body
      }
      Label {
        id: artistRadio
        visible: false
        text: "Start radio from " + root.player.artist
        font.pixelSize: Style.font.body
      }

      Column {
        id: moreColumn
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: Style.space(6)
        anchors.verticalCenter: parent.verticalCenter

        MoreItem {
          label: songRadio.text
          onPicked: root.player.radioFrom(root.player.trackId, root.player.title)
        }
        MoreItem {
          visible: root.player.artistId !== ""
          label: artistRadio.text
          onPicked: root.player.radioFrom(root.player.artistId, root.player.artist)
        }
        MoreItem {
          visible: root.player.canTire
          label: "Tired of this song"
          onPicked: root.player.tired()
        }
      }
    }
  }
}
