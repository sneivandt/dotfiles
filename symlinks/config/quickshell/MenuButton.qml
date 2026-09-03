import QtQuick
import QtQuick.Layouts
import "Theme.js" as Theme

Rectangle {
    id: root

    property string icon: ""
    property string label: ""
    property string detail: ""
    property bool danger: false
    property bool selected: false

    signal triggered()

    implicitHeight: 42
    radius: 7
    color: mouse.containsMouse ? Theme.hover : (selected ? "#263651" : "transparent")
    border.width: selected ? 1 : 0
    border.color: Theme.blue
    opacity: enabled ? 1 : 0.45

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 10
        anchors.rightMargin: 10
        spacing: 10

        Text {
            text: root.icon
            color: root.danger ? Theme.red : Theme.blue
            font.family: Theme.iconFont
            font.pixelSize: 15
            Layout.preferredWidth: 20
            horizontalAlignment: Text.AlignHCenter
        }

        ColumnLayout {
            spacing: 0
            Layout.fillWidth: true

            Text {
                text: root.label
                color: root.danger ? Theme.red : Theme.foreground
                font.family: Theme.font
                font.pixelSize: 13
            }

            Text {
                visible: text.length > 0
                text: root.detail
                color: Theme.muted
                font.family: Theme.font
                font.pixelSize: 10
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
        }

        Text {
            text: "\uf105"
            color: Theme.muted
            font.family: Theme.iconFont
            font.pixelSize: 12
        }
    }

    MouseArea {
        id: mouse

        anchors.fill: parent
        enabled: root.enabled
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.triggered()
    }

    Behavior on color {
        ColorAnimation {
            duration: 120
        }
    }
}
