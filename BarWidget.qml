import QtQuick
import Quickshell
import qs.Ui
import qs.Commons

// What you see of Omadora. omadora-engine plays Pandora, radio and on
// demand alike; this widget is the player.
BarWidget {
  id: root
  moduleName: "scttymn.omadora"

  Engine {
    id: engine
  }

  Player {
    id: player
    engine: engine
  }

  readonly property string label: {
    if (player.title && player.artist)
      return player.title + " · " + player.artist
    return player.title || player.artist || "Pandora"
  }

  Theme {
    id: theme
  }

  property bool popupOpen: false

  function close() {
    popupOpen = false
  }

  // The full player is a regular window, so Hyprland tiles, focuses, and
  // closes it like any other app.
  function expand() {
    popupOpen = false
    playerWindow.visible = true
  }

  function shrink() {
    playerWindow.visible = false
    popupOpen = true
  }

  // The bar's own click layer calls triggerPress on a registered WidgetButton.
  // A MouseArea underneath that layer never sees the left click.
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    labelVisible: false
    hasVisualContent: true
    tooltipText: root.label
    fixedWidth: button.vertical ? -1 : logo.implicitWidth + button.scaledHorizontalMargin * 2

    PandoraLogo {
      id: logo
      anchors.centerIn: parent
      // Glyph icons ink about 85% of their font size; match the neighbors.
      logoHeight: Math.round(Style.font.body * 0.85)
      color: button.foreground
      opacity: player.hasPlayer ? 1 : 0.6
    }

    onPressed: function(b) {
      if (b === Qt.MiddleButton) {
        player.next()
        return
      }
      if (b !== Qt.LeftButton)
        return
      root.popupOpen = !root.popupOpen
    }

    onWheelMoved: function(delta) {
      if (delta < 0)
        player.next()
    }
  }

  PopupCard {
    id: popup
    anchorItem: root
    bar: root.bar
    owner: root
    open: root.popupOpen
    padding: 0
    contentWidth: popup.fittedContentWidth(Style.space(380))
    contentHeight: popup.cappedContentHeight(Style.space(280))

    MiniPlayer {
      anchors.fill: parent
      player: player
      theme: theme
      fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
      onExpandRequested: root.expand()
      onCloseRequested: root.close()
      onArtistRequested: {
        root.expand()
        fullPlayer.showArtist(player.artistId, player.artist)
      }
      onAlbumRequested: {
        root.expand()
        fullPlayer.showAlbum(player.albumId, player.album, player.artist)
      }
    }
  }

  // Modes belong to a station, so fetch them again when it changes.
  Connections {
    target: player
    function onSourceIdChanged() {
      if (playerWindow.visible)
        Qt.callLater(player.refreshModes)
    }
  }

  FloatingWindow {
    id: playerWindow
    visible: false
    title: "Omadora"
    color: theme.bg
    implicitWidth: Style.space(1100)
    implicitHeight: Style.space(700)
    minimumSize: Qt.size(Style.space(720), Style.space(520))
    onVisibleChanged: if (visible) player.refreshModes()

    FullPlayer {
      id: fullPlayer
      anchors.fill: parent
      engine: engine
      player: player
      theme: theme
      fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
      onShrinkRequested: root.shrink()
    }
  }
}
