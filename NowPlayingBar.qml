import QtQuick
import qs.Commons

// What's playing, along the bottom of the full player wherever the big
// now-playing view isn't: the song, the five controls, and a way back.
Rectangle {
  id: root

  required property var player
  required property var theme
  property string fontFamily: Style.font.family
  // Whether there's reason to show it; it also needs a song loaded.
  property bool wanted: true
  signal showPlayer()

  component Label: Text {
    textFormat: Text.PlainText
    font.family: root.fontFamily
    elide: Text.ElideRight
  }

  height: wanted && player.title !== "" ? Style.space(64) : 0
  visible: height > 0
  color: theme.surface

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
      icon: root.player.liked ? "󰔓" : "󰔔"
      active: root.player.liked
      enabled: root.player.canLike
      fontFamily: root.fontFamily
      onActivated: root.player.thumbUp()
    }
  }
}
