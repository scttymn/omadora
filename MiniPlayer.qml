import QtQuick
import qs.Commons

// The card that hangs off the bar: station, art, transport.
Item {
  id: root

  required property var player
  required property var theme
  property string fontFamily: Style.font.family
  signal expandRequested()
  signal closeRequested()
  signal artistRequested()
  signal albumRequested()

  readonly property string stationLabel: player.sourceLabel !== "" ? player.sourceLabel : "Omadora"
  readonly property bool showNotice: player.title === "" && player.notice !== ""

  Rectangle {
    anchors.fill: parent
    radius: Style.cornerRadius
    color: root.theme.surface

    Item {
      id: header
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.top: parent.top
      anchors.margins: Style.space(16)
      height: Style.space(24)

      Text {
        id: stationIcon
        textFormat: Text.PlainText
        text: "󰀃"
        color: root.theme.accent
        font.family: root.fontFamily
        font.pixelSize: Style.font.title
        anchors.left: parent.left
        anchors.verticalCenter: parent.verticalCenter
      }

      Text {
        textFormat: Text.PlainText
        text: root.stationLabel
        color: root.theme.muted
        font.family: root.fontFamily
        font.pixelSize: Style.font.bodySmall
        font.capitalization: Font.AllUppercase
        font.letterSpacing: 1.2
        elide: Text.ElideRight
        anchors.left: stationIcon.right
        anchors.leftMargin: Style.space(8)
        anchors.right: headerActions.left
        anchors.rightMargin: Style.space(8)
        anchors.verticalCenter: parent.verticalCenter
      }

      Row {
        id: headerActions
        anchors.right: parent.right
        anchors.rightMargin: -Style.space(4)
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(4)

        IconButton {
          theme: root.theme
          icon: "󰘖"
          fontFamily: root.fontFamily
          width: Style.space(26)
          iconSize: Style.font.title
          onActivated: root.expandRequested()
        }

        IconButton {
          theme: root.theme
          icon: "󰅖"
          fontFamily: root.fontFamily
          width: Style.space(26)
          iconSize: Style.font.title
          onActivated: root.closeRequested()
        }
      }
    }

    Item {
      id: track
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.top: header.bottom
      anchors.leftMargin: Style.space(16)
      anchors.rightMargin: Style.space(16)
      anchors.topMargin: Style.space(18)
      height: Style.space(72)

      ArtMark {
        id: art
        width: Style.space(72)
        height: Style.space(72)
        artUrl: root.player.artUrl
        art: root.theme.art
        ring: root.theme.ring
      }

      Column {
        anchors.left: art.right
        anchors.leftMargin: Style.space(16)
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(4)

        Text {
          textFormat: Text.PlainText
          width: parent.width
          text: {
            if (root.player.title !== "")
              return root.player.title
            if (root.player.notice !== "")
              return root.player.notice
            if (root.player.loading)
              return "Loading…"
            return "Nothing playing"
          }
          color: root.showNotice ? root.theme.muted : root.theme.strong
          font.family: root.fontFamily
          font.pixelSize: root.showNotice ? Style.font.bodySmall : Style.font.heading
          font.bold: !root.showNotice
          wrapMode: root.showNotice ? Text.WordWrap : Text.NoWrap
          maximumLineCount: 4
          elide: Text.ElideRight
        }

        Text {
          textFormat: Text.PlainText
          width: parent.width
          text: root.player.artist
          color: artistMouse.containsMouse ? root.theme.accent : root.theme.fg
          font.family: root.fontFamily
          font.pixelSize: Style.font.subtitle
          font.underline: artistMouse.containsMouse
          elide: Text.ElideRight
          visible: text !== ""

          MouseArea {
            id: artistMouse
            width: Math.min(parent.width, parent.implicitWidth)
            height: parent.height
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.artistRequested()
          }
        }

        Text {
          textFormat: Text.PlainText
          width: parent.width
          text: root.player.album
          color: albumMouse.containsMouse ? root.theme.accent : root.theme.muted
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          font.underline: albumMouse.containsMouse
          elide: Text.ElideRight
          visible: text !== ""

          MouseArea {
            id: albumMouse
            width: Math.min(parent.width, parent.implicitWidth)
            height: parent.height
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.albumRequested()
          }
        }

        // Another device has the account; the official apps offer the same.
        Rectangle {
          visible: root.player.busy
          width: listenLabel.implicitWidth + Style.space(24)
          height: Style.space(28)
          radius: Style.cornerRadius
          color: listenArea.containsMouse ? Qt.lighter(root.theme.accent, 1.08) : root.theme.accent

          Text {
            id: listenLabel
            anchors.centerIn: parent
            textFormat: Text.PlainText
            text: "Listen here"
            color: root.theme.accentText
            font.family: root.fontFamily
            font.pixelSize: Style.font.bodySmall
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
    }

    Column {
      id: progress
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.top: track.bottom
      anchors.leftMargin: Style.space(16)
      anchors.rightMargin: Style.space(16)
      anchors.topMargin: Style.space(10)
      spacing: Style.space(2)

      Scrubber {
        id: seekBar
        width: parent.width
        fraction: root.player.length > 0 ? root.player.position / root.player.length : 0
        enabled: root.player.canSeek
        track: root.theme.raised
        fill: root.theme.accent
        onSeeked: function (fraction) { root.player.seekFraction(fraction) }
      }

      Item {
        width: parent.width
        height: elapsed.implicitHeight

        Text {
          id: elapsed
          textFormat: Text.PlainText
          text: root.player.formatTime(seekBar.dragging || seekBar.settling ? seekBar.shown * root.player.length : root.player.position)
          color: root.theme.muted
          font.family: root.fontFamily
          font.pixelSize: Style.font.bodySmall
          anchors.left: parent.left
        }

        Text {
          textFormat: Text.PlainText
          text: root.player.formatTime(root.player.length)
          color: root.theme.muted
          font.family: root.fontFamily
          font.pixelSize: Style.font.bodySmall
          anchors.right: parent.right
        }
      }
    }

    // Five controls spread edge to edge, pause in the middle.
    Item {
      id: transport
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.top: progress.bottom
      anchors.leftMargin: Style.space(16)
      anchors.rightMargin: Style.space(16)
      anchors.topMargin: Style.space(12)
      height: Style.space(52)

      readonly property real slot: Style.space(48)
      readonly property real step: (width - slot) / 4

      IconButton {
        theme: root.theme
        x: transport.step * 0 + (transport.slot - width) / 2
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
        x: transport.step * 1 + (transport.slot - width) / 2
        anchors.verticalCenter: parent.verticalCenter
        // A podcast goes back 15 seconds instead.
        icon: root.player.isPodcast ? "󱥆" : "󰦛"
        enabled: root.player.canReplay
        fontFamily: root.fontFamily
        onActivated: root.player.isPodcast ? root.player.skip(-15) : root.player.replay()
      }

      IconButton {
        theme: root.theme
        x: transport.step * 2 + (transport.slot - width) / 2
        anchors.verticalCenter: parent.verticalCenter
        icon: root.player.loading ? "󰔟" : (root.player.playing ? "󰏤" : "󰐊")
        primary: true
        fontFamily: root.fontFamily
        onActivated: root.player.playPause()
      }

      IconButton {
        theme: root.theme
        x: transport.step * 3 + (transport.slot - width) / 2
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
        x: transport.step * 4 + (transport.slot - width) / 2
        anchors.verticalCenter: parent.verticalCenter
        icon: root.player.isPodcast ? "󰒭" : (root.player.liked ? "󰔓" : "󰔔")
        active: !root.player.isPodcast && root.player.liked
        enabled: root.player.isPodcast ? root.player.canGoNext : root.player.canLike
        fontFamily: root.fontFamily
        onActivated: root.player.isPodcast ? root.player.next() : root.player.thumbUp()
      }
    }

    Item {
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.bottom: parent.bottom
      anchors.leftMargin: Style.space(16)
      anchors.rightMargin: Style.space(16)
      anchors.bottomMargin: Style.space(12)
      height: Style.space(18)

      Text {
        textFormat: Text.PlainText
        text: !root.player.onDemand && root.player.skipsLeft >= 0 ? root.player.skipsLeft + " skips left this hour" : ""
        color: root.theme.muted
        font.family: root.fontFamily
        font.pixelSize: Style.font.bodySmall
        anchors.left: parent.left
        anchors.verticalCenter: parent.verticalCenter
        elide: Text.ElideRight
        width: Math.max(0, parent.width - volume.width - Style.space(12))
      }

      Row {
        id: volume
        visible: root.player.volumeReady
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(8)

        Text {
          textFormat: Text.PlainText
          text: "󰖀"
          color: root.theme.muted
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          anchors.verticalCenter: parent.verticalCenter
        }

        Scrubber {
          width: Style.space(64)
          anchors.verticalCenter: parent.verticalCenter
          live: true
          fraction: Math.max(0, Math.min(1, root.player.volume))
          track: root.theme.raised
          fill: root.theme.fg
          onSeeked: function (fraction) { root.player.setVolume(fraction) }
        }
      }
    }

    // No account yet: only a prompt. The form is in the full player, where
    // typing works.
    Rectangle {
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.top: header.bottom
      anchors.bottom: parent.bottom
      anchors.bottomMargin: Style.space(8)
      visible: root.player.needsAccount
      color: root.theme.surface

      MouseArea {
        anchors.fill: parent
      }

      Column {
        anchors.centerIn: parent
        width: parent.width - Style.space(48)
        spacing: Style.space(16)

        OmadoraMark {
          anchors.horizontalCenter: parent.horizontalCenter
          size: Style.space(44)
          color: root.theme.accent
        }

        Text {
          width: parent.width
          textFormat: Text.PlainText
          text: "Sign in to start listening."
          color: root.theme.fg
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          horizontalAlignment: Text.AlignHCenter
          wrapMode: Text.WordWrap
        }

        Rectangle {
          anchors.horizontalCenter: parent.horizontalCenter
          width: signInLabel.implicitWidth + Style.space(40)
          height: Style.space(36)
          radius: Style.cornerRadius
          color: signInArea.containsMouse ? Qt.lighter(root.theme.accent, 1.08) : root.theme.accent

          Text {
            id: signInLabel
            anchors.centerIn: parent
            textFormat: Text.PlainText
            text: "Sign in"
            color: root.theme.accentText
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            font.bold: true
          }

          MouseArea {
            id: signInArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.expandRequested()
          }
        }
      }
    }
  }
}
