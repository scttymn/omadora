import QtQuick
import qs.Commons

// Discover: music magazines' latest articles as a magazine page. Articles
// with a picture are cards in a grid; text-only ones (Rolling Stone's news)
// are a Headlines rail beside it. Chips are the album, song, or artist an
// article is about on Pandora; the headline opens the article itself in the
// browser.
Item {
  id: root

  required property var engine
  required property var player
  required property var theme
  property string fontFamily: Style.font.family

  // From the engine's `discover` command: feeds, items, pending, errors.
  property var discover: null
  property bool loading: false
  property string feed: ""

  signal open(var page)
  signal refresh()
  signal showPlayer()

  readonly property var items: {
    var all = discover ? discover.items : []
    if (feed === "")
      return all
    return all.filter(function (i) { return i.feed === feed })
  }
  // Filters pick among the feeds with pictures; text-only stories are all
  // in the Headlines rail, whatever the filter.
  readonly property var withPictures: items.filter(function (i) { return i.image !== "" })
  readonly property var headlines: (discover ? discover.items : []).filter(function (i) { return i.image === "" })
  readonly property var pictureFeeds: {
    if (!discover)
      return []
    return discover.feeds.filter(function (f) {
      return discover.items.some(function (i) { return i.feed === f.name && i.image !== "" })
    })
  }
  readonly property int pending: discover ? discover.pending : 0

  function ago(ms) {
    if (!ms)
      return ""
    var s = (Date.now() - ms) / 1000
    if (s < 3600)
      return Math.max(1, Math.round(s / 60)) + " min ago"
    if (s < 86400)
      return Math.round(s / 3600) + " hr ago"
    var d = Math.round(s / 86400)
    if (d === 1)
      return "yesterday"
    if (d < 7)
      return d + " days ago"
    return new Date(ms).toLocaleDateString(Qt.locale(), "MMM d")
  }

  // "Pitchfork: Best New Albums" → magazine and section.
  function source(name) {
    var at = name.indexOf(": ")
    return at < 0 ? { magazine: name, section: "" } : { magazine: name.slice(0, at), section: name.slice(at + 2) }
  }

  // An article's chips: what it is about, then who made it.
  function chips(item) {
    var out = []
    var seenArtists = {}
    for (var i = 0; i < item.matches.length; i++) {
      var m = item.matches[i]
      if (m.kind === "album")
        out.push({ kind: "album", label: m.name, id: m.id, name: m.name, artist: m.artist, art: m.art })
      else if (m.kind === "track")
        out.push({ kind: "track", label: m.name, id: m.id, albumId: m.albumId, album: m.album, artist: m.artist, art: m.art })
      if (m.kind === "artist" && !seenArtists[m.id]) {
        seenArtists[m.id] = true
        out.push({ kind: "artist", label: m.name, id: m.id, name: m.name, art: m.art })
      } else if (m.artistId && !seenArtists[m.artistId]) {
        seenArtists[m.artistId] = true
        out.push({ kind: "artist", label: m.artist, id: m.artistId, name: m.artist, art: "" })
      }
    }
    return out
  }

  function follow(chip) {
    if (chip.kind === "album")
      root.open({ kind: "album", id: chip.id, name: chip.name, artist: chip.artist })
    else if (chip.kind === "track")
      root.open({ kind: "album", id: chip.albumId, name: chip.album, artist: chip.artist, track: chip.id })
    else
      root.open({ kind: "artist", id: chip.id, name: chip.name })
  }

  component Label: Text {
    textFormat: Text.PlainText
    font.family: root.fontFamily
    elide: Text.ElideRight
  }

  component FilterChip: Rectangle {
    id: filter
    property string label: ""
    property bool selected: false
    signal picked()

    width: filterLabel.implicitWidth + Style.space(24)
    height: Style.space(32)
    radius: Style.cornerRadius
    color: selected ? root.theme.accent : (filterArea.containsMouse ? root.theme.raised : "transparent")
    border.color: selected ? root.theme.accent : root.theme.border
    border.width: 1

    Label {
      id: filterLabel
      anchors.centerIn: parent
      text: filter.label
      color: filter.selected ? root.theme.accentText : root.theme.fg
      font.pixelSize: Style.font.bodySmall
    }

    MouseArea {
      id: filterArea
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: filter.picked()
    }
  }

  // What an article is about: an album, song, or artist on Pandora.
  component MatchChip: Rectangle {
    id: chip
    property var info: ({})
    signal picked()

    width: Math.min(chipRow.implicitWidth + Style.space(16), parent ? parent.width : 9999)
    height: Style.space(28)
    radius: Style.cornerRadius
    color: chipArea.containsMouse ? root.theme.raised : Util.alpha(root.theme.raised, 0.45)
    clip: true

    Row {
      id: chipRow
      anchors.left: parent.left
      anchors.leftMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(6)

      Label {
        anchors.verticalCenter: parent.verticalCenter
        text: chip.info.kind === "album" ? "󰀥" : chip.info.kind === "track" ? "󰝚" : "󰠃"
        color: root.theme.accent
        font.pixelSize: Style.font.body
      }

      Label {
        anchors.verticalCenter: parent.verticalCenter
        width: Math.min(implicitWidth, Style.space(220))
        text: chip.info.label || ""
        color: chipArea.containsMouse ? root.theme.strong : root.theme.fg
        font.pixelSize: Style.font.bodySmall
      }
    }

    MouseArea {
      id: chipArea
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: chip.picked()
    }
  }

  Rectangle {
    anchors.fill: parent
    color: root.theme.bg
  }

  // ---- filters and status

  Item {
    id: bar
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.top: parent.top
    anchors.leftMargin: Style.space(28)
    anchors.rightMargin: Style.space(28)
    height: filters.height + Style.space(36)

    Flow {
      id: filters
      anchors.left: parent.left
      anchors.right: status.left
      anchors.rightMargin: Style.space(16)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(8)

      FilterChip {
        label: "All"
        selected: root.feed === ""
        onPicked: root.feed = ""
      }

      Repeater {
        model: root.pictureFeeds
        delegate: FilterChip {
          required property var modelData
          label: root.source(modelData.name).section || modelData.name
          selected: root.feed === modelData.name
          onPicked: root.feed = modelData.name
        }
      }
    }

    Row {
      id: status
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(14)

      Label {
        anchors.verticalCenter: parent.verticalCenter
        visible: root.pending > 0
        text: "Matching " + root.pending + " to Pandora…"
        color: root.theme.muted
        font.pixelSize: Style.font.bodySmall
      }

      Label {
        id: refreshLink
        anchors.verticalCenter: parent.verticalCenter
        text: root.loading ? "Refreshing…" : "󰑐  Refresh"
        color: refreshArea.containsMouse ? root.theme.accent : root.theme.muted
        font.pixelSize: Style.font.bodySmall

        MouseArea {
          id: refreshArea
          anchors.fill: parent
          anchors.margins: -Style.space(6)
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          enabled: !root.loading
          onClicked: root.refresh()
        }
      }
    }
  }

  // ---- the magazine

  GridView {
    id: grid
    visible: root.withPictures.length > 0
    anchors.top: bar.bottom
    anchors.bottom: strip.top
    anchors.left: parent.left
    anchors.right: rail.visible ? rail.left : parent.right
    anchors.leftMargin: Style.space(28) - gap / 2
    anchors.rightMargin: Style.space(28) - gap / 2
    clip: true
    boundsBehavior: Flickable.StopAtBounds
    model: root.withPictures

    readonly property real gap: Style.space(24)
    readonly property int columns: Math.max(1, Math.floor(width / Style.space(260)))
    readonly property real card: width / columns - gap
    cellWidth: width / columns
    cellHeight: card + Style.space(244)

    delegate: Item {
      id: cardItem
      required property var modelData
      width: grid.cellWidth
      height: grid.cellHeight
      readonly property var where: root.source(modelData.feed)
      readonly property var chipList: root.chips(modelData)

      Column {
        x: grid.gap / 2
        width: grid.card
        spacing: Style.space(8)

        ArtMark {
          width: parent.width
          height: width
          rings: false
          art: root.theme.raised
          artUrl: cardItem.modelData.image

          MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: Qt.openUrlExternally(cardItem.modelData.link)
          }
        }

        Item { width: 1; height: Style.space(2) }

        Label {
          width: parent.width
          text: [cardItem.where.magazine, cardItem.where.section, root.ago(cardItem.modelData.published)]
            .filter(function (s) { return s }).join("  ·  ")
          color: root.theme.accent
          font.pixelSize: Style.font.caption
          font.capitalization: Font.AllUppercase
          font.letterSpacing: 1.1
        }

        Label {
          width: parent.width
          height: Style.space(44)
          text: cardItem.modelData.title
          color: headlineArea.containsMouse ? root.theme.accent : root.theme.strong
          font.pixelSize: Style.font.title
          font.bold: true
          wrapMode: Text.WordWrap
          maximumLineCount: 2
          verticalAlignment: Text.AlignTop

          MouseArea {
            id: headlineArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: Qt.openUrlExternally(cardItem.modelData.link)
          }
        }

        Label {
          width: parent.width
          height: Style.space(52)
          text: cardItem.modelData.summary
          color: root.theme.muted
          font.pixelSize: Style.font.bodySmall
          wrapMode: Text.WordWrap
          maximumLineCount: 3
          verticalAlignment: Text.AlignTop
        }

        // Chips, or why there are none. Two rows, so a long artist credit
        // beside a song or album still shows.
        Item {
          width: parent.width
          height: Style.space(62)
          clip: true

          Flow {
            width: parent.width
            spacing: Style.space(6)
            visible: cardItem.chipList.length > 0

            Repeater {
              model: cardItem.chipList
              delegate: MatchChip {
                required property var modelData
                info: modelData
                onPicked: root.follow(modelData)
              }
            }
          }

          Label {
            visible: cardItem.chipList.length === 0
            y: Style.space(6)
            text: cardItem.modelData.resolved ? "Not on Pandora" : "Looking it up…"
            color: root.theme.faint
            font.pixelSize: Style.font.caption
          }
        }
      }
    }
  }

  // ---- headlines: articles without a picture

  Item {
    id: rail
    visible: root.headlines.length > 0
    anchors.top: bar.bottom
    anchors.bottom: strip.top
    anchors.right: parent.right
    anchors.rightMargin: Style.space(28)
    // Beside the grid; the whole page when there is nothing with a picture.
    readonly property bool alone: !grid.visible
    width: alone ? parent.width - Style.space(56) : Math.max(Style.space(260), Math.min(Style.space(360), parent.width * 0.3))

    Rectangle {
      visible: !rail.alone
      anchors.left: parent.left
      anchors.leftMargin: -Style.space(14)
      anchors.top: parent.top
      anchors.bottom: parent.bottom
      width: 1
      color: root.theme.surface
    }

    Label {
      id: railHeading
      anchors.left: parent.left
      anchors.leftMargin: Style.space(4)
      text: "Headlines"
      color: root.theme.faint
      font.pixelSize: Style.font.bodySmall
      font.bold: true
      font.capitalization: Font.AllUppercase
      font.letterSpacing: 1.6
    }

    ListView {
      id: headlineList
      anchors.top: railHeading.bottom
      anchors.topMargin: Style.space(12)
      anchors.bottom: parent.bottom
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.leftMargin: Style.space(4)
      clip: true
      boundsBehavior: Flickable.StopAtBounds
      model: root.headlines

      delegate: Column {
        id: headline
        required property var modelData
        readonly property var where: root.source(modelData.feed)
        readonly property var chipList: root.chips(modelData)
        width: headlineList.width
        spacing: Style.space(6)
        bottomPadding: Style.space(16)

        Label {
          width: parent.width
          text: [headline.where.magazine, headline.where.section, root.ago(headline.modelData.published)]
            .filter(function (s) { return s }).join("  ·  ")
          color: root.theme.accent
          font.pixelSize: Style.font.caption
          font.capitalization: Font.AllUppercase
          font.letterSpacing: 1.1
        }

        Label {
          width: parent.width
          text: headline.modelData.title
          color: headlineArea.containsMouse ? root.theme.accent : root.theme.strong
          font.pixelSize: Style.font.body
          font.bold: true
          wrapMode: Text.WordWrap
          maximumLineCount: rail.alone ? 2 : 3

          MouseArea {
            id: headlineArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: Qt.openUrlExternally(headline.modelData.link)
          }
        }

        // With the whole page to itself, there is room for the summary.
        Label {
          visible: rail.alone && text !== ""
          width: parent.width
          text: headline.modelData.summary
          color: root.theme.muted
          font.pixelSize: Style.font.bodySmall
          wrapMode: Text.WordWrap
          maximumLineCount: 2
        }

        Flow {
          visible: headline.chipList.length > 0
          width: parent.width
          spacing: Style.space(6)

          Repeater {
            model: headline.chipList
            delegate: MatchChip {
              required property var modelData
              info: modelData
              onPicked: root.follow(modelData)
            }
          }
        }

        Rectangle {
          width: parent.width
          height: 1
          color: root.theme.surface
        }
      }
    }
  }

  Label {
    anchors.centerIn: parent
    visible: !grid.visible && !rail.visible
    text: {
      if (root.loading || !root.discover)
        return "Loading the latest reviews…"
      if (root.discover.errors.length > 0)
        return root.discover.errors.join("\n")
      return "Nothing here yet."
    }
    color: root.theme.muted
    font.pixelSize: Style.font.body
    horizontalAlignment: Text.AlignHCenter
  }

  // ---- what's playing, to control it while reading

  Rectangle {
    id: strip
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.bottom: parent.bottom
    height: root.player.title !== "" ? Style.space(64) : 0
    visible: height > 0
    color: root.theme.surface

    ArtMark {
      id: stripArt
      anchors.left: parent.left
      anchors.leftMargin: Style.space(18)
      anchors.verticalCenter: parent.verticalCenter
      width: Style.space(44)
      height: width
      rings: false
      art: root.theme.art
      artUrl: root.player.artUrl
    }

    Column {
      anchors.left: stripArt.right
      anchors.leftMargin: Style.space(14)
      anchors.right: stripControls.left
      anchors.rightMargin: Style.space(14)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(2)

      Label {
        width: parent.width
        text: root.player.title
        color: stripArea.containsMouse ? root.theme.accent : root.theme.strong
        font.pixelSize: Style.font.body
        font.bold: true
      }

      Label {
        width: parent.width
        text: [root.player.artist, root.player.sourceLabel].filter(function (s) { return s }).join("  ·  ")
        color: root.theme.muted
        font.pixelSize: Style.font.caption
      }
    }

    MouseArea {
      id: stripArea
      anchors.left: parent.left
      anchors.right: stripControls.left
      anchors.top: parent.top
      anchors.bottom: parent.bottom
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: root.showPlayer()
    }

    Row {
      id: stripControls
      anchors.right: parent.right
      anchors.rightMargin: Style.space(18)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(6)

      // The same five as the mini player.
      IconButton {
        theme: root.theme
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
        anchors.verticalCenter: parent.verticalCenter
        icon: "󰦛"
        enabled: root.player.canReplay
        fontFamily: root.fontFamily
        onActivated: root.player.replay()
      }

      IconButton {
        theme: root.theme
        anchors.verticalCenter: parent.verticalCenter
        width: Style.space(44)
        icon: root.player.loading ? "󰔟" : (root.player.playing ? "󰏤" : "󰐊")
        primary: true
        iconSize: Style.font.iconLarge
        fontFamily: root.fontFamily
        onActivated: root.player.playPause()
      }

      IconButton {
        theme: root.theme
        anchors.verticalCenter: parent.verticalCenter
        icon: "󰒭"
        enabled: root.player.canGoNext
        fontFamily: root.fontFamily
        onActivated: root.player.next()
      }

      IconButton {
        theme: root.theme
        anchors.verticalCenter: parent.verticalCenter
        icon: root.player.rating === 1 ? "󰔓" : "󰔔"
        active: root.player.rating === 1
        enabled: root.player.canRate
        fontFamily: root.fontFamily
        onActivated: root.player.thumbUp()
      }
    }
  }
}
