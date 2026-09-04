pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import "Theme.js" as Theme

ShellPopup {
    id: root

    property var quotes: []
    property int updated: 0
    property bool loading: false
    property string error: ""
    property string expandedSymbol: ""
    signal refreshRequested
    readonly property bool hasQuotes: Boolean(quotes && quotes.length > 0)
    readonly property bool stale: hasQuotes && (updated <= 0 || clock.date.getTime() / 1000 - updated > 15 * 60)
    readonly property string statusText: {
        if (loading)
            return hasQuotes ? "Refreshing quotes..." : "Loading quotes...";
        if (error.length > 0)
            return (hasQuotes ? "Refresh failed. Showing previous quotes.\n" : "Could not load quotes.\n") + error;
        if (!hasQuotes)
            return "No quote data available";
        if (stale)
            return updated > 0 ? "Quotes may be out of date." : "Quote update time is unavailable.";
        return "";
    }

    panelWidth: 420
    onVisibleChanged: {
        if (!visible)
            expandedSymbol = "";
    }

    SystemClock {
        id: clock

        precision: SystemClock.Minutes
    }

    ColumnLayout {
        width: parent.width
        spacing: Theme.spacing

        MenuHeader {
            id: header

            Layout.fillWidth: true
            icon: "\uf201"
            title: "Markets"
            subtitle: root.updated > 0 ? "Updated " + Qt.formatDateTime(new Date(root.updated * 1000), root.stale ? "MMM d, HH:mm" : "HH:mm") : "Market overview"
            accentColor: Theme.mutedStrong
            accentBackground: "transparent"
            trailingItem: MenuIconButton {
                glyph: "\uf2f1"
                tooltip: "Refresh quotes"
                enabled: !root.loading
                onTriggered: root.refreshRequested()
            }
        }

        Flickable {
            id: scroll

            function reveal(item) {
                if (item.y < contentY)
                    contentY = item.y;
                else if (item.y + Math.min(item.height, height) > contentY + height)
                    contentY = Math.max(0, item.y + Math.min(item.height, height) - height);
            }

            Layout.fillWidth: true
            implicitHeight: Math.min(contentHeight, Math.max(0, root.availableContentHeight - header.implicitHeight - Theme.spacing))
            contentHeight: quotesColumn.implicitHeight
            contentWidth: width
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            flickableDirection: Flickable.VerticalFlick
            activeFocusOnTab: contentHeight > height
            Keys.onDownPressed: contentY = Math.min(Math.max(0, contentHeight - height), contentY + 58)
            Keys.onUpPressed: contentY = Math.max(0, contentY - 58)
            Keys.onPressed: event => {
                if (event.key === Qt.Key_PageDown) {
                    contentY = Math.min(Math.max(0, contentHeight - height), contentY + height);
                    event.accepted = true;
                } else if (event.key === Qt.Key_PageUp) {
                    contentY = Math.max(0, contentY - height);
                    event.accepted = true;
                }
            }
            ScrollBar.vertical: ShellScrollBar {}

            Column {
                id: quotesColumn

                width: scroll.width
                spacing: Theme.spacing

                Text {
                    width: parent.width
                    topPadding: Theme.spacing
                    bottomPadding: Theme.spacing
                    visible: root.statusText.length > 0
                    text: root.statusText
                    color: !root.loading && (root.error.length > 0 || root.stale) ? Theme.yellow : Theme.mutedStrong
                    font.family: Theme.font
                    font.pixelSize: Theme.textSmall
                    wrapMode: Text.Wrap
                    maximumLineCount: 4
                    elide: Text.ElideRight
                    textFormat: Text.PlainText
                }

                Repeater {
                    model: root.quotes || []

                    StockCard {
                        id: card

                        required property var modelData

                        width: quotesColumn.width
                        quote: modelData
                        expanded: root.expandedSymbol === modelData.symbol
                        onTriggered: root.expandedSymbol = expanded ? "" : modelData.symbol
                        onActiveFocusChanged: {
                            if (activeFocus)
                                scroll.reveal(card);
                        }
                    }
                }
            }
        }
    }
}
