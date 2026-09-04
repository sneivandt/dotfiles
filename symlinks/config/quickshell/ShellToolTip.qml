import QtQuick
import QtQuick.Controls
import "Theme.js" as Theme

ToolTip {
    id: root
    popupType: Popup.Window
    delay: 500
    timeout: 5000
    padding: 8
    font.family: Theme.font
    font.pixelSize: Theme.textSmall
    contentItem: Text {
        text: root.text
        textFormat: Text.PlainText
        font: root.font
        color: Theme.foreground
    }
    background: Rectangle {
        color: Theme.backgroundSolid
        radius: Theme.controlRadius
        border.color: Theme.border
    }
}
