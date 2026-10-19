import QtQuick
import Quickshell.Io
import qs.Commons

// Player colors, taken from the active Omarchy theme. Foreground, background,
// and accent come from the shell's Color singleton, so a theme switch repaints
// the player. The extra shades and the swatch palette come from the theme's
// colors.toml; when a theme leaves one out, it is mixed from the base roles.
QtObject {
  id: root

  property var values: ({})

  function mix(a, b, t) {
    return Qt.rgba(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, 1)
  }

  function pick(key, fallback) {
    var v = values[key]
    return v ? v : fallback
  }

  function parse(raw) {
    var parsed = {}
    var lines = String(raw || "").split("\n")
    for (var i = 0; i < lines.length; i++) {
      var match = lines[i].match(/^\s*([A-Za-z0-9_-]+)\s*=\s*["']?(#[0-9A-Fa-f]{6})/)
      if (match)
        parsed[match[1]] = match[2]
    }
    values = parsed
  }

  // Full player body and the right column.
  readonly property color bg: Color.background
  // Mini card, station sidebar, search field, selected tab.
  readonly property color surface: pick("lighter_background", mix(Color.background, Color.foreground, 0.07))
  // Progress track, selected station row, hover.
  readonly property color raised: mix(surface, Color.foreground, 0.08)
  readonly property color line: raised
  // Outlines on chips and other unfilled controls.
  readonly property color border: mix(surface, Color.foreground, 0.2)
  readonly property color fg: Color.foreground
  readonly property color strong: pick("bright_foreground", Color.foreground)
  readonly property color muted: mix(Color.foreground, surface, 0.3)
  readonly property color faint: mix(Color.foreground, surface, 0.55)
  readonly property color accent: Color.accent
  readonly property color accentText: Color.background
  readonly property color art: mix(Color.accent, Color.background, 0.45)
  readonly property color ring: mix(art, Color.background, 0.45)
  readonly property var swatches: [
    pick("bright_magenta", pick("magenta", Color.accent)),
    art,
    pick("bright_cyan", pick("cyan", Color.accent)),
    pick("yellow", Color.accent),
    pick("red", Color.urgent),
    pick("green", Color.accent),
    pick("bright_yellow", pick("orange", Color.accent))
  ]

  function swatch(index) {
    var n = Math.max(0, Number(index) || 0)
    return swatches[n % swatches.length]
  }

  // Runtime theme switches reach Color through shell IPC rather than a file
  // change, so reload whenever its base roles move.
  readonly property string themeKey: String(Color.background) + String(Color.foreground) + String(Color.accent)
  onThemeKeyChanged: colorsFile.reload()

  property FileView colorsFile: FileView {
    id: colorsFile
    path: Color.currentThemePath + "/colors.toml"
    watchChanges: true
    printErrors: false
    onLoaded: root.parse(text())
    onLoadFailed: root.values = ({})
    onFileChanged: reload()
  }
}
