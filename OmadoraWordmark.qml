import QtQuick
import QtQuick.Shapes
import qs.Commons

// "omadora", lettered after Pandora's wordmark: its o, a, d, r, and a, with
// an m drawn to match (two arches the weight of its n, as one outline so
// no renderer seam shows where they meet). Takes any color.
Item {
  id: root

  // Height of the lowercase letters, which sizes everything else.
  property real xHeight: Style.space(14)
  property color color: Color.foreground

  // Drawn on a grid where the x-height runs from 10 to 45.9 (the baseline)
  // and the d rises to 0.
  readonly property real unit: xHeight / 35.9
  // Where the middle of the lowercase sits, measured from the top.
  readonly property real xHeightCenter: 27.95 * unit

  implicitWidth: 285.9 * unit
  implicitHeight: 45.9 * unit

  Shape {
    width: 285.9 * root.unit
    height: 46 * root.unit
    preferredRendererType: Shape.CurveRenderer

    ShapePath {
      scale: Qt.size(root.unit, root.unit)
      fillColor: root.color
      fillRule: ShapePath.WindingFill
      strokeColor: "transparent"
      strokeWidth: 0

      PathSvg {
        path: "M 18.5,17.4 c 6.1,0,11.1,5,11.1,11.1 c 0,6.1,-5,11.1,-11.1,11.1 c -6.1,0,-11.1,-5,-11.1,-11.1 C "
          + "7.4,22.3,12.4,17.4,18.5,17.4 M 18.5,10 c -10.2,0,-18.5,8.3,-18.5,18.5 c "
          + "0,10.2,8.3,18.5,18.5,18.5 c 10.2,0,18.5,-8.3,18.5,-18.5 C 37,18.2,28.7,10,18.5,10 M42.00,45.9 "
          + "L42.00,28.50 A15.85,18.5 0 0 1 70.00,16.62 A15.85,18.5 0 0 1 98.00,28.50 L98.00,45.9 L90.60,45.9 "
          + "L90.60,28.50 A8.45,11.10 0 0 0 73.70,28.50 L73.70,45.9 L66.30,45.9 L66.30,28.50 A8.45,11.10 0 0 "
          + "0 49.40,28.50 L49.40,45.9 Z M 133.2,11.1 v 3 c -3.2,-2.6,-7.3,-4.2,-11.7,-4.2 c "
          + "-10.2,0,-18.5,8.3,-18.5,18.5 c 0,10.2,8.3,18.5,18.5,18.5 c 4.4,0,8.5,-1.6,11.7,-4.2 v 3 h 6.8 V "
          + "11.1 H 133.2 z M 121.9,40 c -6.4,0,-11.5,-5.2,-11.5,-11.5 c 0,-6.4,5.2,-11.5,11.5,-11.5 c "
          + "6.4,0,11.5,5.2,11.5,11.5 C 133.4,34.8,128.2,40,121.9,40 M 178.5,0 c -1.9,0,-3.4,1.5,-3.4,3.4 v "
          + "10.7 c -3.2,-2.6,-7.3,-4.2,-11.7,-4.2 c -10.2,0,-18.5,8.3,-18.5,18.5 c 0,10.2,8.3,18.5,18.5,18.5 "
          + "c 4.4,0,8.5,-1.6,11.7,-4.2 v 3 h 6.8 V 3.4 V 0 H 178.5 z M 163.8,40 c "
          + "-6.4,0,-11.5,-5.2,-11.5,-11.5 c 0,-6.4,5.2,-11.5,11.5,-11.5 c 6.4,0,11.5,5.2,11.5,11.5 C "
          + "175.4,34.8,170.2,40,163.8,40 M 205.4,17.4 c 6.1,0,11.1,5,11.1,11.1 c 0,6.1,-5,11.1,-11.1,11.1 c "
          + "-6.1,0,-11.1,-5,-11.1,-11.1 C 194.3,22.3,199.3,17.4,205.4,17.4 M 205.4,10 c "
          + "-10.2,0,-18.5,8.3,-18.5,18.5 c 0,10.2,8.3,18.5,18.5,18.5 c 10.2,0,18.5,-8.3,18.5,-18.5 C "
          + "223.9,18.2,215.6,10,205.4,10 M 236.1,13.3 c -4,2.5,-7.4,7,-7.4,15 v 17.5 h 7.4 V 28.3 c "
          + "0,-11.5,11.9,-10.9,11.9,-10.9 V 10 v 0 C 247.9,10,241.5,9.8,236.1,13.3 M 279.1,11.1 v 3 c "
          + "-3.2,-2.6,-7.3,-4.2,-11.7,-4.2 c -10.2,0,-18.5,8.3,-18.5,18.5 c 0,10.2,8.3,18.5,18.5,18.5 c "
          + "4.4,0,8.5,-1.6,11.7,-4.2 v 3 h 6.8 V 11.1 H 279.1 z M 267.9,40 c -6.4,0,-11.5,-5.2,-11.5,-11.5 c "
          + "0,-6.4,5.2,-11.5,11.5,-11.5 c 6.4,0,11.5,5.2,11.5,11.5 C 279.4,34.8,274.2,40,267.9,40 "
      }
    }
  }
}
