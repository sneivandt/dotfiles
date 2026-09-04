import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "Theme.js" as Theme

FocusScope {
    id: root

    required property var quote
    property bool expanded: false
    property real expansionProgress: expanded ? 1 : 0
    readonly property color accentColor: Number(quote.change) === 0 ? Theme.mutedStrong : (quote.change > 0 ? Theme.green : Theme.red)

    signal triggered

    function price(value) {
        return Number(value).toLocaleString(Qt.locale("en_US"), "f", 2);
    }

    function percent(value) {
        const number = Number(value);
        return (number >= 0 ? "+" : "") + number.toFixed(2) + "%";
    }

    implicitHeight: summary.height + details.implicitHeight * expansionProgress
    height: implicitHeight
    clip: true

    AbstractButton {
        id: summary

        width: parent.width
        height: 58
        padding: Theme.spacing
        hoverEnabled: true
        focusPolicy: Qt.StrongFocus
        Accessible.name: root.quote.symbol + ", " + root.quote.prefix + root.price(root.quote.price) + ", " + root.percent(root.quote.change)
        Accessible.description: root.expanded ? "Hide price history" : "Show price history"
        onClicked: root.triggered()
        Keys.onReturnPressed: event => {
            if (!event.isAutoRepeat)
                root.triggered();
        }
        Keys.onEnterPressed: event => {
            if (!event.isAutoRepeat)
                root.triggered();
        }

        background: Rectangle {
            radius: Theme.itemRadius
            color: summary.down ? Theme.pressed : (summary.hovered ? Theme.hover : (root.expanded ? Theme.blueSoft : "transparent"))
            border.width: summary.visualFocus ? 1 : 0
            border.color: Theme.blue
        }

        contentItem: RowLayout {
            spacing: Theme.spacing

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2

                Text {
                    Layout.fillWidth: true
                    text: root.quote.symbol
                    color: Theme.foreground
                    font.family: Theme.font
                    font.pixelSize: Theme.textBody
                    font.weight: Font.DemiBold
                    elide: Text.ElideRight
                }

                Text {
                    Layout.fillWidth: true
                    text: root.quote.name
                    color: Theme.mutedStrong
                    font.family: Theme.font
                    font.pixelSize: Theme.textSmall
                    elide: Text.ElideRight
                    textFormat: Text.PlainText
                }
            }

            ColumnLayout {
                spacing: 2

                Text {
                    Layout.alignment: Qt.AlignRight
                    text: root.quote.prefix + root.price(root.quote.price)
                    color: Theme.foreground
                    font.family: Theme.font
                    font.pixelSize: Theme.textBody
                    font.weight: Font.DemiBold
                }

                Text {
                    Layout.alignment: Qt.AlignRight
                    text: root.percent(root.quote.change)
                    color: root.accentColor
                    font.family: Theme.font
                    font.pixelSize: Theme.textSmall
                }
            }

            Text {
                text: "\uf107"
                color: root.expanded || summary.hovered || summary.visualFocus ? Theme.blue : Theme.mutedStrong
                font.family: Theme.iconFont
                font.pixelSize: Theme.textSmall
                rotation: root.expansionProgress * 180
            }
        }

        HoverHandler {
            cursorShape: Qt.PointingHandCursor
        }
    }

    StockDetails {
        id: details

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: summary.bottom
        height: implicitHeight
        visible: root.expansionProgress > 0
        opacity: root.expansionProgress
        quote: root.quote
    }

    Behavior on expansionProgress {
        NumberAnimation {
            duration: Theme.animationNormal
            easing.type: Easing.OutCubic
        }
    }
}
