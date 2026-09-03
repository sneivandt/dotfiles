import QtQuick
import "Theme.js" as Theme

Rectangle {
    id: root

    property string icon: ""
    property color accentColor: Theme.blue
    property color accentBackground: Theme.blueSoft
    property bool selected: false

    signal triggered()

    implicitWidth: 32
    implicitHeight: 32
    radius: Theme.controlRadius
    scale: mouse.pressed ? 0.94 : 1
    color: mouse.pressed ? Theme.pressed : (mouse.containsMouse ? Theme.hover : (selected ? accentBackground : "transparent"))
    border.width: 1
    border.color: mouse.containsMouse || selected ? accentBackground : Theme.borderSubtle

    Text {
        anchors.fill: parent
        text: root.icon
        color: root.accentColor
        font.family: Theme.iconFont
        font.pixelSize: 13
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        renderType: Text.NativeRendering
    }

    MouseArea {
        id: mouse

        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.triggered()
    }

    Behavior on color {
        ColorAnimation {
            duration: Theme.animationFast
        }
    }

    Behavior on border.color {
        ColorAnimation {
            duration: Theme.animationFast
        }
    }

    Behavior on scale {
        NumberAnimation {
            duration: Theme.animationFast
            easing.type: Easing.OutCubic
        }
    }
}
