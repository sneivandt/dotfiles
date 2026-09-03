import QtQuick
import QtQuick.Layouts
import "Theme.js" as Theme

RowLayout {
    id: root

    property string icon: ""
    property string title: ""
    property string subtitle: ""
    property color accentColor: Theme.blue
    property color accentBackground: Theme.blueSoft
    property alias trailingItem: trailingSlot.data

    spacing: 10

    Rectangle {
        visible: root.icon.length > 0
        implicitWidth: 34
        implicitHeight: 34
        radius: Theme.controlRadius
        color: root.accentBackground
        border.width: 1
        border.color: root.accentBackground

        Text {
            anchors.fill: parent
            text: root.icon
            color: root.accentColor
            font.family: Theme.iconFont
            font.pixelSize: 14
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            renderType: Text.NativeRendering
        }
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 1

        Text {
            Layout.fillWidth: true
            text: root.title
            color: Theme.foreground
            font.family: Theme.font
            font.pixelSize: 15
            font.weight: Font.DemiBold
            elide: Text.ElideRight
        }

        Text {
            visible: root.subtitle.length > 0
            Layout.fillWidth: true
            text: root.subtitle
            color: Theme.mutedStrong
            font.family: Theme.font
            font.pixelSize: 10
            elide: Text.ElideRight
        }
    }

    Item {
        id: trailingSlot

        implicitWidth: childrenRect.width
        implicitHeight: childrenRect.height
    }
}
