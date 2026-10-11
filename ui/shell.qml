// omaestro's own quickshell, for a session without the Omarchy shell (or
// with omaestro not loaded in it). The daemon writes this folder under
// $XDG_RUNTIME_DIR and runs `quickshell -p` on it for the first card. The
// colors are the Omarchy theme's when there is one.
import QtQuick
import Quickshell
import Quickshell.Io

ShellRoot {
  id: shell

  // `name = "#rrggbb"` lines of the theme's colors.toml.
  property var colors: ({})
  readonly property string daemonPid: Quickshell.env("OMAESTRO_PID") || ""

  function parseColors(text) {
    var found = {}
    var lines = String(text || "").split("\n")
    for (var i = 0; i < lines.length; i++) {
      var match = lines[i].match(/^\s*([a-z_]+)\s*=\s*"(#[0-9a-fA-F]{6,8})"/)
      if (match) found[match[1]] = match[2]
    }
    return found
  }

  FileView {
    path: Quickshell.env("HOME") + "/.local/state/omarchy/current/theme/colors.toml"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: shell.colors = shell.parseColors(text())
    onLoadFailed: shell.colors = ({})
  }

  Cards {
    background: shell.colors.background || "#101315"
    foreground: shell.colors.foreground || "#cacccc"
    border: shell.colors.accent || "#cacccc"
  }

  // Quit when the daemon that started us is gone, so a crash leaves
  // nothing behind; the next daemon starts its own.
  Timer {
    interval: 5000
    repeat: true
    running: shell.daemonPid !== ""
    onTriggered: daemonCheck.running = true
  }

  Process {
    id: daemonCheck
    command: ["test", "-d", "/proc/" + shell.daemonPid]
    onExited: code => {
      if (code !== 0) Qt.quit()
    }
  }
}
