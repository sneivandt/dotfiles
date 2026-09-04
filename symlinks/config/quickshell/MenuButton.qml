import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "Theme.js" as Theme

AbstractButton {
    id: root

    property string glyph: ""
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
    signal triggered

    implicitHeight: detail.length > 0 || trailingDetail.length > 0 ? 58 : 48
    implicitWidth: 180
    padding: 10
    hoverEnabled: clickable
    focusPolicy: clickable || activeFocus ? Qt.StrongFocus : Qt.NoFocus
    opacity: enabled ? 1 : 0.45
    Accessible.name: label
    Accessible.description: detail
    onClicked: if (clickable)
        triggered()
    Keys.onReturnPressed: if (clickable && enabled)
        triggered()
    Keys.onEnterPressed: if (clickable && enabled)
        triggered()

    background: Rectangle {
        radius: Theme.itemRadius
        color: root.clickable && root.down ? Theme.pressed : (root.clickable && root.hovered ? Theme.hover : (root.selected ? root.accentBackground : "transparent"))
        border.width: root.visualFocus ? 1 : 0
        border.color: root.accentColor
        Behavior on color {
            ColorAnimation {
                duration: Theme.animationFast
            }
        }
    }

    contentItem: RowLayout {
        spacing: 12
        Text {
            visible: root.glyph.length > 0
            Layout.preferredWidth: 20
            text: root.glyph
            color: root.danger || root.selected ? root.accentColor : Theme.mutedStrong
            font.family: Theme.iconFont
            font.pixelSize: 15
            horizontalAlignment: Text.AlignHCenter
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 3
            Text {
                Layout.fillWidth: true
                text: root.label
                textFormat: Text.PlainText
                color: root.danger ? Theme.red : Theme.foreground
                font.family: Theme.font
                font.pixelSize: Theme.textBody
                font.weight: root.selected ? Font.DemiBold : Font.Normal
                elide: Text.ElideRight
            }
            Text {
                visible: root.detail.length > 0
                Layout.fillWidth: true
                text: root.detail
                textFormat: Text.PlainText
                color: Theme.mutedStrong
                font.family: Theme.font
                font.pixelSize: Theme.textSmall
                elide: Text.ElideRight
            }
        }
        ColumnLayout {
            visible: root.trailing.length > 0 || root.trailingDetail.length > 0
            spacing: 3
            Text {
                visible: root.trailing.length > 0
                Layout.alignment: Qt.AlignRight
                text: root.trailing
                textFormat: Text.PlainText
                color: root.selected ? root.accentColor : Theme.foreground
                font.family: Theme.font
                font.pixelSize: Theme.textBody
            }
            Text {
                visible: root.trailingDetail.length > 0
                Layout.alignment: Qt.AlignRight
                text: root.trailingDetail
                textFormat: Text.PlainText
                color: root.trailingDetailColor
                font.family: Theme.font
                font.pixelSize: Theme.textSmall
            }
        }
        Text {
            visible: root.showChevron && root.clickable
            text: "\uf105"
            color: Theme.mutedStrong
            font.family: Theme.iconFont
            font.pixelSize: 11
        }
    }

    HoverHandler {
        cursorShape: root.clickable ? Qt.PointingHandCursor : Qt.ArrowCursor
    }
}
