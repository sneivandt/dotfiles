import QtQuick
import QtQuick.Controls
import "Theme.js" as Theme

AbstractButton {
    id: root

    property string glyph: ""
    property string tooltip: ""
    property color accentColor: Theme.blue
    property color accentBackground: Theme.blueSoft
    property bool selected: false
    signal triggered

    implicitWidth: 32
    implicitHeight: 32
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus
    opacity: enabled ? 1 : 0.45
    Accessible.name: tooltip
    onClicked: triggered()
    Keys.onReturnPressed: triggered()
    Keys.onEnterPressed: triggered()

    background: Rectangle {
        radius: Theme.controlRadius
        color: root.down ? Theme.pressed : (root.hovered ? Theme.hover : (root.selected ? root.accentBackground : "transparent"))
        border.width: root.visualFocus ? 1 : 0
        border.color: root.accentColor
        Behavior on color {
            ColorAnimation {
                duration: Theme.animationFast
            }
        }
    }
    contentItem: Text {
        text: root.glyph
        color: root.selected ? root.accentColor : Theme.foreground
        font.family: Theme.iconFont
        font.pixelSize: 14
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }
    ShellToolTip {
        text: root.tooltip
        visible: root.tooltip.length > 0 && (root.hovered || root.visualFocus) && !root.down
    }
    HoverHandler {
        cursorShape: Qt.PointingHandCursor
    }
}
