import QtQuick
import QtQuick.Layouts
import "Theme.js" as Theme

Rectangle {
    id: root

    property string icon: ""
    property string label: ""
    property string detail: ""
    property string trailing: ""
    property string trailingDetail: ""
    property color trailingDetailColor: Theme.mutedStrong
    property bool danger: false
    property bool selected: false
    property bool clickable: true
    property bool showChevron: true
    property color accentColor: danger ? Theme.red : Theme.blue
    property color accentBackground: danger ? Theme.redSoft : Theme.blueSoft

    signal triggered()

    implicitHeight: 52
    radius: Theme.itemRadius
    scale: mouse.pressed ? 0.99 : 1
    color: mouse.pressed ? Theme.pressed : (mouse.containsMouse ? Theme.hover : (selected ? accentBackground : Theme.raised))
    border.width: 1
    border.color: mouse.containsMouse || selected ? accentBackground : Theme.borderSubtle
    opacity: enabled ? 1 : 0.45

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 9
        anchors.rightMargin: 11
        spacing: 9

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
                text: root.label
                color: root.selected && root.danger ? Theme.red : Theme.foreground
                font.family: Theme.font
                font.pixelSize: 13
                font.weight: Font.DemiBold
                elide: Text.ElideRight
            }

            Text {
                visible: root.detail.length > 0
                Layout.fillWidth: true
                text: root.detail
                color: Theme.mutedStrong
                font.family: Theme.font
                font.pixelSize: 10
                elide: Text.ElideRight
            }
        }

        ColumnLayout {
            visible: root.trailing.length > 0 || root.trailingDetail.length > 0
            spacing: 1

            Text {
                visible: root.trailing.length > 0
                Layout.alignment: Qt.AlignRight
                text: root.trailing
                color: Theme.foreground
                font.family: Theme.font
                font.pixelSize: 12
                font.weight: Font.DemiBold
            }

            Text {
                visible: root.trailingDetail.length > 0
                Layout.alignment: Qt.AlignRight
                text: root.trailingDetail
                color: root.trailingDetailColor
                font.family: Theme.font
                font.pixelSize: 10
                font.weight: Font.DemiBold
            }
        }

        Text {
            visible: root.showChevron
            text: "\uf105"
            color: mouse.containsMouse ? root.accentColor : Theme.mutedStrong
            font.family: Theme.iconFont
            font.pixelSize: 11
        }
    }

    MouseArea {
        id: mouse

        anchors.fill: parent
        enabled: root.enabled
        hoverEnabled: true
        acceptedButtons: root.clickable ? Qt.LeftButton : Qt.NoButton
        cursorShape: root.clickable ? Qt.PointingHandCursor : Qt.ArrowCursor
        onClicked: root.triggered()
    }

    Behavior on color {
        ColorAnimation {
            duration: 120
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
