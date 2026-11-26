import QtQuick
import QtQuick.Controls
import "../lib/Theme.js" as Theme

Slider {
    id: root

    property bool muted: false
    readonly property color activeColor: muted ? Theme.mutedStrong : (value > 1 ? Theme.purple : Theme.blue)

    implicitHeight: 44
    from: 0
    to: 1.5
    stepSize: 0.01
    live: true
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus
    opacity: enabled ? 1 : 0.45
    Accessible.name: "Output volume"
    Accessible.description: "Use arrow keys to adjust volume from 0 to 150 percent. Above 100 percent amplifies audio."

    function snapDetent() {
        // Magnetic detent at 100% during pointer drags; arrow keys step freely.
        if (pressed && Math.abs(value - 1.0) <= 0.025)
            value = 1.0;
        return value;
    }

    HoverHandler {
        cursorShape: root.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
    }

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
            color: Theme.purpleSoft
        }

        Rectangle {
            width: root.visualPosition * parent.width
            height: parent.height
            radius: height / 2
            color: root.activeColor

            Behavior on color {
                ColorAnimation {
                    duration: Theme.animationFast
                }
            }
        }

        Rectangle {
            x: parent.width * 2 / 3 - width / 2
            y: -4
            width: 2
            height: parent.height + 8
            color: Theme.cyan
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
        border.color: root.activeColor
        scale: root.pressed ? 1.18 : (root.hovered ? 1.08 : 1.0)

        Behavior on scale {
            NumberAnimation {
                duration: Theme.animationFast
                easing.type: Easing.OutCubic
            }
        }

        Behavior on border.color {
            ColorAnimation {
                duration: Theme.animationFast
            }
        }

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
