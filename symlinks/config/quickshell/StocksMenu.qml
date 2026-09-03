import QtQuick
import QtQuick.Layouts
import Quickshell
import "Theme.js" as Theme

PopupWindow {
    id: root

    required property Item anchorItem
    property var quotes: []
    property int updated: 0

    function price(value) {
        return Number(value).toLocaleString(Qt.locale("en_US"), "f", 2);
    }

    implicitWidth: 370
    implicitHeight: 76 + Math.max(1, quotes.length) * 52
    color: "transparent"
    grabFocus: true

    anchor {
        window: root.anchorItem.QsWindow.window
        adjustment: PopupAdjustment.SlideX | PopupAdjustment.FlipY
        gravity: Edges.Bottom | Edges.Right
        onAnchoring: {
            const content = root.anchorItem.QsWindow.contentItem;
            const point = content.mapFromItem(root.anchorItem, root.anchorItem.width - root.width, root.anchorItem.height + 6);
            anchor.rect.x = point.x;
            anchor.rect.y = point.y;
        }
    }

    Rectangle {
        anchors.fill: parent
        radius: 10
        color: Theme.backgroundSolid
        border.width: 1
        border.color: Theme.border

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 12
            spacing: 3

            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 6
                Layout.rightMargin: 4
                Layout.bottomMargin: 6

                ColumnLayout {
                    spacing: 0

                    Text {
                        text: "Markets"
                        color: Theme.foreground
                        font.family: Theme.font
                        font.pixelSize: 16
                        font.weight: Font.DemiBold
                    }

                    Text {
                        text: root.updated > 0 ? "Updated " + Qt.formatTime(new Date(root.updated * 1000), "HH:mm") : "Loading quotes"
                        color: Theme.muted
                        font.family: Theme.font
                        font.pixelSize: 10
                    }
                }
            }

            Text {
                visible: root.quotes.length === 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                text: "No quote data available"
                color: Theme.muted
                font.family: Theme.font
                font.pixelSize: 12
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
            }

            Repeater {
                model: root.quotes

                Rectangle {
                    required property var modelData

                    Layout.fillWidth: true
                    implicitHeight: 49
                    radius: 7
                    color: quoteMouse.containsMouse ? Theme.hover : Theme.raised

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 11
                        anchors.rightMargin: 11

                        ColumnLayout {
                            spacing: 0
                            Layout.fillWidth: true

                            Text {
                                text: modelData.symbol
                                color: Theme.foreground
                                font.family: Theme.font
                                font.pixelSize: 13
                                font.weight: Font.DemiBold
                            }

                            Text {
                                text: modelData.name
                                color: Theme.muted
                                font.family: Theme.font
                                font.pixelSize: 10
                            }
                        }

                        ColumnLayout {
                            spacing: 0
                            Layout.alignment: Qt.AlignRight

                            Text {
                                Layout.alignment: Qt.AlignRight
                                text: modelData.prefix + root.price(modelData.price)
                                color: Theme.foreground
                                font.family: Theme.font
                                font.pixelSize: 13
                                font.weight: Font.DemiBold
                            }

                            Text {
                                Layout.alignment: Qt.AlignRight
                                text: (modelData.change >= 0 ? "+" : "") + Number(modelData.change).toFixed(2) + "%"
                                color: modelData.change >= 0 ? Theme.green : Theme.red
                                font.family: Theme.font
                                font.pixelSize: 10
                            }
                        }
                    }

                    MouseArea {
                        id: quoteMouse

                        anchors.fill: parent
                        hoverEnabled: true
                        acceptedButtons: Qt.NoButton
                    }
                }
            }
        }
    }
}
