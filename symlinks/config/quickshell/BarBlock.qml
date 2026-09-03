import QtQuick
import "Theme.js" as Theme

Rectangle {
    id: root

    property alias text: label.text
    property alias textColor: label.color
    property alias fontFamily: label.font.family
    property alias fontPixelSize: label.font.pixelSize
    property alias textFormat: label.textFormat
    property int horizontalPadding: 12
    property bool interactive: true

    signal activated(int button)
    signal scrolled(int steps)

    implicitWidth: label.implicitWidth + horizontalPadding * 2
    implicitHeight: 22
    radius: 6
    color: mouse.containsMouse && interactive ? Theme.hover : Theme.background

    Text {
        id: label

        anchors.centerIn: parent
        color: Theme.foreground
        font.family: Theme.font
        font.pixelSize: 14
        textFormat: Text.PlainText
        verticalAlignment: Text.AlignVCenter
    }

    MouseArea {
        id: mouse

        anchors.fill: parent
        enabled: root.interactive
        acceptedButtons: Qt.AllButtons
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: (event) => {
            return root.activated(event.button);
        }
        onWheel: (event) => {
            return root.scrolled(event.angleDelta.y > 0 ? 1 : -1);
        }
    }

    Behavior on color {
        ColorAnimation {
            duration: 120
        }
    }
}
