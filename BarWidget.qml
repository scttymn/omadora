import QtQuick
import Quickshell
import qs.Ui
import qs.Commons

// What you see of Omadora. The site is a separate Chrome window.
BarWidget {
  id: root
  moduleName: "scttymn.omadora"

  Service {
    id: pandora
  }

  readonly property bool hasTrack: pandora.title !== "" || pandora.artist !== ""
  readonly property string label: {
    if (pandora.title && pandora.artist)
      return pandora.title + "  ·  " + pandora.artist
    if (pandora.title)
      return pandora.title
    if (pandora.artist)
      return pandora.artist
    return pandora.hasPlayer ? "Pandora" : "Omadora"
  }
  readonly property string playIcon: pandora.playing ? "󰏤" : "󰐊"

  property bool popupOpen: false

  function close() { popupOpen = false }

  implicitWidth: row.implicitWidth + Style.space(14)
  implicitHeight: barSize

  Row {
    id: row
    anchors.centerIn: parent
    spacing: Style.space(6)

    Text {
      textFormat: Text.PlainText
      anchors.verticalCenter: parent.verticalCenter
      text: root.playIcon
      color: pandora.playing ? root.bar.barForeground : Qt.darker(root.bar.barForeground, 1.5)
      font.family: root.bar.fontFamily
      font.pixelSize: Style.font.body
    }

    Text {
      textFormat: Text.PlainText
      anchors.verticalCenter: parent.verticalCenter
      text: root.label
      color: root.bar.barForeground
      font.family: root.bar.fontFamily
      font.pixelSize: Style.font.body
      elide: Text.ElideRight
      width: Math.min(implicitWidth, Style.space(160))
      visible: !root.bar.vertical
    }
  }

  MouseArea {
    anchors.fill: parent
    hoverEnabled: true
    cursorShape: Qt.PointingHandCursor
    acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton

    onClicked: function(mouse) {
      if (mouse.button === Qt.MiddleButton) {
        pandora.next()
      } else if (mouse.button === Qt.RightButton) {
        root.popupOpen = !root.popupOpen
      } else if (!pandora.hasPlayer) {
        pandora.engine("launch")
      } else {
        pandora.playPause()
      }
    }
    onWheel: function(wheel) {
      if (wheel.angleDelta.y > 0)
        pandora.previous()
      else if (wheel.angleDelta.y < 0)
        pandora.next()
    }
    onEntered: if (root.bar) root.bar.showTooltip(root, root.label)
    onExited: if (root.bar) root.bar.hideTooltip(root)
  }

  PopupCard {
    id: popup
    anchorItem: root
    bar: root.bar
    owner: root
    open: root.popupOpen
    contentWidth: popup.fittedContentWidth(Style.space(280))
    contentHeight: popup.fittedContentHeight(column.implicitHeight)

    Column {
      id: column
      anchors.fill: parent
      spacing: Style.space(10)

      Text {
        textFormat: Text.PlainText
        width: parent.width
        text: pandora.title || "Pandora"
        color: root.bar.foreground
        font.family: root.bar.fontFamily
        font.pixelSize: Style.font.body
        elide: Text.ElideRight
      }

      Text {
        textFormat: Text.PlainText
        width: parent.width
        text: pandora.artist
        color: Qt.darker(root.bar.foreground, 1.6)
        font.family: root.bar.fontFamily
        font.pixelSize: Style.font.caption
        elide: Text.ElideRight
        visible: text !== ""
      }

      Row {
        anchors.horizontalCenter: parent.horizontalCenter
        spacing: Style.space(6)

        Button {
          iconText: "󰒮"
          foreground: root.bar.foreground
          enabled: pandora.player && pandora.player.canGoPrevious
          opacity: enabled ? 1 : 0.4
          onClicked: pandora.previous()
        }

        Button {
          iconText: root.playIcon
          foreground: root.bar.foreground
          enabled: pandora.hasPlayer
          opacity: enabled ? 1 : 0.4
          onClicked: pandora.playPause()
        }

        Button {
          iconText: "󰒭"
          foreground: root.bar.foreground
          enabled: pandora.player && pandora.player.canGoNext
          opacity: enabled ? 1 : 0.4
          onClicked: pandora.next()
        }
      }

      Button {
        text: "Show Pandora"
        foreground: root.bar.foreground
        onClicked: pandora.engine("show")
      }

      Button {
        text: "Hide the site"
        foreground: root.bar.foreground
        onClicked: pandora.engine("hide")
      }
    }
  }
}
