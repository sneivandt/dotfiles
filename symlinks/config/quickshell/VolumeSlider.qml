import QtQuick
import QtQuick.Controls
import "Theme.js" as Theme

Slider {
    id: root

    implicitHeight: 44
    from: 0
    to: 1.5
    stepSize: 0.01
    live: true
    focusPolicy: Qt.StrongFocus
    opacity: enabled ? 1 : 0.45
    Accessible.name: "Output volume"
    Accessible.description: "Use arrow keys to adjust volume from 0 to 150 percent. Above 100 percent amplifies audio."

    background: Item {
        x: root.leftPadding + root.handle.width / 2
        y: root.topPadding + root.availableHeight / 2 - height / 2
        width: root.availableWidth - root.handle.width
        height: 8

        Rectangle {
            anchors.fill: parent
            radius: height / 2
            color: Theme.border
        }

        Rectangle {
            x: parent.width * 2 / 3
            width: parent.width / 3
            height: parent.height
            radius: height / 2
            color: Theme.yellowSoft
        }

        Rectangle {
            width: root.visualPosition * parent.width
            height: parent.height
            radius: height / 2
            color: root.value > 1 ? Theme.yellow : Theme.blue
        }

        Rectangle {
            x: parent.width * 2 / 3 - width / 2
            y: -4
            width: 2
            height: parent.height + 8
            color: Theme.yellow
        }
    }

    handle: Rectangle {
        x: root.leftPadding + root.visualPosition * (root.availableWidth - width)
        y: root.topPadding + root.availableHeight / 2 - height / 2
        implicitWidth: 14
        implicitHeight: 14
        radius: width / 2
        color: Theme.foreground
        border.width: 2
        border.color: root.value > 1 ? Theme.yellow : Theme.blue

        Rectangle {
            anchors.fill: parent
            anchors.margins: -3
            radius: width / 2
            color: "transparent"
            border.width: root.visualFocus ? 1 : 0
            border.color: Theme.foreground
        }
    }
}
