import QtQuick
import qs.Commons
import "ArtCache.js" as ArtCache

// Album art, or a swatch with the ring mark from the mockup when there is none.
Item {
  id: root

  property string artUrl: ""
  // What the Image loads: the engine's copy on disk, once it has one.
  property string shownUrl: ""

  function fetchArt() {
    var wanted = artUrl
    ArtCache.resolve(wanted, function (shown) {
      if (root.artUrl === wanted)
        root.shownUrl = shown
    })
  }
  onArtUrlChanged: fetchArt()
  Component.onCompleted: fetchArt()

  // The cache script is one per importing file, so the engine is handed to
  // it through here, where every picture shares it.
  function useEngine(engine) {
    ArtCache.setEngine(engine)
  }
  property bool rings: true
  property color art: Color.accent
  property color ring: Color.background

  component Ring: Rectangle {
    property real size: 0.5
    property real stroke: 0.035
    anchors.centerIn: parent
    width: parent.width * size
    height: width
    radius: width / 2
    color: "transparent"
    border.color: root.ring
    border.width: Math.max(1, Math.round(parent.width * stroke))
  }

  Rectangle {
    anchors.fill: parent
    radius: Style.cornerRadius
    color: root.art
    clip: true

    Ring { visible: root.rings; size: 0.62 }
    Ring { visible: root.rings; size: 0.44 }
    Ring { visible: root.rings; size: 0.26 }
    Ring { visible: root.rings; size: 0.1; stroke: 0.025 }

    Image {
      anchors.fill: parent
      fillMode: Image.PreserveAspectCrop
      asynchronous: true
      source: root.shownUrl
      sourceSize.width: Math.round(width * 2)
      sourceSize.height: Math.round(height * 2)
      visible: status === Image.Ready
    }
  }
}
