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
    // Every picture asks it for its copy on disk.
    Component.onCompleted: artCache.useEngine(engine)
  }

  ArtMark {
    id: artCache
    visible: false
  }

  Player {
    id: player
    engine: engine
  }

  readonly property string label: {
    if (player.title && player.artist)
      return player.title + " · " + player.artist
    return player.title || player.artist || "Omadora"
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

    OmadoraMark {
      id: logo
      anchors.centerIn: parent
      // A solid circle reads smaller than a glyph of the same height, so
      // match the bar's other circular mark (GitHub's), not the glyph median.
      size: Style.font.body * 0.96
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
        if (player.isPodcast)
          fullPlayer.showShow(player.showKey, player.artist)
        else
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
    // Stations made elsewhere (Pandora's website or apps) show up when the
    // player opens, and while it stays open.
    onVisibleChanged: {
      if (visible) {
        player.refreshModes()
        player.refreshStations()
      }
    }

    Timer {
      interval: 60000
      repeat: true
      running: playerWindow.visible
      onTriggered: player.refreshStations()
    }
    // Closed from outside (Super+W, or the compositor): its visible stays
    // true otherwise, and opening it again would change nothing.
    onClosed: visible = false

    FullPlayer {
      id: fullPlayer
      anchors.fill: parent
      engine: engine
      player: player
      theme: theme
      fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
      shown: playerWindow.visible
      onShrinkRequested: root.shrink()
    }
  }
}
