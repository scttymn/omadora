import QtQuick
import qs.Commons

// One row in search results or an artist or album view: art, two lines,
// and an optional action on the right that shows on hover.
Rectangle {
  id: row

  required property var theme
  property string fontFamily: Style.font.family
  property string title: ""
  property string subtitle: ""
  property string artUrl: ""
  property color swatch: theme.art
  property string glyph: ""
  property string action: ""
  property bool busy: false
  property bool current: false
  signal picked()
  signal actionPicked()

  height: Style.space(52)
  radius: Style.cornerRadius
  color: current ? theme.raised : (rowMouse.containsMouse ? Util.alpha(theme.raised, 0.6) : "transparent")

  ArtMark {
    id: art
    visible: row.glyph === ""
    anchors.left: parent.left
    anchors.leftMargin: Style.space(8)
    anchors.verticalCenter: parent.verticalCenter
    width: Style.space(36)
    height: width
    rings: false
    art: row.swatch
    artUrl: row.artUrl
  }

  Text {
    visible: row.glyph !== ""
    anchors.horizontalCenter: art.horizontalCenter
    anchors.verticalCenter: parent.verticalCenter
    textFormat: Text.PlainText
    text: row.glyph
    color: row.theme.fg
    font.family: row.fontFamily
    font.pixelSize: Style.font.title
  }

  Column {
    anchors.left: art.right
    anchors.leftMargin: Style.space(12)
    anchors.right: actionLabel.visible ? actionLabel.left : parent.right
    anchors.rightMargin: Style.space(10)
    anchors.verticalCenter: parent.verticalCenter
    spacing: Style.space(2)

    Text {
      width: parent.width
      textFormat: Text.PlainText
      text: row.title
      color: row.current ? row.theme.accent : row.theme.strong
      font.family: row.fontFamily
      font.pixelSize: Style.font.body
      font.bold: row.current
      elide: Text.ElideRight
    }

    Text {
      width: parent.width
      visible: text !== ""
      textFormat: Text.PlainText
      text: row.subtitle
      color: row.theme.muted
      font.family: row.fontFamily
      font.pixelSize: Style.font.caption
      elide: Text.ElideRight
    }
  }

  MouseArea {
    id: rowMouse
    anchors.fill: parent
    hoverEnabled: true
    cursorShape: Qt.PointingHandCursor
    onClicked: row.picked()
  }

  Text {
    id: actionLabel
    visible: row.action !== "" && (row.busy || rowMouse.containsMouse || actionMouse.containsMouse)
    anchors.right: parent.right
    anchors.rightMargin: Style.space(10)
    anchors.verticalCenter: parent.verticalCenter
    textFormat: Text.PlainText
    text: row.busy ? "Starting…" : row.action
    color: actionMouse.containsMouse ? row.theme.strong : row.theme.accent
    font.family: row.fontFamily
    font.pixelSize: Style.font.bodySmall

    MouseArea {
      id: actionMouse
      anchors.fill: parent
      anchors.margins: -Style.space(6)
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: row.actionPicked()
    }
  }
}
