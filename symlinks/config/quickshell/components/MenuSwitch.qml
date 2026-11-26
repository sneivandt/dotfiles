import QtQuick
import "../lib/Theme.js" as Theme

Rectangle {
    id: root

    property bool checked: false
    property color accentColor: Theme.blue

    implicitWidth: 36
    implicitHeight: 20
    radius: height / 2
    color: checked ? accentColor : Theme.border
    opacity: enabled ? 1 : 0.6

    Rectangle {
        x: root.checked ? root.width - width - 3 : 3
        anchors.verticalCenter: parent.verticalCenter
        width: 14
        height: 14
        radius: width / 2
        color: Theme.foreground

        Behavior on x {
            NumberAnimation {
                duration: Theme.animationFast
                easing.type: Easing.OutCubic
            }
        }
    }

    Behavior on color {
        ColorAnimation {
            duration: Theme.animationFast
        }
    }
}
