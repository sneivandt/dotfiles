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

    spacing: 12

    Text {
        visible: root.icon.length > 0
        Layout.preferredWidth: 20
        text: root.icon
        color: root.accentColor
        font.family: Theme.iconFont
        font.pixelSize: 17
        horizontalAlignment: Text.AlignHCenter
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 4

        Text {
            Layout.fillWidth: true
            text: root.title
            textFormat: Text.PlainText
            color: Theme.foreground
            font.family: Theme.font
            font.pixelSize: Theme.textHeading
            font.weight: Font.DemiBold
            elide: Text.ElideRight
        }

        Text {
            visible: root.subtitle.length > 0
            Layout.fillWidth: true
            text: root.subtitle
            textFormat: Text.PlainText
            color: Theme.mutedStrong
            font.family: Theme.font
            font.pixelSize: Theme.textSmall
            elide: Text.ElideRight
        }
    }

    Item {
        id: trailingSlot

        implicitWidth: childrenRect.width
        implicitHeight: childrenRect.height
    }
}
