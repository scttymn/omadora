import QtQuick
import qs.Commons

// The mark and the wordmark together. The mark centers on the lowercase,
// not on the d's ascender, so the pair reads as one line.
Item {
  id: root

  property real xHeight: Style.space(14)
  property color markColor: Color.accent
  property color wordColor: Color.foreground

  readonly property real markSize: xHeight * 1.95
  readonly property real gap: xHeight * 0.5
  // Lift everything if the mark reaches above the wordmark's top.
  readonly property real markTop: word.xHeightCenter - markSize / 2
  readonly property real rise: Math.min(0, markTop)

  implicitWidth: markSize + gap + word.implicitWidth
  implicitHeight: Math.max(markTop + markSize, word.implicitHeight) - rise

  OmadoraMark {
    y: root.markTop - root.rise
    size: root.markSize
    color: root.markColor
  }

  OmadoraWordmark {
    id: word
    x: root.markSize + root.gap
    y: -root.rise
    xHeight: root.xHeight
    color: root.wordColor
  }
}
