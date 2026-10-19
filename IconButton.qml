import QtQuick
import qs.Commons

// A glyph button. The primary one is the filled accent square around pause.
Rectangle {
  id: root

  required property var theme
  property string icon: ""
  property bool primary: false
  property bool active: false
  property real iconSize: primary ? Style.font.iconLarge : Style.font.title
  property string fontFamily: Style.font.family
  signal activated()

  width: primary ? Style.space(52) : Style.space(36)
  height: width
  radius: Style.cornerRadius
  color: {
    if (primary)
      return area.containsMouse ? Qt.lighter(theme.accent, 1.08) : theme.accent
    if (area.containsMouse && root.enabled)
      return theme.raised
    return "transparent"
  }
  opacity: enabled ? 1 : 0.35

  Text {
    anchors.centerIn: parent
    textFormat: Text.PlainText
    text: root.icon
    color: root.primary ? root.theme.accentText : (root.active ? root.theme.accent : root.theme.fg)
    font.family: root.fontFamily
    font.pixelSize: root.iconSize
  }

  MouseArea {
    id: area
    anchors.fill: parent
    hoverEnabled: true
    enabled: root.enabled
    cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
    onClicked: root.activated()
  }
}
