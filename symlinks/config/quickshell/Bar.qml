import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Services.Mpris
import Quickshell.Services.SystemTray
import Quickshell.Services.UPower
import Quickshell.Wayland
import Quickshell.Widgets
import "Theme.js" as Theme

PanelWindow {
    id: bar

    required property var modelData
    readonly property var hyprMonitor: Hyprland.monitorFor(screen)
    readonly property var activeWorkspace: hyprMonitor ? hyprMonitor.activeWorkspace : null
    readonly property var player: {
        const players = Mpris.players.values;
        for (let i = 0; i < players.length; ++i) {
            if (players[i].dbusName.toLowerCase().includes("spotify"))
                return players[i];

        }
        return players.length > 0 ? players[0] : null;
    }
    property var stockQuotes: []
    property int stocksUpdated: 0
    property string networkIcon: "\uf127"
    property bool networkConnected: false
    property string networkName: ""
    property string networkType: ""
    property string networkDevice: ""

    function closeMenus(except) {
        if (except !== "volume")
            volumeMenu.visible = false;

        if (except !== "battery")
            batteryMenu.visible = false;

        if (except !== "power")
            powerMenu.visible = false;

        if (except !== "calendar")
            calendarMenu.visible = false;

        if (except !== "stocks")
            stocksMenu.visible = false;

        if (except !== "network")
            networkMenu.visible = false;

    }

    function toggleMenu(name, menu) {
        const show = !menu.visible;
        closeMenus(show ? name : "");
        menu.visible = show;
    }

    screen: modelData
    implicitHeight: 30
    exclusiveZone: visible ? 30 : 0
    color: "transparent"
    visible: !(activeWorkspace && activeWorkspace.hasFullscreen)
    WlrLayershell.layer: WlrLayer.Bottom

    anchors {
        top: true
        left: true
        right: true
    }

    Item {
        anchors.fill: parent

        RowLayout {
            anchors.left: parent.left
            anchors.leftMargin: 8
            anchors.verticalCenter: parent.verticalCenter
            spacing: 4

            Rectangle {
                implicitWidth: workspaceRow.implicitWidth + 12
                implicitHeight: 22
                radius: 6
                color: Theme.background

                Row {
                    id: workspaceRow

                    anchors.centerIn: parent
                    spacing: 0

                    Repeater {
                        model: 9

                        Item {
                            required property int index
                            readonly property int number: index + 1
                            readonly property var workspace: {
                                const workspaces = Hyprland.workspaces.values;
                                for (let i = 0; i < workspaces.length; ++i) {
                                    if (workspaces[i].id === number)
                                        return workspaces[i];

                                }
                                return null;
                            }
                            readonly property bool active: bar.activeWorkspace && bar.activeWorkspace.id === number

                            visible: active || workspace !== null
                            width: visible ? 22 : 0
                            height: 22

                            Rectangle {
                                anchors.fill: parent
                                radius: 6
                                color: workspaceMouse.containsMouse ? Theme.hover : "transparent"

                                Text {
                                    anchors.centerIn: parent
                                    text: parent.parent.active ? "\u25cf" : "\u25cb"
                                    color: parent.parent.active ? Theme.blue : Theme.foreground
                                    font.family: Theme.font
                                    font.pixelSize: 16
                                }
                            }

                            MouseArea {
                                id: workspaceMouse

                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: Hyprland.dispatch("hl.dsp.focus({ workspace = " + parent.number + " })")
                            }
                        }
                    }
                }
            }

            BarBlock {
                readonly property var toplevel: Hyprland.activeToplevel

                visible: Boolean(toplevel && toplevel.title && toplevel.title.length > 0)
                text: toplevel && toplevel.title ? toplevel.title.slice(0, 80) : ""
                interactive: false
            }
        }

        RowLayout {
            anchors.right: parent.right
            anchors.rightMargin: 8
            anchors.verticalCenter: parent.verticalCenter
            spacing: 4

            BarBlock {
                visible: bar.player !== null && bar.player.playbackState !== MprisPlaybackState.Stopped
                text: visible ? "\uf001  " + ((bar.player.trackArtist ? bar.player.trackArtist + " - " : "") + (bar.player.trackTitle || "Unknown track")).slice(0, 72) : ""
                textColor: bar.player && bar.player.isPlaying ? Theme.purple : Theme.muted
                onActivated: (button) => {
                    if (!bar.player)
                        return ;

                    if (button === Qt.MiddleButton && bar.player.canGoPrevious)
                        bar.player.previous();
                    else if (button === Qt.RightButton && bar.player.canGoNext)
                        bar.player.next();
                    else if (bar.player.canTogglePlaying)
                        bar.player.togglePlaying();
                }
            }

            Rectangle {
                visible: SystemTray.items.values.length > 0
                implicitWidth: trayRow.implicitWidth + 12
                implicitHeight: 22
                radius: 6
                color: Theme.background

                Row {
                    id: trayRow

                    anchors.centerIn: parent
                    spacing: 6

                    Repeater {
                        model: SystemTray.items

                        Item {
                            id: trayItem

                            required property var modelData

                            width: 18
                            height: 18

                            Rectangle {
                                anchors.fill: parent
                                radius: 4
                                color: trayMouse.containsMouse ? Theme.hover : "transparent"
                            }

                            IconImage {
                                anchors.centerIn: parent
                                width: 14
                                height: 14
                                source: trayItem.modelData.icon
                            }

                            MouseArea {
                                id: trayMouse

                                anchors.fill: parent
                                acceptedButtons: Qt.AllButtons
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: (event) => {
                                    if (event.button === Qt.MiddleButton) {
                                        trayItem.modelData.secondaryActivate();
                                    } else if (event.button === Qt.RightButton || trayItem.modelData.onlyMenu) {
                                        const point = bar.contentItem.mapFromItem(trayItem, 0, trayItem.height);
                                        trayItem.modelData.display(bar, point.x, point.y);
                                    } else {
                                        trayItem.modelData.activate();
                                    }
                                }
                                onWheel: (event) => {
                                    return trayItem.modelData.scroll(event.angleDelta.y, false);
                                }
                            }
                        }
                    }
                }
            }

            BarBlock {
                id: stocksButton

                text: "\uf201"
                fontFamily: Theme.iconFont
                textColor: bar.stockQuotes.length > 0 ? Theme.foreground : Theme.muted
                onActivated: bar.toggleMenu("stocks", stocksMenu)
            }

            BarBlock {
                id: networkButton

                text: bar.networkIcon
                fontFamily: Theme.iconFont
                textColor: bar.networkIcon === "\uf127" ? Theme.muted : Theme.foreground
                onActivated: (button) => {
                    if (button === Qt.RightButton)
                        networkEditor.startDetached();
                    else
                        bar.toggleMenu("network", networkMenu);
                }
            }

            BarBlock {
                id: volumeButton

                text: audio.muted ? "\uf6a9" : "\uf028"
                fontFamily: Theme.iconFont
                textColor: !audio.available || audio.muted ? Theme.muted : Theme.foreground
                onActivated: (button) => {
                    if (button === Qt.MiddleButton)
                        audio.toggleMuted();
                    else
                        bar.toggleMenu("volume", volumeMenu);
                }
                onScrolled: (steps) => {
                    return audio.setVolume(audio.volume + steps * 0.05);
                }
            }

            BarBlock {
                id: batteryButton

                readonly property var battery: UPower.displayDevice
                readonly property real charge: battery && battery.ready ? battery.percentage : 0
                readonly property bool charging: battery && (battery.state === UPowerDeviceState.Charging || battery.state === UPowerDeviceState.FullyCharged)

                visible: battery && battery.ready && battery.isLaptopBattery
                text: {
                    if (charging)
                        return "\uf0e7";

                    if (charge <= 0.15)
                        return "\uf244";

                    if (charge <= 0.35)
                        return "\uf243";

                    if (charge <= 0.6)
                        return "\uf242";

                    if (charge <= 0.85)
                        return "\uf241";

                    return "\uf240";
                }
                fontFamily: Theme.iconFont
                textColor: charging ? Theme.green : (charge <= 0.15 ? Theme.red : (charge <= 0.3 ? Theme.yellow : Theme.foreground))
                onActivated: bar.toggleMenu("battery", batteryMenu)
            }

            BarBlock {
                id: powerButton

                text: "\uf011"
                fontFamily: Theme.iconFont
                onActivated: bar.toggleMenu("power", powerMenu)
            }

            BarBlock {
                id: clockButton

                text: Qt.formatDateTime(clock.date, "MMM dd HH:mm")
                horizontalPadding: 12
                onActivated: bar.toggleMenu("calendar", calendarMenu)

                SystemClock {
                    id: clock

                    precision: SystemClock.Minutes
                }
            }
        }
    }

    VolumeMenu {
        id: volumeMenu

        anchorItem: volumeButton
        audio: audio
        visible: false
    }

    BatteryMenu {
        id: batteryMenu

        anchorItem: batteryButton
        battery: batteryButton.battery
        visible: false
    }

    StocksMenu {
        id: stocksMenu

        anchorItem: stocksButton
        visible: false
        quotes: bar.stockQuotes
        updated: bar.stocksUpdated
    }

    NetworkMenu {
        id: networkMenu

        anchorItem: networkButton
        visible: false
        connected: bar.networkConnected
        connectionName: bar.networkName
        connectionType: bar.networkType
        deviceName: bar.networkDevice
        statusIcon: bar.networkIcon
        onOpenEditorRequested: networkEditor.startDetached()
        onRefreshRequested: network.running = true
    }

    PowerMenu {
        id: powerMenu

        anchorItem: powerButton
        visible: false
    }

    CalendarMenu {
        id: calendarMenu

        anchorItem: clockButton
        visible: false
    }

    AudioState {
        id: audio
    }

    Process {
        id: stocks

        command: [Quickshell.env("HOME") + "/.config/hypr/scripts/stocks.sh"]
        running: true
        onExited: {
            try {
                const payload = JSON.parse(stocksOutput.text);
                bar.stockQuotes = payload.quotes || [];
                bar.stocksUpdated = payload.updated || 0;
            } catch (error) {
                bar.stockQuotes = [];
                bar.stocksUpdated = 0;
            }
        }

        stdout: StdioCollector {
            id: stocksOutput
        }
    }

    Timer {
        interval: 300000
        repeat: true
        running: true
        onTriggered: stocks.running = true
    }

    Process {
        id: network

        command: ["nmcli", "-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device"]
        running: true
        onExited: {
            const lines = networkOutput.text.trim().split("\n");
            let connected = null;
            let priority = 0;
            for (let i = 0; i < lines.length; ++i) {
                const fields = lines[i].split(":");
                if (fields.length < 4 || fields[2] !== "connected")
                    continue;

                const candidatePriority = fields[1] === "wifi" ? 3 : (fields[1] === "ethernet" ? 2 : 1);
                if (candidatePriority > priority) {
                    connected = fields;
                    priority = candidatePriority;
                }
            }
            if (!connected) {
                bar.networkIcon = "\uf127";
                bar.networkConnected = false;
                bar.networkName = "";
                bar.networkType = "";
                bar.networkDevice = "";
            } else {
                bar.networkConnected = true;
                bar.networkDevice = connected[0];
                bar.networkType = connected[1];
                bar.networkName = connected.slice(3).join(":");
                bar.networkIcon = connected[1] === "wifi" ? "\uf1eb" : "\uf796";
            }
        }

        stdout: StdioCollector {
            id: networkOutput
        }
    }

    Timer {
        interval: 10000
        repeat: true
        running: true
        onTriggered: network.running = true
    }

    Process {
        id: networkEditor

        command: ["nm-connection-editor"]
    }
}
