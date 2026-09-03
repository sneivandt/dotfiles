import QtQuick
import QtQuick.Layouts
import "Theme.js" as Theme

Rectangle {
    id: root

    required property var quote
    property bool expanded: false
    property real expansionProgress: expanded ? 1 : 0

    signal triggered()

    function price(value) {
        return Number(value).toLocaleString(Qt.locale("en_US"), "f", 2);
    }

    function percent(value) {
        const number = Number(value);
        return (number >= 0 ? "+" : "") + number.toFixed(2) + "%";
    }

    implicitHeight: 62 + expansionProgress * 134
    radius: Theme.itemRadius
    clip: true
    color: cardMouse.pressed ? Theme.pressed : (cardMouse.containsMouse ? Theme.hover : Theme.raised)
    border.width: 1
    border.color: expanded ? root.accentColor : (cardMouse.containsMouse ? Theme.border : Theme.borderSubtle)
    readonly property color accentColor: quote.change >= 0 ? Theme.green : Theme.red
    readonly property color accentBackground: quote.change >= 0 ? Theme.greenSoft : Theme.redSoft

    Item {
        id: summary

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: 62

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 11
            spacing: 10

            Rectangle {
                implicitWidth: 36
                implicitHeight: 36
                radius: Theme.controlRadius
                color: root.accentBackground

                Text {
                    anchors.centerIn: parent
                    text: root.quote.change >= 0 ? "\uf062" : "\uf063"
                    color: root.accentColor
                    font.family: Theme.iconFont
                    font.pixelSize: 13
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 1

                Text {
                    Layout.fillWidth: true
                    text: root.quote.symbol
                    color: Theme.foreground
                    font.family: Theme.font
                    font.pixelSize: 13
                    font.weight: Font.DemiBold
                    elide: Text.ElideRight
                }

                Text {
                    Layout.fillWidth: true
                    text: root.quote.name
                    color: Theme.mutedStrong
                    font.family: Theme.font
                    font.pixelSize: 10
                    elide: Text.ElideRight
                }
            }

            ColumnLayout {
                spacing: 1

                Text {
                    Layout.alignment: Qt.AlignRight
                    text: root.quote.prefix + root.price(root.quote.price)
                    color: Theme.foreground
                    font.family: Theme.font
                    font.pixelSize: 12
                    font.weight: Font.DemiBold
                }

                Text {
                    Layout.alignment: Qt.AlignRight
                    text: root.percent(root.quote.change)
                    color: root.accentColor
                    font.family: Theme.font
                    font.pixelSize: 10
                    font.weight: Font.DemiBold
                }
            }

            Text {
                text: "\uf107"
                color: root.expanded || cardMouse.containsMouse ? root.accentColor : Theme.mutedStrong
                font.family: Theme.iconFont
                font.pixelSize: 10
                rotation: root.expansionProgress * 180
            }
        }

        MouseArea {
            id: cardMouse

            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.triggered()
        }
    }

    StockDetails {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: summary.bottom
        height: 134
        visible: root.expansionProgress > 0
        opacity: Math.min(1, root.expansionProgress * 3)
        quote: root.quote
    }

    Behavior on expansionProgress {
        NumberAnimation {
            duration: 160
            easing.type: Easing.OutCubic
        }
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
}
