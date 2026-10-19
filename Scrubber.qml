import QtQuick
import qs.Commons

// A thin bar to scrub: the track's progress, or a volume. Hovering shows a
// handle; dragging previews the spot (`shown`), and the seek is reported once
// on release. A `live` scrubber reports every move instead, for volume.
Item {
  id: root

  property real fraction: 0
  property color track: Color.muted
  property color fill: Color.accent
  property int barHeight: Style.space(3)
  property bool live: false
  signal seeked(real fraction)

  readonly property bool dragging: area.pressed
  readonly property bool hot: enabled && (area.containsMouse || dragging)
  property real dragFraction: 0
  // After a release, hold the dropped spot until the player catches up.
  property bool settling: false
  // What the bar shows: the drag while scrubbing, else the real position.
  readonly property real shown: Math.max(0, Math.min(1, dragging || settling ? dragFraction : fraction))

  Timer {
    id: settle
    interval: 900
    onTriggered: root.settling = false
  }

  implicitHeight: Style.space(14)

  Rectangle {
    id: bar
    anchors.verticalCenter: parent.verticalCenter
    width: parent.width
    height: root.hot ? root.barHeight + Style.space(2) : root.barHeight
    radius: Style.cornerRadius > 0 ? height / 2 : 0
    color: root.track

    Rectangle {
      width: parent.width * root.shown
      height: parent.height
      radius: parent.radius
      color: root.fill
    }
  }

  Rectangle {
    visible: root.hot
    x: Math.round(bar.width * root.shown - width / 2)
    anchors.verticalCenter: parent.verticalCenter
    width: Style.space(12)
    height: width
    radius: Style.cornerRadius > 0 ? width / 2 : Style.space(2)
    color: root.fill
    border.color: Qt.lighter(root.fill, 1.2)
    border.width: 1
  }

  MouseArea {
    id: area
    anchors.fill: parent
    anchors.topMargin: -Style.space(6)
    anchors.bottomMargin: -Style.space(6)
    enabled: root.enabled
    hoverEnabled: true
    preventStealing: true
    cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor

    function at(x) {
      return width > 0 ? Math.max(0, Math.min(1, x / width)) : 0
    }

    onPressed: function (mouse) {
      root.dragFraction = at(mouse.x)
      if (root.live)
        root.seeked(root.dragFraction)
    }
    onPositionChanged: function (mouse) {
      if (!pressed)
        return
      root.dragFraction = at(mouse.x)
      if (root.live)
        root.seeked(root.dragFraction)
    }
    onReleased: function (mouse) {
      root.dragFraction = at(mouse.x)
      if (!root.live) {
        root.settling = true
        settle.restart()
      }
      root.seeked(root.dragFraction)
    }
  }
}
