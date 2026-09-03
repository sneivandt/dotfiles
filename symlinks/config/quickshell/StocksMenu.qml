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

    implicitWidth: 390
    implicitHeight: 70 + Math.max(1, quotes.length) * 59
    color: "transparent"
    grabFocus: true

    anchor {
        window: root.anchorItem.QsWindow.window
        adjustment: PopupAdjustment.SlideX | PopupAdjustment.FlipY
        gravity: Edges.Bottom | Edges.Right
        onAnchoring: {
            const content = root.anchorItem.QsWindow.contentItem;
            const point = content.mapFromItem(root.anchorItem, root.anchorItem.width - root.width, root.anchorItem.height + 8);
            anchor.rect.x = point.x;
            anchor.rect.y = point.y;
        }
    }

    MenuPanel {
        anchors.fill: parent

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 14
            spacing: 5

            MenuHeader {
                Layout.fillWidth: true
                Layout.leftMargin: 2
                Layout.rightMargin: 2
                Layout.bottomMargin: 5
                icon: "\uf201"
                title: "Markets"
                subtitle: root.updated > 0 ? "Updated " + Qt.formatTime(new Date(root.updated * 1000), "HH:mm") : "Loading quotes"
                accentColor: Theme.green
                accentBackground: Theme.greenSoft
            }

            Text {
                visible: root.quotes.length === 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                text: "No quote data available"
                color: Theme.mutedStrong
                font.family: Theme.font
                font.pixelSize: 12
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
            }

            Repeater {
                model: root.quotes

                MenuButton {
                    required property var modelData

                    Layout.fillWidth: true
                    icon: modelData.change >= 0 ? "\uf062" : "\uf063"
                    label: modelData.symbol
                    detail: modelData.name
                    trailing: modelData.prefix + root.price(modelData.price)
                    trailingDetail: (modelData.change >= 0 ? "+" : "") + Number(modelData.change).toFixed(2) + "%"
                    trailingDetailColor: modelData.change >= 0 ? Theme.green : Theme.red
                    danger: modelData.change < 0
                    clickable: false
                    showChevron: false
                }
            }
        }
    }
}
