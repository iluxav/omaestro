// The Omarchy plugin's panel: every rule the daemon has, grouped by the
// file it comes from, each with a switch; above them the override switch
// (whether rules may take chords Hyprland already uses), red while it is
// on; and a reload button. Summon it with
//   omarchy-shell shell toggle io.github.iluxav.omaestro
// (`om panel`, or the panel plugin's SUPER+ALT+O). Everything goes through
// the om command, which scripts/om locates: `om list --json` and
// `om status --json` fill the panel, a switch runs `om disable`/`om enable`
// or `om override on|off`, Reload runs `om reload`. What is switched stays
// so across reloads and daemon restarts.
import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Ui

Item {
  id: root

  property var shell: null
  property var manifest: null

  readonly property string pluginDir: String(Qt.resolvedUrl(".")).replace(/^file:\/\//, "")
  readonly property string om: pluginDir + "scripts/om"
  readonly property string pluginId: (manifest && manifest.id) || "io.github.iluxav.omaestro"

  property bool opened: false
  // Rows of `om list --json`: {id, kind, detail, origin, enabled, problem?, overrides?, bound?}.
  property var rules: []
  // From `om status --json`.
  property bool overrideOn: false
  property string loadError: ""
  // Why the list is empty, when it is.
  property string error: ""
  property string listStderr: ""
  property string actionStderr: ""
  // The rule whose switch is being flipped; its knob moves right away.
  property string busyId: ""
  // What the last enable, disable, override or reload answered.
  property string message: ""

  // The keys inside a mode go with the mode's own row.
  readonly property var shownRules: rules.filter(function(r) {
    return r.kind !== "mode_key" && r.kind !== "mode_exit"
  })
  // The same, sorted by file and line, each row knowing its section.
  readonly property var grouped: groupRules(shownRules)
  readonly property int offCount: shownRules.filter(function(r) { return !r.enabled }).length
  readonly property int refusedCount: shownRules.filter(function(r) { return r.enabled && r.problem }).length
  readonly property int takenCount: shownRules.filter(function(r) { return r.overrides }).length
  readonly property string summary: error !== "" ? ""
    : shownRules.length === 0 ? "no rules loaded"
    : shownRules.length + (shownRules.length === 1 ? " rule" : " rules")
      + (offCount > 0 ? ", " + offCount + " off" : "")
      + (refusedCount > 0 ? ", " + refusedCount + " without a bind" : "")

  function open(payloadJson) {
    root.opened = true
    root.message = ""
    refresh()
    // The window is instantiated hidden, so the content's `focus: true` is
    // evaluated before the surface is mapped and Escape would land nowhere.
    Qt.callLater(function() {
      if (root.opened) keyCatcher.forceActiveFocus()
    })
  }

  function close() {
    root.opened = false
    confirm.opened = false
  }

  function dismiss() {
    if (root.shell && typeof root.shell.hide === "function") root.shell.hide(root.pluginId)
    else close()
  }

  function refresh() {
    if (!listProc.running) {
      root.listStderr = ""
      listProc.running = true
    }
    if (!statusProc.running) statusProc.running = true
  }

  // Where a rule comes from: a plugin's name for lib/<name>/init.lua, else
  // the rule file itself.
  function sourceOf(rule) {
    var file = String(rule.origin).replace(/:\d+$/, "")
    var plugin = file.match(/^lib\/([^\/]+)\//)
    if (plugin) return { key: file, title: plugin[1] + "  (plugin)" }
    return { key: file, title: file }
  }

  function lineOf(rule) {
    var m = String(rule.origin).match(/:(\d+)$/)
    return m ? parseInt(m[1]) : 0
  }

  function groupRules(rules) {
    var sorted = rules.slice().sort(function(a, b) {
      var fa = sourceOf(a).key, fb = sourceOf(b).key
      if (fa !== fb) return fa < fb ? -1 : 1
      var la = lineOf(a), lb = lineOf(b)
      if (la !== lb) return la - lb
      return a.id < b.id ? -1 : a.id > b.id ? 1 : 0
    })
    var out = []
    var last = null
    for (var i = 0; i < sorted.length; i++) {
      var source = sourceOf(sorted[i])
      var row = Object.assign({}, sorted[i])
      row.section = source.title
      row.first = source.key !== last
      last = source.key
      out.push(row)
    }
    return out
  }

  // "hotkey  SUPER + ALT + J", "on_focus  class=firefox", "trigger  hello".
  // An app hotkey shows its chord; where it applies goes in the description.
  function label(rule) {
    if (rule.kind === "app_hotkey") return "hotkey  " + String(rule.detail).split(" in ")[0]
    return rule.kind + "  " + (rule.detail !== "" ? rule.detail : rule.id)
  }

  // The scope of an app hotkey, what the bind sync has to say, the origin.
  function describe(rule) {
    var notes = []
    if (rule.kind === "app_hotkey") {
      var app = String(rule.detail).split(" in ").slice(1).join(" in ")
      var state = rule.bound === true ? "bound now" : rule.bound === false ? "not bound now" : ""
      notes.push("only while " + app + " has focus" + (state ? " (" + state + ")" : ""))
    }
    if (rule.problem) notes.push("no bind: " + String(rule.problem).split(";")[0])
    else if (rule.overrides) notes.push("instead of " + rule.overrides)
    notes.push(rule.origin)
    return notes.join("  ·  ")
  }

  function run(args) {
    if (actionProc.running) return false
    root.message = ""
    root.actionStderr = ""
    actionProc.command = [root.om].concat(args)
    actionProc.running = true
    return true
  }

  function setEnabled(id, enabled) {
    if (run([enabled ? "enable" : "disable", id])) root.busyId = id
  }

  function setOverride(on) {
    confirm.opened = false
    run(["override", on ? "on" : "off"])
  }

  // Turning override on goes through the warning first.
  function askOverride() {
    confirm.opened = true
  }

  function cancelOverride() {
    confirm.opened = false
  }

  function reload() {
    run(["reload"])
  }

  // What a failed om command means to the person looking at the panel.
  function explain(stderr) {
    var text = String(stderr || "").trim()
    if (text.indexOf("Connection refused") !== -1 || text.indexOf("No such file") !== -1)
      return "The daemon is not running. `om doctor` says why."
    return text !== "" ? text.replace(/^om: /, "") : "om failed"
  }

  Process {
    id: listProc
    command: [root.om, "list", "--json"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var out = String(text || "").trim()
        if (out === "") return
        try {
          root.rules = JSON.parse(out)
          root.error = ""
        } catch (e) {
          root.error = "om list answered something that is not JSON (version mismatch?)"
        }
      }
    }
    // Exit and stream-finished have no guaranteed order: when a failed exit
    // beat the collector and published the generic message, replace it with
    // the specific one once it lands.
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        root.listStderr = String(text || "").trim()
        if (root.error !== "" && root.listStderr !== "") root.error = root.explain(root.listStderr)
      }
    }
    onExited: function(exitCode) {
      if (exitCode !== 0) {
        root.rules = []
        root.error = root.explain(root.listStderr)
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
          root.overrideOn = status.override === true
          root.loadError = status.load_error ? String(status.load_error) : ""
        } catch (e) {}
      }
    }
  }

  Process {
    id: actionProc
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var out = String(text || "").trim()
        if (out !== "") root.message = out
      }
    }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        root.actionStderr = String(text || "").trim()
        if (root.actionStderr !== "") root.message = root.explain(root.actionStderr)
      }
    }
    onExited: function(exitCode) {
      root.busyId = ""
      if (exitCode !== 0 && root.message === "") root.message = root.explain(root.actionStderr)
      root.refresh()
    }
  }

  PanelWindow {
    id: window
    visible: root.opened
    anchors { top: true; bottom: true; left: true; right: true }
    color: "transparent"
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.namespace: "omaestro-rules"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive

    // The scrim: a click outside the card closes the panel.
    Rectangle {
      anchors.fill: parent
      color: Color.menu.scrim
      MouseArea {
        anchors.fill: parent
        onClicked: root.dismiss()
      }
    }

    Item {
      id: keyCatcher
      anchors.fill: parent
      focus: true
      Keys.onPressed: function(event) {
        if (confirm.handleKey(event)) {
          event.accepted = true
        } else if (event.key === Qt.Key_Escape) {
          root.dismiss()
          event.accepted = true
        }
      }

      Rectangle {
        id: card
        anchors.centerIn: parent
        width: Math.min(Style.space(600), keyCatcher.width - Style.space(32))
        height: Math.min(content.implicitHeight + Style.spacing.huge * 2,
                         keyCatcher.height - Style.space(64))
        color: Color.popups.background
        border.color: Color.popups.border
        border.width: Math.max(1, Style.space(2))
        radius: Style.cornerRadius

        // Clicks on the card stay on the card.
        MouseArea {
          anchors.fill: parent
          onClicked: {}
        }

        ColumnLayout {
          id: content
          anchors.fill: parent
          anchors.margins: Style.spacing.huge
          spacing: Style.spacing.lg

          RowLayout {
            Layout.fillWidth: true
            spacing: Style.spacing.lg

            Column {
              Layout.fillWidth: true
              spacing: Style.spacing.xs
              Text {
                textFormat: Text.PlainText
                text: "omaestro"
                color: Color.popups.text
                font.family: Style.font.family
                font.pixelSize: Style.font.heading
                font.bold: true
              }
              Text {
                textFormat: Text.PlainText
                text: root.summary
                visible: text !== ""
                color: Qt.darker(Color.popups.text, 1.4)
                font.family: Style.font.family
                font.pixelSize: Style.font.caption
              }
            }

            Button {
              text: "Reload"
              tooltipText: "Load the rule files again"
              bordered: true
              foreground: Color.popups.text
              onClicked: root.reload()
            }
            Button {
              text: "Close"
              bordered: true
              foreground: Color.popups.text
              onClicked: root.dismiss()
            }
          }

          // Who wins a chord: a setting of the whole panel, above the rules
          // and not shaped like one. Off, Hyprland's own binds win and a rule
          // on one of them is refused; on (after a warning), the rules win.
          RowLayout {
            Layout.fillWidth: true
            spacing: Style.spacing.lg

            ToggleSwitch {
              Layout.alignment: Qt.AlignVCenter
              checked: root.overrideOn
              foreground: root.overrideOn ? Color.urgent : Color.popups.text
              accent: root.overrideOn ? Color.urgent : Color.accent
              onToggled: root.overrideOn ? root.setOverride(false) : root.askOverride()
            }

            Column {
              Layout.fillWidth: true
              spacing: Style.spacing.xxs
              Text {
                width: parent.width
                textFormat: Text.PlainText
                elide: Text.ElideRight
                text: root.overrideOn
                  ? "Override on: rules take over Hyprland's own shortcuts"
                    + (root.takenCount > 0 ? " (" + root.takenCount + " taken)" : "")
                  : "Override off: Hyprland's own shortcuts win"
                color: root.overrideOn ? Color.urgent : Color.popups.text
                font.family: Style.font.family
                font.pixelSize: Style.font.body
                font.bold: root.overrideOn
              }
              Text {
                width: parent.width
                textFormat: Text.PlainText
                elide: Text.ElideRight
                text: root.overrideOn
                  ? "A replaced shortcut comes back when its rule goes or this is turned off"
                  : "A rule on a chord Omarchy or your config already uses is refused and says so"
                color: Qt.darker(Color.popups.text, 1.4)
                font.family: Style.font.family
                font.pixelSize: Style.font.caption
              }
            }
          }

          PanelSeparator {
            Layout.fillWidth: true
            foreground: Color.popups.text
          }

          ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            implicitHeight: contentHeight
            visible: root.grouped.length > 0
            clip: true
            spacing: Style.spacing.sm
            model: root.grouped
            delegate: Column {
              required property var modelData
              required property int index
              width: list.width
              spacing: Style.spacing.sm
              // The file (or plugin) the next rows come from.
              PanelSectionHeader {
                visible: modelData.first
                text: modelData.section
                foreground: Color.popups.text
                topPadding: modelData.first && index > 0 ? Style.spacing.lg : Style.spacing.xs
              }
              Toggle {
                width: parent.width
                label: root.label(modelData)
                description: root.describe(modelData)
                checked: root.busyId === modelData.id ? !modelData.enabled : modelData.enabled
                foreground: modelData.problem && modelData.enabled ? Color.urgent : Color.popups.text
                onClicked: root.setEnabled(modelData.id, !modelData.enabled)
              }
            }
          }

          Text {
            Layout.fillWidth: true
            visible: root.error !== "" || root.shownRules.length === 0
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            text: root.error !== "" ? root.error
                : "No rules yet. `om plugin available` lists the plugins to start with; put rules in ~/.config/omaestro/rules.d/ and they load on save."
            color: root.error !== "" ? Color.urgent : Qt.darker(Color.popups.text, 1.4)
            font.family: Style.font.family
            font.pixelSize: Style.font.body
          }

          Text {
            Layout.fillWidth: true
            visible: root.loadError !== "" || root.message !== ""
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            text: root.loadError !== "" ? "Last load failed: " + root.loadError : root.message
            color: root.loadError !== "" ? Color.urgent : Qt.darker(Color.popups.text, 1.4)
            font.family: Style.font.family
            font.pixelSize: Style.font.caption
          }
        }
      }

      ConfirmDialog {
        id: confirm
        anchors.fill: parent
        message: "Rules will take over shortcuts Omarchy or your Hyprland config already use. "
          + "A replaced shortcut stops working while its rule is loaded and switched on; "
          + "it comes back (Hyprland reloads its config) when the rule goes or this is turned off."
        confirmText: "Override"
        background: Color.popups.background
        foreground: Color.popups.text
        onConfirmed: root.setOverride(true)
        onCanceled: root.cancelOverride()
      }
    }
  }
}
