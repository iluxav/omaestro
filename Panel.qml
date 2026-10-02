// The Omarchy plugin's panel: every rule the daemon has, by its label,
// grouped by the plugin or file it comes from, each with a switch; above
// them the override switch (whether rules may take chords Hyprland already
// uses), red while it is on; a reload button; under them a field that
// installs a plugin from its URL. Summon it with
//   omarchy-shell shell toggle io.github.iluxav.omaestro
// (`om panel`, or SUPER+ALT+O from init.lua). Everything goes through the
// om command, which scripts/om locates: `om list --json` and
// `om status --json` fill the panel, a switch runs `om disable`/`om enable`
// or `om override on|off`, Reload runs `om reload`, a plugin's Configure
// opens `om plugin configure` in a terminal and Remove runs
// `om plugin remove`, a file's Edit opens it in Omarchy's editor, Add runs
// `om plugin add`. What is switched stays so across reloads and restarts.
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
  // Rows of `om list --json`: {id, kind, detail, label?, origin, enabled, problem?, overrides?, bound?}.
  property var rules: []
  // From `om status --json`.
  property bool overrideOn: false
  property string loadError: ""
  property string configDir: ""
  // Why the list is empty, when it is.
  property string error: ""
  property string listStderr: ""
  property string actionStderr: ""
  // The rule whose switch is being flipped; its knob moves right away.
  property string busyId: ""
  // What the last enable, disable, override, reload, add or remove answered.
  property string message: ""
  // The plugin the remove dialog asks about.
  property string removing: ""

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
    removeConfirm.opened = false
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

  // Where a rule comes from: a plugin (lib/<name>/...), init.lua (omaestro's
  // own, as the panel's chord is), or another rule file.
  function sourceOf(rule) {
    var file = String(rule.origin).replace(/:\d+$/, "")
    var plugin = file.match(/^lib\/([^\/]+)\//)
    if (plugin) return { key: "lib/" + plugin[1], title: plugin[1] + "  (plugin)", plugin: plugin[1], file: "" }
    if (file === "init.lua") return { key: file, title: "omaestro", plugin: "", file: file }
    return { key: file, title: file, plugin: "", file: file }
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
      row.plugin = source.plugin
      row.file = source.file
      row.first = source.key !== last
      last = source.key
      out.push(row)
    }
    return out
  }

  // What sets the rule off: a chord as it is ("SUPER + ALT + J"), else the
  // kind and its detail ("every  45m", "on_focus  class=firefox"). An app
  // hotkey shows its chord; where it applies goes in the description.
  function what(rule) {
    if (rule.kind === "app_hotkey") return String(rule.detail).split(" in ")[0]
    if (rule.kind === "hotkey" || rule.kind === "mode") return rule.detail
    return rule.kind + "  " + (rule.detail !== "" ? rule.detail : rule.id)
  }

  // The rule's own name when it gave one, else what sets it off.
  function label(rule) {
    return rule.label ? rule.label : what(rule)
  }

  // What sets it off (when the label did not say), the scope of an app
  // hotkey, what the bind sync has to say, the origin.
  function describe(rule) {
    var notes = []
    if (rule.label) notes.push(what(rule))
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

  // Plugins go in the config the daemon runs on (`om status` says which).
  // `om plugin configure` needs a terminal for the editor; it stays open
  // with the error when there is one. The panel goes away so the terminal
  // gets the keyboard.
  function configure(plugin) {
    if (root.configDir === "") return
    Util.execArgv(["omarchy-launch-tui", "--app-id=org.omarchy.omaestro", "bash", "-c",
      '"$0" plugin configure "$1" --config-dir "$2" || { echo; read -rp "Press Enter to close "; }',
      root.om, plugin, root.configDir])
    root.dismiss()
  }

  // A rule file of your own, in Omarchy's editor.
  function edit(file) {
    if (root.configDir === "") return
    Util.execArgv(["omarchy-launch-editor", root.configDir + "/" + file])
    root.dismiss()
  }

  function askRemove(plugin) {
    root.removing = plugin
    removeConfirm.opened = true
  }

  function remove() {
    removeConfirm.opened = false
    if (root.removing !== "" && root.configDir !== "")
      run(["plugin", "remove", root.removing, "--config-dir", root.configDir])
    root.removing = ""
  }

  // From the field, or `url` when given.
  function addPlugin(url) {
    url = String(url || addField.text || "").trim()
    if (url === "" || actionProc.running || root.configDir === "") return
    if (run(["plugin", "add", url, "--config-dir", root.configDir])) {
      root.message = "Installing " + url + "…"
      addField.text = ""
    }
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
          root.configDir = status.config_dir ? String(status.config_dir) : ""
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
        else if (root.message.indexOf("Installing ") === 0) root.message = ""
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
      // An added or removed plugin reaches the list once the daemon has
      // reloaded the rules (a moment after the files change).
      settle.restart()
    }
  }

  Timer {
    id: settle
    interval: 800
    onTriggered: root.refresh()
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
        if (confirm.handleKey(event) || removeConfirm.handleKey(event)) {
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

            // The logo, from the plugin's own directory.
            Image {
              Layout.alignment: Qt.AlignVCenter
              source: Qt.resolvedUrl("assets/omaestro-logo-512.png")
              sourceSize.width: Style.space(44)
              sourceSize.height: Style.space(44)
              fillMode: Image.PreserveAspectFit
              smooth: true
            }

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
          // and not shaped like one. Off, the binds Omarchy and the user's
          // config already have win and a rule on one of them is refused;
          // on (after a warning), the rules take them.
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
                text: "Override existing shortcuts"
                  + (root.overrideOn && root.takenCount > 0 ? " (" + root.takenCount + " taken over)" : "")
                color: root.overrideOn ? Color.urgent : Color.popups.text
                font.family: Style.font.family
                font.pixelSize: Style.font.body
                font.bold: root.overrideOn
              }
              Text {
                width: parent.width
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                text: root.overrideOn
                  ? "On: rules take those chords; the originals come back when this is turned off"
                  : "Off: a rule on a chord Omarchy or your config already uses is not bound"
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

          // The rows, in a column so the card grows to fit them exactly (a
          // ListView only estimates its content height); when the screen is
          // too small the column scrolls.
          Flickable {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            implicitHeight: rows.implicitHeight
            contentHeight: rows.implicitHeight
            visible: root.grouped.length > 0
            clip: true
            boundsBehavior: Flickable.StopAtBounds

            Column {
              id: rows
              width: list.width
              spacing: Style.spacing.sm

              Repeater {
                model: root.grouped
                delegate: Column {
                  required property var modelData
                  required property int index
                  width: rows.width
                  spacing: Style.spacing.sm
                  // The plugin (or file) the next rows come from, and what
                  // can be done with it.
                  RowLayout {
                    visible: modelData.first
                    width: parent.width
                    spacing: Style.spacing.sm
                    PanelSectionHeader {
                      Layout.fillWidth: true
                      Layout.alignment: Qt.AlignBottom
                      text: modelData.section
                      foreground: Color.popups.text
                      topPadding: modelData.first && index > 0 ? Style.spacing.lg : Style.spacing.xs
                    }
                    Button {
                      visible: modelData.plugin !== "" && root.configDir !== ""
                      text: "Configure"
                      tooltipText: "Its options, in your editor"
                      bordered: true
                      fontSize: Style.font.caption
                      foreground: Color.popups.text
                      onClicked: root.configure(modelData.plugin)
                    }
                    Button {
                      visible: modelData.plugin !== "" && root.configDir !== ""
                      text: "Remove"
                      tooltipText: "Uninstall it and the rule that loads it"
                      bordered: true
                      fontSize: Style.font.caption
                      foreground: Color.popups.text
                      onClicked: root.askRemove(modelData.plugin)
                    }
                    Button {
                      visible: modelData.file !== "" && root.configDir !== ""
                      text: "Edit"
                      tooltipText: "Open " + modelData.file + " in your editor"
                      bordered: true
                      fontSize: Style.font.caption
                      foreground: Color.popups.text
                      onClicked: root.edit(modelData.file)
                    }
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
            }
          }

          Text {
            Layout.fillWidth: true
            visible: root.error !== "" || root.shownRules.length === 0
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            text: root.error !== "" ? root.error
                : "No rules yet. Add a plugin by its URL below, or put rules in ~/.config/omaestro/rules.d/; they load on save."
            color: root.error !== "" ? Color.urgent : Qt.darker(Color.popups.text, 1.4)
            font.family: Style.font.family
            font.pixelSize: Style.font.body
          }

          // A plugin from its repository: a GitHub URL (a /tree/<branch>/<dir>
          // link for one inside a repository), any git URL, or a directory.
          RowLayout {
            Layout.fillWidth: true
            spacing: Style.spacing.sm
            visible: root.error === ""

            TextField {
              id: addField
              Layout.fillWidth: true
              placeholderText: "https://github.com/you/plugin"
              foreground: Color.popups.text
              enabled: !actionProc.running
              onAccepted: root.addPlugin()
            }
            Button {
              text: actionProc.running && root.message.indexOf("Installing ") === 0 ? "Adding…" : "Add plugin"
              tooltipText: "Install the plugin at this URL with its defaults"
              bordered: true
              foreground: Color.popups.text
              onClicked: root.addPlugin()
            }
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
        message: "Rules will take chords that Omarchy or your Hyprland config already use. "
          + "Those shortcuts stop working while the rule is on, and come back when it is "
          + "removed or switched off, or when override is turned off."
        confirmText: "Override"
        background: Color.popups.background
        foreground: Color.popups.text
        onConfirmed: root.setOverride(true)
        onCanceled: root.cancelOverride()
      }

      ConfirmDialog {
        id: removeConfirm
        anchors.fill: parent
        message: "Remove the plugin " + root.removing + "? Its files go from ~/.config/omaestro/lib, "
          + "and so does the rule om wrote for it. A plugin you changed is kept, and says so."
        confirmText: "Remove"
        background: Color.popups.background
        foreground: Color.popups.text
        onConfirmed: root.remove()
        onCanceled: root.removing = ""
      }
    }
  }
}
