pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "Theme.js" as Theme

ShellPopup {
    id: root

    required property var network
    property var selectedNetwork: null
    readonly property bool enteringCredentials: selectedNetwork !== null

    signal openEditorRequested

    function clearCredentials() {
        passwordField.text = "";
        ssidField.text = "";
        revealPassword.checked = false;
        selectedNetwork = null;
    }

    function editAdvanced() {
        clearCredentials();
        close();
        openEditorRequested();
    }

    function requestCredentials(accessPoint) {
        clearCredentials();
        selectedNetwork = accessPoint;
        Qt.callLater(() => {
            if (accessPoint.requiresSsid)
                ssidField.forceActiveFocus();
            else
                passwordField.forceActiveFocus();
        });
    }

    function selectNetwork(accessPoint) {
        if (accessPoint.active)
            return;

        if (accessPoint.advanced)
            editAdvanced();
        else if (accessPoint.requiresSsid || (accessPoint.protected && !accessPoint.saved))
            requestCredentials(accessPoint);
        else
            network.connectNetwork(accessPoint, "", "");
    }

    function submitCredentials() {
        if (!selectedNetwork || network.working)
            return;

        const accessPoint = selectedNetwork;
        const password = passwordField.text;
        const ssid = ssidField.text;
        clearCredentials();
        network.connectNetwork(accessPoint, password, ssid);
    }

    onVisibleChanged: {
        clearCredentials();
        if (visible)
            network.refreshOnOpen();
    }
    onClosingChanged: {
        if (closing)
            clearCredentials();
    }

    Connections {
        function onCredentialsRequested(accessPoint) {
            if (root.visible && !root.closing)
                root.requestCredentials(accessPoint);
        }

        target: root.network
    }

    ColumnLayout {
        width: parent.width
        spacing: Theme.spacing

        MenuHeader {
            id: header

            Layout.fillWidth: true
            title: "Network"
            subtitle: !root.network.available ? (root.network.loading ? "Checking connection…" : "Status unavailable") : root.network.connected ? (root.network.connectivity === "portal" ? "Sign in to this network" : root.network.connectivity === "limited" ? "Connected · limited internet" : "Connected") : "No active connection"
            icon: root.network.icon
            accentColor: Theme.blue
            accentBackground: Theme.blueSoft
            trailingItem: MenuIconButton {
                glyph: "\uf021"
                tooltip: root.network.wifiAvailable && root.network.wifiEnabled ? "Rescan Wi-Fi networks" : "Refresh network status"
                enabled: !root.network.working && !root.network.loading
                onTriggered: root.network.rescan()
            }
        }

        ScrollView {
            id: scroll

            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(body.implicitHeight, Math.max(0, root.availableContentHeight - header.implicitHeight - advanced.implicitHeight - Theme.spacing * 2))
            contentWidth: availableWidth
            contentHeight: body.implicitHeight
            clip: true
            ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
            ScrollBar.vertical: ShellScrollBar {}

            ColumnLayout {
                id: body

                width: scroll.availableWidth
                spacing: Theme.spacing

                MenuButton {
                    Layout.fillWidth: true
                    glyph: root.network.icon
                    label: root.network.connected ? root.network.connectionName : root.network.available ? "Not connected" : "Connection unknown"
                    detail: root.network.connected ? (root.network.connectionType === "ethernet" ? "Wired" : root.network.connectionType === "wifi" ? "Wi-Fi" : root.network.connectionType) + " · " + root.network.deviceName : root.network.available ? "Choose a network below" : "Last known state is retained"
                    trailing: root.network.connected ? (root.network.available ? "Connected" : "Last known") : ""
                    clickable: false
                    showChevron: false
                }

                Text {
                    Layout.fillWidth: true
                    visible: root.network.connections.length > 1
                    text: root.network.connections.slice(1).map(connection => {
                        return connection.name + " · " + connection.device;
                    }).join("\n")
                    font.family: Theme.font
                    font.pixelSize: Theme.textSmall
                    color: Theme.mutedStrong
                    wrapMode: Text.Wrap
                }

                MenuButton {
                    Layout.fillWidth: true
                    visible: root.network.wifiAvailable
                    glyph: "\uf1eb"
                    label: "Wi-Fi"
                    detail: !root.network.wifiHardwareEnabled ? "Blocked by hardware or airplane mode" : root.network.adapters.filter(adapter => {
                        return adapter.managed;
                    }).length === 0 ? "Adapters are unmanaged · use Advanced" : root.network.adapters.map(adapter => {
                        return adapter.name;
                    }).join(" · ")
                    trailing: root.network.wifiEnabled ? "On" : "Off"
                    selected: root.network.wifiEnabled
                    enabled: root.network.available && root.network.wifiAvailable && !root.network.working
                    showChevron: false
                    Accessible.role: Accessible.CheckBox
                    Accessible.checked: root.network.wifiEnabled
                    onTriggered: {
                        root.clearCredentials();
                        root.network.setWifiEnabled(!root.network.wifiEnabled);
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    visible: root.network.working || (root.network.loading && !root.network.available)
                    spacing: Theme.spacing

                    Text {
                        text: "\uf110"
                        font.family: Theme.iconFont
                        font.pixelSize: Theme.textBody
                        color: Theme.blue

                        NumberAnimation on rotation {
                            from: 0
                            to: 360
                            duration: 1200
                            loops: Animation.Infinite
                            running: root.visible && (root.network.working || root.network.loading)
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        text: root.network.actionLabel || "Refreshing network status…"
                        font.family: Theme.font
                        font.pixelSize: Theme.textSmall
                        color: Theme.mutedStrong
                        wrapMode: Text.Wrap
                    }
                }

                Text {
                    Layout.fillWidth: true
                    visible: root.network.error.length > 0
                    text: root.network.error
                    font.family: Theme.font
                    font.pixelSize: Theme.textSmall
                    color: Theme.red
                    wrapMode: Text.Wrap
                    Accessible.role: Accessible.AlertMessage
                }

                Text {
                    Layout.fillWidth: true
                    visible: !root.enteringCredentials && root.network.wifiAvailable && root.network.wifiEnabled
                    text: root.network.networks.length === 0 ? (root.network.scanning ? "Looking for networks..." : "No networks found. Use Rescan to search.") : "Available and saved networks"
                    font.family: Theme.font
                    font.pixelSize: Theme.textSmall
                    color: Theme.mutedStrong
                    wrapMode: Text.Wrap
                }

                Repeater {
                    model: !root.enteringCredentials && root.network.wifiEnabled ? root.network.networks : []

                    delegate: MenuButton {
                        required property var modelData

                        Layout.fillWidth: true
                        label: modelData.name
                        detail: modelData.security + " · " + modelData.device + (modelData.advanced ? " · opens Advanced" : modelData.saved ? " · Saved" : "")
                        trailing: modelData.active ? "Connected" : modelData.available ? modelData.signal + "%" : "Saved"
                        trailingDetail: modelData.available ? (modelData.active ? modelData.signal + "%" : "") : modelData.hidden ? "Hidden" : "Not in range"
                        glyph: modelData.protected || modelData.advanced ? "\uf023" : "\uf1eb"
                        selected: modelData.active
                        enabled: root.network.available && !root.network.working
                        clickable: !modelData.active
                        showChevron: !modelData.active
                        onTriggered: root.selectNetwork(modelData)
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    visible: root.enteringCredentials
                    spacing: Theme.spacing

                    Text {
                        Layout.fillWidth: true
                        text: root.selectedNetwork ? "Connect to " + root.selectedNetwork.name : ""
                        font.family: Theme.font
                        font.pixelSize: Theme.textBody
                        color: Theme.foreground
                        wrapMode: Text.Wrap
                    }

                    Label {
                        visible: ssidField.visible
                        text: "Hidden network name (SSID)"
                        font.family: Theme.font
                        font.pixelSize: Theme.textSmall
                        color: Theme.mutedStrong
                    }

                    TextField {
                        id: ssidField

                        Layout.fillWidth: true
                        visible: root.selectedNetwork !== null && root.selectedNetwork.requiresSsid
                        font.family: Theme.font
                        font.pixelSize: Theme.textBody
                        color: Theme.foreground
                        selectionColor: Theme.selected
                        selectedTextColor: Theme.foreground
                        placeholderText: "Exact network name"
                        placeholderTextColor: Theme.mutedStrong
                        padding: 10
                        Accessible.name: "Hidden network name, SSID"

                        background: Rectangle {
                            color: Theme.raised
                            radius: Theme.controlRadius
                            border.color: ssidField.activeFocus ? Theme.blue : Theme.border
                        }
                    }

                    Label {
                        visible: passwordField.visible
                        text: "Wi-Fi password"
                        font.family: Theme.font
                        font.pixelSize: Theme.textSmall
                        color: Theme.mutedStrong
                    }

                    TextField {
                        id: passwordField

                        Layout.fillWidth: true
                        visible: root.selectedNetwork !== null && root.selectedNetwork.protected
                        font.family: Theme.font
                        font.pixelSize: Theme.textBody
                        color: Theme.foreground
                        selectionColor: Theme.selected
                        selectedTextColor: Theme.foreground
                        echoMode: revealPassword.checked ? TextInput.Normal : TextInput.Password
                        inputMethodHints: Qt.ImhSensitiveData | Qt.ImhNoPredictiveText | Qt.ImhNoAutoUppercase
                        placeholderText: "Password"
                        placeholderTextColor: Theme.mutedStrong
                        padding: 10
                        Accessible.name: "Wi-Fi password"
                        onAccepted: {
                            if (connectButton.enabled)
                                root.submitCredentials();
                        }

                        background: Rectangle {
                            color: Theme.raised
                            radius: Theme.controlRadius
                            border.color: passwordField.activeFocus ? Theme.blue : Theme.border
                        }
                    }

                    CheckBox {
                        id: revealPassword

                        visible: passwordField.visible
                        text: "Show password"
                        font.family: Theme.font
                        font.pixelSize: Theme.textSmall
                        spacing: Theme.spacing
                        padding: 6
                        Accessible.name: text

                        indicator: Rectangle {
                            x: revealPassword.leftPadding
                            y: (revealPassword.height - height) / 2
                            implicitWidth: 18
                            implicitHeight: 18
                            radius: 4
                            color: revealPassword.checked ? Theme.blue : Theme.raised
                            border.color: revealPassword.activeFocus ? Theme.blue : Theme.border

                            Text {
                                anchors.centerIn: parent
                                text: revealPassword.checked ? "\uf00c" : ""
                                font.family: Theme.iconFont
                                font.pixelSize: Theme.textSmall
                                color: Theme.backgroundSolid
                            }
                        }

                        contentItem: Text {
                            leftPadding: revealPassword.indicator.width + revealPassword.spacing
                            text: revealPassword.text
                            font: revealPassword.font
                            color: Theme.foreground
                            verticalAlignment: Text.AlignVCenter
                        }
                    }

                    MenuButton {
                        id: connectButton

                        Layout.fillWidth: true
                        glyph: "\uf1eb"
                        label: "Connect"
                        selected: true
                        enabled: root.network.available && root.network.wifiEnabled && !root.network.working && (!ssidField.visible || ssidField.text.length > 0) && (!passwordField.visible || passwordField.text.length > 0)
                        onTriggered: root.submitCredentials()
                    }

                    MenuButton {
                        Layout.fillWidth: true
                        label: "Cancel"
                        showChevron: false
                        onTriggered: root.clearCredentials()
                    }
                }
            }
        }

        MenuButton {
            id: advanced

            Layout.fillWidth: true
            glyph: "\uf1de"
            label: "Advanced settings"
            onTriggered: root.editAdvanced()
        }
    }
}
