// The cards `om.show` puts on the screen, one at a time: over everything on
// the focused monitor, never taking the keyboard or a click. The daemon
// sends them over quickshell IPC (target `omaestro.ui`) as JSON:
// {id, big, lines, ms, position}. `show` replaces whatever card is up
// (ms 0: until closed); `update` changes the text of the card with that id
// while it is up, for the time it has left, and answers "gone" otherwise.
//
// Two hosts load this: OmarchyCards.qml inside the Omarchy shell, in its
// theme, and shell.qml, omaestro's own quickshell where there is no Omarchy
// shell. The look is the properties below; each host sets them.
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Hyprland

Item {
  id: root

  property color background: "#101315"
  property color foreground: "#cacccc"
  property color border: "#cacccc"
  property string fontFamily: "monospace"
  property int radius: 0
  property int borderWidth: 2
  property int bigSize: 72
  property int lineSize: 22
  property int padding: 28

  // The card on screen.
  property int cardId: 0
  property string big: ""
  property var lines: []
  property string position: "center"
  property bool opened: false
  property var targetScreen: null

  function parse(json) {
    try {
      return JSON.parse(json)
    } catch (e) {
      return null
    }
  }

  function setText(card) {
    root.big = String(card.big || "")
    root.lines = (card.lines || []).map(String)
  }

  function show(json) {
    var card = root.parse(json)
    if (!card) return "not a card: " + json
    // On the monitor in use; showing again a card that is up leaves it there.
    if (!root.opened || card.id !== root.cardId) root.targetScreen = root.focusedScreen()
    root.cardId = card.id
    root.setText(card)
    root.position = String(card.position || "center")
    root.opened = true
    var ms = Number(card.ms || 0)
    if (ms > 0) {
      hideTimer.interval = ms
      hideTimer.restart()
    } else {
      hideTimer.stop()
    }
    return "ok"
  }

  function update(json) {
    var card = root.parse(json)
    if (!card) return "not a card: " + json
    if (!root.opened || card.id !== root.cardId) return "gone"
    root.setText(card)
    return "ok"
  }

  function close(id) {
    if (id !== root.cardId) return
    hideTimer.stop()
    root.opened = false
  }

  function focusedScreen() {
    var monitor = Hyprland.focusedMonitor
    var name = monitor ? String(monitor.name || "") : ""
    var screens = Quickshell.screens
    for (var i = 0; i < screens.length; i++) {
      if (screens[i].name === name) return screens[i]
    }
    return screens.length > 0 ? screens[0] : null
  }

  Timer {
    id: hideTimer
    onTriggered: root.opened = false
  }

  IpcHandler {
    target: "omaestro.ui"
    function show(card: string): string { return root.show(card) }
    function update(card: string): string { return root.update(card) }
    function close(id: int): string { root.close(id); return "ok" }
    function state(): string { return root.opened ? "open " + root.cardId : "closed" }
    function ping(): string { return "ok" }
  }

  PanelWindow {
    id: window
    screen: root.targetScreen
    visible: root.opened || card.opacity > 0
    anchors { top: true; bottom: true; left: true; right: true }
    color: "transparent"
    WlrLayershell.namespace: "omaestro-card"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    exclusionMode: ExclusionMode.Ignore
    // Seen, never in the way: an empty input region lets clicks through.
    mask: Region {}

    Rectangle {
      id: card
      readonly property real maxTextWidth: window.width * 0.8 - 2 * root.padding
      width: column.width + 2 * root.padding
      height: column.height + 2 * root.padding
      x: Math.round((window.width - width) / 2)
      y: Math.round(root.position === "top" ? window.height * 0.12
        : root.position === "bottom" ? window.height * 0.88 - height
        : (window.height - height) / 2)
      color: root.background
      border.color: root.border
      border.width: root.borderWidth
      radius: root.radius
      opacity: root.opened ? 1 : 0

      Behavior on opacity {
        NumberAnimation { duration: 150; easing.type: Easing.OutCubic }
      }

      Column {
        id: column
        anchors.centerIn: parent
        spacing: Math.round(root.lineSize * 0.4)

        Text {
          visible: root.big !== ""
          anchors.horizontalCenter: parent.horizontalCenter
          width: Math.min(implicitWidth, card.maxTextWidth)
          text: root.big
          textFormat: Text.PlainText
          wrapMode: Text.Wrap
          horizontalAlignment: Text.AlignHCenter
          font.family: root.fontFamily
          font.pixelSize: root.bigSize
          font.bold: true
          color: root.foreground
        }

        Repeater {
          model: root.lines

          Text {
            required property string modelData
            anchors.horizontalCenter: parent.horizontalCenter
            width: Math.min(implicitWidth, card.maxTextWidth)
            text: modelData
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            horizontalAlignment: Text.AlignHCenter
            font.family: root.fontFamily
            font.pixelSize: root.lineSize
            color: root.foreground
            opacity: 0.85
          }
        }
      }
    }
  }
}
