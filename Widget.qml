// The omaestro icon in the Omarchy bar. A click opens or closes the rules
// panel (Panel.qml), a right click reloads the rules. The icon takes the
// bar's urgent color while something needs a look (the rules did not load,
// override is on) and dims while the daemon is not running; the tooltip
// says which. State comes from `om status --json` every few seconds,
// through scripts/om like the panel.
import QtQuick
import Quickshell.Io
import qs.Ui

BarWidget {
  id: root
  moduleName: "io.github.iluxav.omaestro"

  readonly property string pluginDir: String(Qt.resolvedUrl(".")).replace(/^file:\/\//, "")
  readonly property string om: pluginDir + "scripts/om"

  property bool running: false
  property bool checked: false
  property bool overrideOn: false
  property string loadError: ""
  property int triggers: 0
  property int disabled: 0
  property string stderrText: ""

  readonly property bool attention: running && (loadError !== "" || overrideOn)
  readonly property string tooltip: !checked ? "omaestro"
    : !running ? "omaestro is not running" + (stderrText !== "" ? ": " + stderrText : "")
    : loadError !== "" ? "omaestro: the rules did not load\n" + loadError
    : "omaestro: " + triggers + (triggers === 1 ? " rule" : " rules")
      + (disabled > 0 ? ", " + disabled + " off" : "")
      + (overrideOn ? "\noverride is on: rules take chords Hyprland already has" : "")

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  function refresh() {
    if (!statusProc.running) statusProc.running = true
  }

  // The "om" of the logo (assets/omaestro-logo.png) on a 50.4 x 24 grid: a
  // ring, and an m whose first arch comes out from behind it, past a gap.
  // Drawn as a vector shape in one colour, the bar's (or its urgent colour),
  // so it has the weight of the glyphs beside it and follows the theme.
  function iconSource(color) {
    // rgb(), not the colour's own string: that becomes #aarrggbb when it has
    // alpha, which SVG does not understand.
    var fill = "rgb(" + Math.round(color.r * 255) + "," + Math.round(color.g * 255) + "," + Math.round(color.b * 255) + ")"
    var svg = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 50.4 24'>"
      + "<defs><mask id='gap'><rect width='50.4' height='24' fill='white'/><circle cx='12' cy='12' r='14' fill='black'/></mask></defs>"
      + "<path mask='url(#gap)' fill='none' stroke='" + fill + "' stroke-width='6.2'"
      + " d='M21.4 10A6.4 6.4 0 0 1 34.2 10V24M34.2 10A6.4 6.4 0 0 1 47 10V24'/>"
      + "<path fill='" + fill + "' fill-rule='evenodd'"
      + " d='M12 0a12 12 0 1 1 0 24a12 12 0 1 1 0-24zM12 5.4a6.6 6.6 0 1 0 0 13.2a6.6 6.6 0 1 0 0-13.2z'/>"
      + "</svg>"
    return "data:image/svg+xml;utf8," + encodeURIComponent(svg)
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    iconComponent: Component {
      Item {
        Image {
          anchors.centerIn: parent
          height: Math.round(parent.height * 0.5)
          width: Math.round(height * 50.4 / 24)
          source: root.iconSource(button.active ? button.activeColor : button.foreground)
          // Rendered large and scaled down, so it is smooth at any display scale.
          sourceSize: Qt.size(202, 96)
          smooth: true
          mipmap: true
        }
      }
    }
    active: root.attention
    dimmed: root.checked && !root.running
    tooltipText: root.tooltip
    onPressed: function(which) {
      if (which === Qt.RightButton) {
        if (!reloadProc.running) reloadProc.running = true
      } else if (root.bar) {
        root.bar.run("omarchy-shell shell toggle io.github.iluxav.omaestro")
      }
    }
  }

  Process {
    id: statusProc
    command: [root.om, "status", "--json"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var out = String(text || "").trim()
        if (out === "") return
        try {
          var status = JSON.parse(out)
          root.running = true
          root.overrideOn = status.override === true
          root.loadError = status.load_error ? String(status.load_error) : ""
          root.triggers = Number(status.triggers || 0)
          root.disabled = Number(status.disabled || 0)
        } catch (e) {}
      }
    }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.stderrText = String(text || "").trim().replace(/^om: /, "")
    }
    onExited: function(exitCode) {
      root.checked = true
      if (exitCode !== 0) root.running = false
    }
  }

  Process {
    id: reloadProc
    command: [root.om, "reload"]
    onExited: root.refresh()
  }

  Timer {
    interval: 5000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refresh()
  }
}
