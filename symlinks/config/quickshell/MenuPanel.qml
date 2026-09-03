import QtQuick
import "Theme.js" as Theme

Rectangle {
    radius: Theme.menuRadius
    color: Theme.menuBackground
    border.width: 1
    border.color: Theme.border

    Rectangle {
        anchors.fill: parent
        anchors.margins: 1
        radius: parent.radius - 1
        color: "transparent"
        border.width: 1
        border.color: Theme.highlight
        opacity: 0.55
    }
}
