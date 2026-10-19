import QtQuick
import QtQuick.Shapes
import qs.Commons

// Omadora's mark: a circle with an eighth note cut out of it, the ring's end
// rounded off like a pen stroke. One outline, so it takes any color.
Item {
  id: root

  // The circle's diameter.
  property real size: Style.space(24)
  property color color: Color.accent

  // Drawn on a 100 grid, where the circle spans 6 to 94.
  readonly property real unit: size / 88

  implicitWidth: size
  implicitHeight: size

  Shape {
    x: -6 * root.unit
    y: -6 * root.unit
    width: 100 * root.unit
    height: 100 * root.unit
    preferredRendererType: Shape.CurveRenderer

    ShapePath {
      scale: Qt.size(root.unit, root.unit)
      fillColor: root.color
      fillRule: ShapePath.WindingFill
      strokeColor: "transparent"
      strokeWidth: 0

      PathSvg {
        path: "M62.83,7.91 A44.0,44.0 0 1 0 83.10,21.00 A7,7 0 0 0 70.83,25.62 L70.83,45.46 A22.0,16.0 -28.0 1 1 62.82,33.89 Z"
      }
    }
  }
}
