// The Omarchy plugin service: keeps the omaestro daemon running while the
// shell is up. It compiles nothing. `scripts/plugin-start.sh` fetches the
// prebuilt release binary (SHA256 pinned in release.sha256) on first use,
// then runs `om daemon`; the daemon reads ~/.config/omaestro like always.
import QtQuick
import Quickshell
import Quickshell.Io

Item {
  id: root

  property var shell: null
  property var manifest: null

  // This file's directory, as a path.
  readonly property string pluginDir: String(Qt.resolvedUrl(".")).replace(/^file:\/\//, "")

  // Back off when the daemon keeps dying (a bad session, a missing tool).
  property int restartDelayMs: 2000

  Process {
    id: daemon
    command: [root.pluginDir + "scripts/plugin-start.sh"]
    running: true
    stdout: SplitParser {
      onRead: data => console.log("omaestro: " + data)
    }
    stderr: SplitParser {
      onRead: data => console.log("omaestro: " + data)
    }
    onExited: (code, status) => {
      if (root.stopping) return
      if (code === 3) {
        // Another daemon answers (the systemd unit's, say): look again in a
        // minute, quietly, in case it goes away.
        restart.interval = 60000
      } else if (code === 0) {
        // A clean stop (`om restart`): straight back, no backoff.
        restart.interval = 2000
      } else {
        console.warn("omaestro daemon exited with " + code + "; restarting in " + root.restartDelayMs + " ms")
        restart.interval = root.restartDelayMs
        root.restartDelayMs = Math.min(root.restartDelayMs * 2, 60000)
      }
      restart.start()
    }
  }

  property bool stopping: false

  Timer {
    id: restart
    repeat: false
    onTriggered: if (!root.stopping) daemon.running = true
  }

  Component.onDestruction: {
    root.stopping = true
    daemon.running = false
  }
}
