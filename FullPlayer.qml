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
  signal closeRequested()

  property string queuePage: "next"
  property bool moreOpen: false
  property point moreAt: Qt.point(0, 0)
  property bool accountOpen: false
  property point accountAt: Qt.point(0, 0)
  // The window is on screen (set by the bar widget).
  property bool shown: true

  // Four places in the header: Discover (the magazine, and where the
  // player opens), Browse, Music (your stations, playlists, and artists),
  // and Podcasts. The track, with up next beside it, is a fifth, reached
  // from the player along the bottom.
  property string section: "discover"
  Component.onCompleted: loadDiscover(false)

  // The engine connected: the magazine it saved can show already.
  Connections {
    target: root.engine
    function onOnlineChanged() {
      if (root.engine.online && root.section === "discover")
        root.loadDiscover(false)
    }
  }

  // Where each place starts: null is the track (Now Playing) or the
  // magazine (Discover).
  function start(name) {
    if (name === "collection")
      return { kind: "collection" }
    if (name === "browse")
      return { kind: "browse" }
    if (name === "podcasts")
      return { kind: "podcasts" }
    return null
  }

  // Each place keeps its own pages, so going back to one finds you where you
  // left it. Now Playing always opens on the track, and choosing the place
  // you are in takes you back to its start.
  property var places: ({})

  function showSection(name) {
    if (name !== section) {
      var kept = Object.assign({}, places)
      kept[section] = { page: page, pageStack: pageStack }
      places = kept
    }
    var mine = name === section || name === "now" ? null : places[name]
    section = name
    page = mine ? mine.page : start(name)
    pageStack = mine ? mine.pageStack : []
    if (name === "discover")
      loadDiscover(false)
  }

  // Pages stack, so Back retraces artist → album → artist.
  property var page: null
  property var pageStack: []
  readonly property string view: page ? page.kind : "now"

  function open(next) {
    if (page)
      pageStack = pageStack.concat([page])
    page = next
    root.player.createError = ""
  }

  // Open a page in a given place, such as the collection's own lists.
  function openIn(name, next) {
    if (section !== name)
      showSection(name)
    open(next)
  }

  function back() {
    if (pageStack.length === 0) {
      page = start(section)
      return
    }
    page = pageStack[pageStack.length - 1]
    pageStack = pageStack.slice(0, -1)
  }

  function showNow() {
    showSection("now")
  }

  function showSearch(query) {
    var q = String(query || "").trim()
    if (q === "")
      return
    // Under Podcasts, search finds shows (or a feed by its address).
    open({ kind: section === "podcasts" ? "podcastSearch" : "search", query: q })
  }

  function showShow(key, name) {
    if (key)
      open({ kind: "show", key: key, name: name || "" })
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

  // Discover: magazine articles, matched to albums, songs, and artists.
  property var discover: null
  property bool discoverLoading: false

  // What was saved shows at once; the fresh magazine replaces it.
  function loadDiscover(refresh) {
    if (discoverLoading)
      return
    discoverLoading = root.engine.signedIn
    root.engine.requestPage("discover", { refresh: refresh === true }, function (ok, result, fresh) {
      if (fresh)
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

  // Signed out (not just the engine restarting): the pages were that
  // account's, and signing in starts over on Discover.
  Connections {
    target: root.engine
    function onNeedsAccountChanged() {
      if (!root.engine.needsAccount)
        return
      root.places = ({})
      root.section = "discover"
      root.page = null
      root.pageStack = []
    }
  }

  Connections {
    target: root.engine
    function onSignedInChanged() {
      if (!root.engine.signedIn)
        return
      if (root.section === "discover")
        root.loadDiscover(false)
    }
  }

  readonly property string sourceCaption: {
    if (player.isPodcast)
      return "Podcast  ·  " + player.artist
    if (!player.onDemand)
      return player.sourceLabel !== "" ? player.sourceLabel : "Omadora"
    var kind = player.sourceKind === "album" ? "Album" : player.sourceKind === "playlist" ? "Playlist" : "Song"
    return player.sourceLabel !== "" ? kind + "  ·  " + player.sourceLabel : kind
  }
  readonly property bool showNotice: player.title === "" && player.notice !== ""
  // As a tiled window the player can get narrow. The queue column goes
  // first, so the track always has room.
  readonly property bool showQueue: width >= Style.space(720)
  readonly property real queueWidth: showQueue ? Math.min(Style.space(320), width * 0.3) : 0

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
  readonly property bool aiOnScreen: shown && section === "now" && showQueue && queuePage === "ai"
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
    if (item.kind === "show")
      showShow(item.key, item.name)
    else if (item.kind === "artist")
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

  Rectangle {
    anchors.fill: parent
    color: root.theme.bg
    clip: true

    // Header: mark, search, volume, close.
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
        id: nav
        anchors.left: logo.right
        anchors.leftMargin: Style.space(36)
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        spacing: Style.space(26)

        Repeater {
          model: [
            { key: "discover", label: "Discover" },
            { key: "browse", label: "Browse" },
            { key: "collection", label: "Music" },
            { key: "podcasts", label: "Podcasts" }
          ]
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
        // What's left between the places and the tools; none in a narrow
        // window, where the places come first.
        readonly property real room: headerTools.x - Style.space(22) - (nav.x + nav.width) - Style.space(28)
        visible: !root.engine.needsAccount && room >= Style.space(140)
        anchors.right: headerTools.left
        anchors.rightMargin: Style.space(22)
        anchors.verticalCenter: parent.verticalCenter
        width: Math.min(Style.space(320), room)
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
          onAccepted: root.showSearch(text)
          Keys.onEscapePressed: {
            text = ""
            focus = false
            if (root.view === "search")
              root.back()
          }

          Label {
            anchors.verticalCenter: parent.verticalCenter
            visible: stationSearch.text.length === 0
            text: root.section === "podcasts" ? "Search podcasts, or paste a feed's address" : "Search artists, songs, stations"
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
          onActivated: root.closeRequested()
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

    // Right: up next and history.
    Item {
      id: queueCol
      visible: root.section === "now" && root.showQueue
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
              if (root.player.stepsThrough && root.queuePage === "next")
                root.player.playOnDemand(root.player.sourceId, modelData.id)
              else if (modelData.kind === "episode")
                root.player.playEpisode(modelData.show, modelData.id)
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
            return root.player.isPodcast ? "Episodes you've listened to show up here." : "Songs you've heard show up here."
          if (root.player.isPodcast)
            return "No more new episodes. Follow shows, and theirs line up here."
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
              default: return root.player.isPodcast
                ? "Asking " + root.agentName + " for podcasts like “" + root.player.artist + "”…"
                : "Asking " + root.agentName + " for music like “" + root.player.title + "”…"
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
      visible: root.section === "now" && root.page === null
      anchors.top: header.bottom
      anchors.bottom: nowBar.top
      anchors.left: parent.left
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
            enabled: root.player.isPodcast || root.player.onDemand && (root.player.sourceKind === "album" || root.player.sourceKind === "playlist")
            hoverEnabled: true
            cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
            onClicked: {
              if (root.player.isPodcast)
                root.showShow(root.player.showKey, root.player.artist)
              else if (root.player.sourceKind === "album")
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
            onClicked: {
              if (root.player.isPodcast)
                root.showShow(root.player.showKey, root.player.artist)
              else
                root.showArtist(root.player.artistId, root.player.artist)
            }
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
            icon: root.player.stepsThrough ? "󰒮" : (root.player.rating === 2 ? "󰔑" : "󰔒")
            active: !root.player.stepsThrough && root.player.rating === 2
            enabled: root.player.stepsThrough ? root.player.canGoBack : root.player.canRate
            fontFamily: root.fontFamily
            onActivated: root.player.stepsThrough ? root.player.previous() : root.player.thumbDown()
          }

          IconButton {
            theme: root.theme
            width: transport.slot
            iconSize: transport.glyph
            x: transport.step * 2
            anchors.verticalCenter: parent.verticalCenter
            // A podcast goes back 15 seconds instead.
            icon: root.player.isPodcast ? "󱥆" : "󰦛"
            enabled: root.player.canReplay
            fontFamily: root.fontFamily
            onActivated: root.player.isPodcast ? root.player.skip(-15) : root.player.replay()
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
            // A podcast goes ahead 30 seconds here, and on to the next episode
            // where the thumbs up would be.
            icon: root.player.isPodcast ? "󰴆" : "󰒭"
            enabled: root.player.isPodcast ? root.player.canSeek : root.player.canGoNext
            fontFamily: root.fontFamily
            onActivated: root.player.isPodcast ? root.player.skip(30) : root.player.next()
          }

          IconButton {
            theme: root.theme
            width: transport.slot
            iconSize: transport.glyph
            x: transport.step * 5
            anchors.verticalCenter: parent.verticalCenter
            icon: root.player.isPodcast ? "󰒭" : (root.player.liked ? "󰔓" : "󰔔")
            active: !root.player.isPodcast && root.player.liked
            enabled: root.player.isPodcast ? root.player.canGoNext : root.player.canLike
            fontFamily: root.fontFamily
            onActivated: root.player.isPodcast ? root.player.next() : root.player.thumbUp()
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

    // A page: beside up next under Now Playing, else the whole window. Each
    // place has its own view, hidden rather than destroyed while another is
    // chosen, so going back finds its page with the pictures still decoded
    // (Qt keeps few it no longer shows).
    Repeater {
      model: ["now", "discover", "browse", "collection", "podcasts"]

      delegate: BrowseView {
        required property string modelData
        readonly property bool here: root.section === modelData
        readonly property var placePage: here ? root.page : (root.places[modelData] ? root.places[modelData].page : null)
        anchors.top: header.bottom
        anchors.bottom: nowBar.top
        anchors.left: parent.left
        anchors.right: modelData === "now" ? queueCol.left : parent.right
        visible: here && root.page !== null
        engine: root.engine
        player: root.player
        theme: root.theme
        fontFamily: root.fontFamily
        page: placePage || ({ kind: "search", query: "" })
        canGoBack: here && (root.pageStack.length > 0 || root.start(root.section) === null)
        localStations: here && root.view === "search" ? root.searchStations : []
        onBack: root.back()
        onOpen: function (next) { root.open(next) }
        onStationPicked: function (station) { root.playStation(station) }
      }
    }

    DiscoverView {
      anchors.top: header.bottom
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.bottom: nowBar.top
      visible: root.section === "discover" && root.page === null
      engine: root.engine
      player: root.player
      theme: root.theme
      fontFamily: root.fontFamily
      discover: root.discover
      loading: root.discoverLoading
      onRefresh: root.loadDiscover(true)
      onOpen: function (next) { root.open(next) }
    }

    // Along the bottom wherever the track isn't on screen, while a song is
    // loaded: a way back to it.
    NowPlayingBar {
      id: nowBar
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.bottom: parent.bottom
      wanted: !stage.visible
      player: root.player
      theme: root.theme
      fontFamily: root.fontFamily
      onShowPlayer: root.showNow()
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
              root.engine.signOut()
            }
          }
        }
      }
    }

    // The "more" menu: radio from this song or its artist, and on radio,
    // tired of this song; for an episode, its show, or done with it.
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
          visible: !root.player.isPodcast
          label: songRadio.text
          onPicked: root.player.radioFrom(root.player.trackId, root.player.title)
        }
        MoreItem {
          visible: root.player.isPodcast
          label: "Go to the show"
          onPicked: root.showShow(root.player.showKey, root.player.artist)
        }
        MoreItem {
          visible: root.player.isPodcast
          label: "Mark played"
          onPicked: {
            root.engine.request("markPlayed", { id: root.player.trackId, played: true })
            if (root.player.canGoNext)
              root.player.next()
          }
        }
        MoreItem {
          visible: !root.player.isPodcast && root.player.artistId !== ""
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
