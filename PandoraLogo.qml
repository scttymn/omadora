import QtQuick
import QtQuick.Shapes
import qs.Commons

// Pandora's P mark, filled with whatever color the bar or player asks for.
Item {
  id: root

  property real logoHeight: Style.font.body
  property color color: Color.foreground

  implicitWidth: Math.round(logoHeight * 18.236 / 24)
  implicitHeight: Math.round(logoHeight)

  Shape {
    x: -root.logoHeight * 1.882 / 24
    width: root.logoHeight
    height: root.logoHeight
    preferredRendererType: Shape.CurveRenderer

    ShapePath {
      scale: Qt.size(root.logoHeight / 24, root.logoHeight / 24)
      fillColor: root.color
      strokeColor: "transparent"
      strokeWidth: 0

      PathSvg {
        path: "M1.882,0 V24 H8.32 A1.085,1.085 0 0 0 9.405,22.915 V18.803 H11.353 C16.183,18.803 20.118,14.588 20.118,9.401 C20.118,4.214 16.183,0 11.353,0 Z"
      }
    }
  }
}
