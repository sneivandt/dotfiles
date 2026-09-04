import QtQuick
import QtQuick.Controls
import "Theme.js" as Theme

ScrollBar {
    id: root
    padding: 2
    minimumSize: 0.08
    policy: ScrollBar.AsNeeded
    contentItem: Rectangle {
        implicitWidth: 4
        implicitHeight: 4
        radius: 2
        color: root.pressed ? Theme.blue : Theme.mutedStrong
        opacity: root.size < 1 ? (root.active || root.hovered ? 0.9 : 0.4) : 0
        Behavior on opacity {
            NumberAnimation {
                duration: Theme.animationFast
            }
        }
    }
    background: Item {}
}
