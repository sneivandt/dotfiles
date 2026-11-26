pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../components"
import "../lib/Theme.js" as Theme

ShellPopup {
    id: root

    required property var network
    property var selectedNetwork: null
    property bool passwordVisible: false
    readonly property bool enteringCredentials: selectedNetwork !== null
    readonly property var extraConnections: root.network.connections.filter(connection => {
        return connection.type !== "wifi" || root.enteringCredentials || !root.network.wifiEnabled || !root.network.networks.some(accessPoint => {
            return accessPoint.active && accessPoint.uuid === connection.uuid && accessPoint.device === connection.device;
        });
    })

    signal openEditorRequested

    dismissOnEscape: !enteringCredentials

    function clearCredentials() {
        passwordField.text = "";
        ssidField.text = "";
        passwordVisible = false;
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

    onEscapePressed: clearCredentials()
    onVisibleChanged: clearCredentials()
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
            subtitle: !root.network.detailsAvailable ? (root.network.loading ? "Checking connection…" : "Status unavailable") : root.network.connected ? (root.network.connectivity === "portal" ? "Sign in to this network" : root.network.connectivity === "limited" ? "Limited internet" : "Connected") : "No active connection"
            icon: root.network.icon
            accentColor: Theme.blue
            accentBackground: Theme.blueSoft
            trailingItem: MenuIconButton {
                glyph: "\uf021"
                tooltip: root.network.wifiAvailable && root.network.wifiEnabled ? "Rescan Wi-Fi networks" : "Refresh network status"
                enabled: !root.network.working && !root.network.loading
                spinning: root.network.scanning || root.network.loading
                onTriggered: root.network.rescan()
            }
        }

        ScrollView {
            id: scroll

            Layout.fillWidth: true
            Layout.minimumHeight: 0
            Layout.preferredHeight: Math.min(body.implicitHeight, Layout.maximumHeight)
            Layout.maximumHeight: Math.min(440, Math.max(0, root.availableContentHeight - header.implicitHeight - advanced.implicitHeight - Theme.spacing * 2))
            contentWidth: availableWidth
            contentHeight: body.implicitHeight
            clip: true
            ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
            ScrollBar.vertical: ShellScrollBar {}

            ColumnLayout {
                id: body

                width: scroll.availableWidth
                spacing: Theme.spacing

                MenuSectionLabel {
                    Layout.fillWidth: true
                    visible: root.extraConnections.length > 0
                    text: "Active"
                }

                Repeater {
                    model: root.extraConnections

                    delegate: MenuButton {
                        required property var modelData

                        Layout.fillWidth: true
                        glyph: modelData.type === "wifi" ? "\uf1eb" : "\uf6ff"
                        label: modelData.name
                        trailing: root.network.detailsAvailable ? "Connected" : "Last known"
                        clickable: false
                        showChevron: false
                    }
                }

                MenuButton {
                    Layout.fillWidth: true
                    visible: root.network.wifiAvailable
                    glyph: "\uf1eb"
                    label: "Wi-Fi"
                    detail: !root.network.wifiHardwareEnabled ? "Blocked by hardware or airplane mode" : root.network.adapters.filter(adapter => {
                        return adapter.managed;
                    }).length === 0 ? "Adapters are unmanaged · use Advanced" : ""
                    selected: root.network.wifiEnabled
                    showSelectionIndicator: false
                    enabled: root.network.detailsAvailable && root.network.wifiAvailable && !root.network.working
                    showChevron: false
                    Accessible.role: Accessible.CheckBox
                    Accessible.checked: root.network.wifiEnabled
                    onTriggered: {
                        root.clearCredentials();
                        root.network.setWifiEnabled(!root.network.wifiEnabled);
                    }

                    trailingItem: MenuSwitch {
                        checked: root.network.wifiEnabled
                        enabled: root.network.detailsAvailable && root.network.wifiAvailable && !root.network.working
                    }
                }

                MenuBanner {
                    Layout.fillWidth: true
                    visible: root.network.working || (root.network.loading && !root.network.detailsAvailable)
                    tone: "info"
                    spinning: true
                    labelObjectName: "networkActionLabel"
                    text: root.network.actionLabel || "Refreshing network status…"
                }

                MenuBanner {
                    Layout.fillWidth: true
                    visible: root.network.error.length > 0
                    tone: "error"
                    labelObjectName: "networkError"
                    text: root.network.error
                }

                MenuSectionLabel {
                    Layout.fillWidth: true
                    visible: !root.enteringCredentials && root.network.wifiAvailable && root.network.wifiEnabled
                    text: "Wi-Fi networks"
                }

                Text {
                    Layout.fillWidth: true
                    visible: !root.enteringCredentials && root.network.detailsAvailable && root.network.wifiAvailable && root.network.wifiEnabled && root.network.networks.length === 0
                    text: root.network.scanning ? "Looking for networks..." : "No networks found. Use Rescan to search."
                    font.family: Theme.font
                    font.pixelSize: Theme.textSmall
                    color: Theme.mutedStrong
                    wrapMode: Text.Wrap
                }

                Repeater {
                    objectName: "networkChoices"
                    model: !root.enteringCredentials && root.network.wifiEnabled ? root.network.networks : []

                    delegate: MenuButton {
                        id: networkButton

                        required property var modelData

                        Layout.fillWidth: true
                        label: modelData.name
                        detail: {
                            const parts = [];
                            if (modelData.profileLabel)
                                parts.push(modelData.profileLabel);
                            if (modelData.advanced)
                                parts.push("Opens Advanced settings");
                            else if (modelData.saved && !modelData.active && modelData.available)
                                parts.push("Saved");
                            if (root.network.adapters.length > 1)
                                parts.push(modelData.device);
                            return parts.join(" · ");
                        }
                        trailing: modelData.active ? (root.network.detailsAvailable ? "Connected" : "Last known") : modelData.available ? modelData.signal + "%" : "Saved"
                        trailingDetail: !modelData.available && !modelData.active ? (modelData.hidden ? "Hidden" : "Not in range") : ""
                        glyph: "\uf1eb"
                        glyphColor: modelData.active ? Theme.blue : (!modelData.available ? Theme.muted : (modelData.signal >= 70 ? Theme.foreground : (modelData.signal >= 40 ? Theme.mutedStrong : Theme.yellow)))
                        badgeGlyph: modelData.protected || modelData.advanced ? "\uf023" : ""
                        badgeColor: modelData.active ? Theme.blue : Theme.mutedStrong
                        selected: modelData.active
                        enabled: root.network.detailsAvailable && !root.network.working
                        clickable: !modelData.active
                        hoverEnabled: clickable || !!modelData.profileLabel
                        focusPolicy: hoverEnabled || activeFocus ? Qt.StrongFocus : Qt.NoFocus
                        showChevron: !modelData.active
                        onTriggered: root.selectNetwork(modelData)
                        onActiveFocusChanged: {
                            if (!activeFocus)
                                return;
                            const viewport = scroll.contentItem as Flickable;
                            if (!viewport)
                                return;
                            if (networkButton.y < viewport.contentY)
                                viewport.contentY = networkButton.y;
                            else if (networkButton.y + height > viewport.contentY + viewport.height)
                                viewport.contentY = networkButton.y + height - viewport.height;
                        }

                        trailingItem: MenuIconButton {
                            objectName: "networkDisconnectButton"
                            visible: Boolean(networkButton.modelData.active && networkButton.modelData.device)
                            implicitWidth: 28
                            implicitHeight: 28
                            glyph: "\uf127"
                            tooltip: "Disconnect"
                            accentColor: Theme.red
                            accentBackground: Theme.redSoft
                            enabled: root.network.detailsAvailable && !root.network.working
                            onTriggered: root.network.disconnectNetwork(networkButton.modelData)
                        }

                        ShellToolTip {
                            text: networkButton.detail
                            visible: !!networkButton.modelData.profileLabel && (networkButton.hovered || networkButton.visualFocus)
                        }
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    visible: root.enteringCredentials
                    implicitHeight: credentialsColumn.implicitHeight + 24
                    radius: Theme.itemRadius
                    color: Theme.raised
                    border.width: 1
                    border.color: Theme.borderSubtle

                    ColumnLayout {
                        id: credentialsColumn

                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.leftMargin: 12
                        anchors.rightMargin: 12
                        spacing: Theme.spacing

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: Theme.spacing

                            MenuIconButton {
                                objectName: "networkCredentialsBack"
                                implicitWidth: 28
                                implicitHeight: 28
                                glyph: "\uf104"
                                tooltip: "Back to networks"
                                onTriggered: root.clearCredentials()
                            }

                            Text {
                                objectName: "networkCredentialsTitle"
                                Layout.fillWidth: true
                                text: root.selectedNetwork ? "Connect to " + root.selectedNetwork.name : ""
                                textFormat: Text.PlainText
                                font.family: Theme.font
                                font.pixelSize: Theme.textBody
                                font.weight: Font.DemiBold
                                color: Theme.foreground
                                wrapMode: Text.Wrap
                            }
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
                            visible: root.selectedNetwork !== null && Boolean(root.selectedNetwork.requiresSsid)
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
                                color: Theme.menuBackground
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
                            echoMode: root.passwordVisible ? TextInput.Normal : TextInput.Password
                            inputMethodHints: Qt.ImhSensitiveData | Qt.ImhNoPredictiveText | Qt.ImhNoAutoUppercase
                            placeholderText: "Password"
                            placeholderTextColor: Theme.mutedStrong
                            padding: 10
                            rightPadding: 38
                            Accessible.name: "Wi-Fi password"
                            onAccepted: {
                                if (connectButton.enabled)
                                    root.submitCredentials();
                            }

                            background: Rectangle {
                                color: Theme.menuBackground
                                radius: Theme.controlRadius
                                border.color: passwordField.activeFocus ? Theme.blue : Theme.border
                            }

                            MenuIconButton {
                                objectName: "networkRevealPassword"
                                anchors.right: parent.right
                                anchors.rightMargin: 4
                                anchors.verticalCenter: parent.verticalCenter
                                implicitWidth: 28
                                implicitHeight: 28
                                glyph: root.passwordVisible ? "\uf070" : "\uf06e"
                                tooltip: root.passwordVisible ? "Hide password" : "Show password"
                                selected: root.passwordVisible
                                onTriggered: root.passwordVisible = !root.passwordVisible
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            Layout.topMargin: 2
                            spacing: Theme.spacing

                            MenuButton {
                                Layout.fillWidth: true
                                implicitHeight: 40
                                label: "Cancel"
                                centerLabel: true
                                showChevron: false
                                onTriggered: root.clearCredentials()
                            }

                            MenuButton {
                                id: connectButton

                                Layout.fillWidth: true
                                implicitHeight: 40
                                label: "Connect"
                                selected: true
                                centerLabel: true
                                showSelectionIndicator: false
                                showChevron: false
                                enabled: root.network.detailsAvailable && root.network.wifiEnabled && !root.network.working && (!ssidField.visible || ssidField.text.length > 0) && (!passwordField.visible || passwordField.text.length > 0)
                                onTriggered: root.submitCredentials()
                            }
                        }
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
