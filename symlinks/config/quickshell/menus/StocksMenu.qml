pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../components"
import "../lib/Theme.js" as Theme

ShellPopup {
    id: root

    property var quotes: []
    property var missingSymbols: []
    property int updated: 0
    required property date currentDate
    property bool loading: false
    property string error: ""
    property string expandedSymbol: ""
    signal refreshRequested
    readonly property int staleAfterSeconds: 15 * 60
    readonly property bool hasQuotes: Boolean(quotes && quotes.length > 0)
    readonly property bool stale: hasQuotes && (updated <= 0 || currentDate.getTime() / 1000 - updated >= staleAfterSeconds)
    readonly property string statusText: {
        if (loading)
            return hasQuotes ? "Refreshing quotes..." : "Loading quotes...";
        if (!hasQuotes)
            return error || "No quote data available";
        const status = [];
        if (error.length > 0)
            status.push(error);
        else if (stale)
            status.push(updated > 0 ? "Quotes may be out of date." : "Quote update time is unavailable.");
        if (missingSymbols.length > 0)
            status.push("Quotes unavailable for " + missingSymbols.join(", ") + ".");
        return status.join("\n");
    }

    panelWidth: 420
    onVisibleChanged: {
        if (!visible)
            expandedSymbol = "";
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
            accentColor: Theme.blue
            accentBackground: Theme.blueSoft
            trailingItem: MenuIconButton {
                glyph: "\uf021"
                tooltip: "Refresh quotes"
                enabled: !root.loading
                spinning: root.loading
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

                MenuBanner {
                    width: parent.width
                    visible: root.statusText.length > 0
                    tone: !root.loading && (root.error.length > 0 || root.stale || root.missingSymbols.length > 0) ? "warning" : "info"
                    spinning: root.loading
                    maximumLineCount: 4
                    text: root.statusText
                }

                Repeater {
                    model: root.quotes || []

                    StockCard {
                        id: card

                        required property var modelData

                        width: quotesColumn.width
                        quote: modelData
                        currentDate: root.currentDate
                        staleAfterSeconds: root.staleAfterSeconds
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
