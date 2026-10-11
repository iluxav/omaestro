// The cards inside the Omarchy shell, in its theme. Service.qml loads this
// in a Loader, so a shell without these modules still runs the daemon.
import QtQuick
import qs.Commons

Cards {
  background: Color.popups.background
  foreground: Color.popups.text
  border: Color.popups.border
  fontFamily: Style.font.family
  radius: Style.cornerRadius
  borderWidth: Math.max(1, Style.space(2))
  bigSize: Math.round(Style.font.displayLarge * 2.5)
  lineSize: Style.font.display
  padding: Style.space(28)
}
