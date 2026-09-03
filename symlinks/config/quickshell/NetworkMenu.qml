import QtQuick
import QtQuick.Layouts
import Quickshell
import "Theme.js" as Theme

PopupWindow {
    id: root

    required property Item anchorItem
    property bool connected: false
    property string connectionName: ""
    property string connectionType: ""
    property string deviceName: ""
    property string statusIcon: "\uf127"

    signal openEditorRequested()
    signal refreshRequested()

    function typeLabel(type) {
        if (type === "wifi")
            return "Wi-Fi connection";

        if (type === "ethernet")
            return "Wired connection";

        return type.length > 0 ? type.charAt(0).toUpperCase() + type.slice(1) + " connection" : "Network connection";
    }

    implicitWidth: 370
    implicitHeight: 194
    color: "transparent"
    grabFocus: true
    onVisibleChanged: {
        if (visible)
            refreshRequested();

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

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 14
            spacing: 6

            MenuHeader {
                Layout.fillWidth: true
                Layout.leftMargin: 2
                Layout.rightMargin: 2
                Layout.bottomMargin: 4
                icon: root.statusIcon
                title: "Network"
                subtitle: root.connected ? "Connected via " + root.typeLabel(root.connectionType).replace(" connection", "") : "No active connection"
                accentColor: root.connected ? Theme.green : Theme.mutedStrong
                accentBackground: root.connected ? Theme.greenSoft : Theme.hover
            }

            MenuButton {
                Layout.fillWidth: true
                icon: root.statusIcon
                label: root.connected ? root.connectionName : "Offline"
                detail: root.connected ? root.typeLabel(root.connectionType) : "Check your adapter or connection"
                trailing: root.connected ? "Connected" : "Unavailable"
                trailingDetail: root.deviceName
                trailingDetailColor: root.connected ? Theme.green : Theme.mutedStrong
                accentColor: root.connected ? Theme.green : Theme.mutedStrong
                accentBackground: root.connected ? Theme.greenSoft : Theme.hover
                clickable: false
                showChevron: false
            }

            MenuButton {
                Layout.fillWidth: true
                icon: "\uf1de"
                label: "Connection editor"
                detail: "Manage networks, VPNs, and adapters"
                onTriggered: {
                    root.visible = false;
                    root.openEditorRequested();
                }
            }
        }
    }
}
