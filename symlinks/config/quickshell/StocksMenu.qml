import QtQuick
import QtQuick.Layouts
import Quickshell
import "Theme.js" as Theme

PopupWindow {
    id: root

    required property Item anchorItem
    property var quotes: []
    property int updated: 0
    property string expandedSymbol: ""
    property bool windowExpanded: false
    readonly property int collapsedHeight: 28 + headerContainer.height + (quotes.length === 0 ? 68 : quotes.length * 68)

    implicitWidth: 420
    implicitHeight: collapsedHeight + (windowExpanded ? 134 : 0)
    color: "transparent"
    grabFocus: true
    onExpandedSymbolChanged: {
        if (expandedSymbol.length > 0) {
            collapseTimer.stop();
            windowExpanded = true;
        } else if (visible) {
            collapseTimer.restart();
        }
    }
    onVisibleChanged: {
        if (!visible) {
            collapseTimer.stop();
            expandedSymbol = "";
            windowExpanded = false;
        }
    }

    Timer {
        id: collapseTimer

        interval: 180
        onTriggered: root.windowExpanded = false
    }

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
        clip: true

        Column {
            id: contentColumn

            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.leftMargin: 14
            anchors.rightMargin: 14
            anchors.topMargin: 14
            spacing: 6

            Item {
                id: headerContainer

                width: parent.width
                height: marketHeader.implicitHeight + 4

                MenuHeader {
                    id: marketHeader

                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.leftMargin: 2
                    anchors.rightMargin: 2
                    icon: "\uf201"
                    title: "Markets"
                    subtitle: root.updated > 0 ? "Updated " + Qt.formatTime(new Date(root.updated * 1000), "HH:mm") : "Loading quotes"
                    accentColor: Theme.green
                    accentBackground: Theme.greenSoft
                }
            }

            Text {
                visible: root.quotes.length === 0
                width: parent.width
                height: visible ? 62 : 0
                text: "No quote data available"
                color: Theme.mutedStrong
                font.family: Theme.font
                font.pixelSize: 12
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
            }

            Repeater {
                model: root.quotes

                StockCard {
                    required property var modelData

                    width: contentColumn.width
                    quote: modelData
                    expanded: root.expandedSymbol === modelData.symbol
                    onTriggered: root.expandedSymbol = expanded ? "" : modelData.symbol
                }
            }
        }
    }
}
