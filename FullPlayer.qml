import QtQuick
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
    if (q !== "")
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

  onSideTabChanged: if (sideTab === "playlists") loadPlaylists()
  Connections {
    target: root.engine
    function onSignedInChanged() {
      if (root.sideTab === "playlists")
        root.loadPlaylists()
    }
  }

  readonly property string sourceCaption: {
    if (!player.onDemand)
      return player.sourceLabel !== "" ? player.sourceLabel : "Pandora"
    var kind = player.sourceKind === "album" ? "Album" : player.sourceKind === "playlist" ? "Playlist" : "Song"
    return player.sourceLabel !== "" ? kind + "  ·  " + player.sourceLabel : kind
  }
  readonly property bool showNotice: player.title === "" && player.notice !== ""
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
    signal picked()

    height: Style.space(52)
    radius: Style.cornerRadius
    color: current ? root.theme.raised : (rowMouse.containsMouse ? Util.alpha(root.theme.raised, 0.6) : "transparent")

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
        text: row.detail
        color: root.theme.muted
        font.pixelSize: Style.font.caption
        visible: text !== ""
      }
    }

    MouseArea {
      id: rowMouse
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: row.picked()
    }
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
        anchors.left: parent.left
        anchors.leftMargin: Style.space(18)
        anchors.verticalCenter: parent.verticalCenter
        xHeight: Style.space(13)
        markColor: root.theme.accent
        wordColor: root.theme.strong
      }

      Rectangle {
        x: root.showStations ? root.sideWidth : Style.space(140)
        anchors.verticalCenter: parent.verticalCenter
        width: Math.max(Style.space(120), Math.min(Style.space(320), parent.width - x - Style.space(220)))
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
      visible: root.showStations
      anchors.top: header.bottom
      anchors.bottom: parent.bottom
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
          model: [{ key: "stations", label: "Stations" }, { key: "playlists", label: "Playlists" }]
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
            return "Signing in to Pandora…"
          return root.playlistsLoaded ? "No playlists yet." : "Loading playlists…"
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
          required property var modelData
          required property int index
          width: stationList.width
          name: modelData.name || ""
          detail: modelData.thumbprint ? "Your thumbs up" : ""
          art: modelData.art || ""
          swatch: root.theme.swatch(index)
          current: root.player.isRadio && !root.shuffling && modelData.id === root.player.sourceId
          onPicked: root.playStation(modelData)
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
            return root.player.notice !== "" ? root.player.notice : "Signing in to Pandora…"
          return root.player.stationsLoaded ? "No stations yet. Search for an artist or song to start one." : "Loading stations…"
        }
        color: root.theme.muted
        font.pixelSize: Style.font.bodySmall
      }
    }

    // Right: up next and history.
    Item {
      id: queueCol
      visible: root.showQueue
      anchors.top: header.bottom
      anchors.bottom: parent.bottom
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
            { key: "history", label: "History" }
          ]

          delegate: Rectangle {
            required property var modelData
            readonly property bool selected: root.queuePage === modelData.key
            width: queueTabs.width / 2
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
        anchors.top: queueTabs.bottom
        anchors.topMargin: Style.space(20)
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Style.space(21)
        anchors.rightMargin: Style.space(17)
        clip: true
        spacing: Style.space(20)
        visible: count > 0
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
        visible: queueList.count === 0
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
    }

    // Middle: the track.
    Item {
      id: stage
      anchors.top: header.bottom
      anchors.bottom: parent.bottom
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
            icon: root.player.onDemand ? "󰒮" : "󰔒"
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
            icon: "󰔔"
            active: root.player.rating === 1
            enabled: root.player.canRate
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
            enabled: root.player.canTire
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

    // The overflow menu under the "more" button.
    MouseArea {
      anchors.fill: parent
      visible: root.moreOpen
      onClicked: root.moreOpen = false
    }

    Rectangle {
      visible: root.moreOpen
      x: Math.min(parent.width - width - Style.space(8), root.moreAt.x - width)
      y: root.moreAt.y + Style.space(6)
      width: tiredLabel.implicitWidth + Style.space(28)
      height: Style.space(36)
      radius: Style.cornerRadius
      color: root.theme.surface
      border.color: root.theme.border
      border.width: 1

      Label {
        id: tiredLabel
        anchors.centerIn: parent
        text: "Tired of this song"
        color: tiredMouse.containsMouse ? root.theme.accent : root.theme.fg
        font.pixelSize: Style.font.body
      }

      MouseArea {
        id: tiredMouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: {
          root.moreOpen = false
          root.player.tired()
        }
      }
    }
  }
}
