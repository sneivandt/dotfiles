pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
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
    required property var audio
    required property var network
    required property var markets
    required property var menuController
    readonly property var hyprMonitor: Hyprland.monitorFor(screen)
    readonly property var activeWorkspace: hyprMonitor ? hyprMonitor.activeWorkspace : null
    readonly property var player: {
        const players = Mpris.players.values;
        return players.find(player => player.dbusName.toLowerCase().includes("spotify")) || players[0] || null;
    }
    readonly property string track: player ? (player.trackArtist ? player.trackArtist + " - " : "") + (player.trackTitle || "Unknown track") : ""
    readonly property var battery: UPower.displayDevice
    readonly property bool hasBattery: battery && battery.ready && battery.isLaptopBattery
    readonly property real charge: hasBattery ? battery.percentage : 0
    readonly property bool charging: hasBattery && (battery.state === UPowerDeviceState.Charging || battery.state === UPowerDeviceState.FullyCharged)
    readonly property string batteryIcon: charging ? "\uf0e7" : (charge <= 0.15 ? "\uf244" : (charge <= 0.35 ? "\uf243" : (charge <= 0.6 ? "\uf242" : (charge <= 0.85 ? "\uf241" : "\uf240"))))

    function toggleMenu(menu) {
        menuController.toggle(menu);
    }

    screen: modelData
    implicitHeight: Theme.barHeight
    exclusiveZone: visible ? Theme.barHeight : 0
    color: "transparent"
    visible: !(activeWorkspace && activeWorkspace.hasFullscreen)
    // Reserve tiled space, but let floating and dragged windows cover the bar.
    WlrLayershell.layer: WlrLayer.Bottom
    WlrLayershell.namespace: "dotfiles-bar"
    onVisibleChanged: {
        if (!visible && menuController.openMenu && menuController.openMenu.anchorWindow === bar)
            menuController.openMenu.visible = false;
    }
    anchors {
        top: true
        left: true
        right: true
    }

    RowLayout {
        id: barLayout
        anchors.fill: parent
        anchors.leftMargin: 8
        anchors.rightMargin: 8
        spacing: 8

        WorkspaceGroup {
            id: workspaceGroup
            workspaces: Hyprland.workspaces.values
            activeWorkspace: bar.activeWorkspace
            activeToplevel: Hyprland.activeToplevel
            titleWidthBudget: Math.max(0, bar.width - controls.implicitWidth - clockGroup.implicitWidth - workspaceWidth - trayGroup.implicitWidth - 160)
            onWorkspaceRequested: number => Hyprland.dispatch("hl.dsp.focus({ workspace = " + number + " })")
        }

        Item {
            Layout.fillWidth: true
            Layout.minimumWidth: 0
        }

        BarGroup {
            id: mediaGroup
            readonly property real room: bar.width - workspaceGroup.width - controls.implicitWidth - clockGroup.implicitWidth - trayGroup.implicitWidth - 64
            visible: bar.player !== null && bar.player.playbackState !== MprisPlaybackState.Stopped && room >= 44
            Layout.preferredWidth: Math.max(36, Math.min(340, room))
            BarBlock {
                anchors.fill: parent
                glyph: bar.player && bar.player.isPlaying ? "\uf001" : "\uf04c"
                text: parent.width >= 140 ? bar.track : ""
                textColor: bar.player && bar.player.isPlaying ? Theme.purple : Theme.mutedStrong
                tooltip: bar.track + "\nClick: play/pause | Middle: previous | Right: next"
                onActivated: button => {
                    if (!bar.player)
                        return;
                    if (button === Qt.MiddleButton && bar.player.canGoPrevious)
                        bar.player.previous();
                    else if (button === Qt.RightButton && bar.player.canGoNext)
                        bar.player.next();
                    else if (button === Qt.LeftButton && bar.player.canTogglePlaying)
                        bar.player.togglePlaying();
                }
            }
        }

        BarGroup {
            id: trayGroup
            visible: SystemTray.items.values.length > 0
            implicitWidth: visible ? trayRow.implicitWidth : 0
            Row {
                id: trayRow
                Repeater {
                    model: SystemTray.items
                    AbstractButton {
                        id: trayItem
                        required property var modelData
                        implicitWidth: Theme.barControlHeight
                        implicitHeight: Theme.barControlHeight
                        hoverEnabled: true
                        focusPolicy: Qt.StrongFocus
                        Accessible.name: modelData.tooltipTitle || modelData.title || modelData.id
                        function showMenu() {
                            bar.menuController.dismiss();
                            const point = bar.contentItem.mapFromItem(trayItem, 0, trayItem.height);
                            modelData.display(bar, point.x, point.y);
                        }
                        onClicked: {
                            if (modelData.onlyMenu)
                                showMenu();
                            else
                                modelData.activate();
                        }
                        Keys.onMenuPressed: showMenu()
                        Keys.onReturnPressed: clicked()
                        background: Rectangle {
                            radius: Theme.controlRadius
                            color: trayItem.hovered || trayItem.down ? Theme.hover : "transparent"
                            border.width: trayItem.visualFocus ? 1 : 0
                            border.color: Theme.blue
                        }
                        contentItem: Item {
                            IconImage {
                                anchors.centerIn: parent
                                width: 16
                                height: 16
                                source: trayItem.modelData.icon
                            }
                        }
                        MouseArea {
                            anchors.fill: parent
                            acceptedButtons: Qt.MiddleButton | Qt.RightButton
                            onClicked: event => {
                                if (event.button === Qt.MiddleButton)
                                    trayItem.modelData.secondaryActivate();
                                else
                                    trayItem.showMenu();
                            }
                            onWheel: event => trayItem.modelData.scroll(event.angleDelta.y, false)
                        }
                        ShellToolTip {
                            text: trayItem.Accessible.name
                            visible: trayItem.hovered || trayItem.visualFocus
                        }
                        HoverHandler {
                            cursorShape: Qt.PointingHandCursor
                        }
                    }
                }
            }
        }

        BarGroup {
            id: controls
            implicitWidth: controlRow.implicitWidth
            Row {
                id: controlRow
                BarBlock {
                    id: stocksButton
                    glyph: "\uf201"
                    tooltip: "Markets"
                    selected: stocksMenu.visible && !stocksMenu.closing
                    onActivated: bar.toggleMenu(stocksMenu)
                }
                BarBlock {
                    id: networkButton
                    glyph: bar.network.icon
                    tooltip: !bar.network.available ? "Network status unavailable" : (bar.network.connected ? bar.network.connectionName : "Network")
                    textColor: !bar.network.available ? Theme.yellow : (bar.network.connected ? Theme.foreground : Theme.mutedStrong)
                    selected: networkMenu.visible && !networkMenu.closing
                    onActivated: button => {
                        if (button === Qt.RightButton) {
                            bar.menuController.dismiss();
                            networkEditor.startDetached();
                        } else {
                            bar.toggleMenu(networkMenu);
                        }
                    }
                }
                BarBlock {
                    id: volumeButton
                    glyph: bar.audio.muted ? "\uf6a9" : "\uf028"
                    tooltip: bar.audio.available ? (bar.audio.muted ? "Muted" : "Volume " + Math.round(bar.audio.volume * 100) + "%") + "\nScroll: volume | Middle click: mute" : "No audio output"
                    textColor: bar.audio.available && !bar.audio.muted ? Theme.foreground : Theme.mutedStrong
                    selected: volumeMenu.visible && !volumeMenu.closing
                    onActivated: button => {
                        if (button === Qt.MiddleButton)
                            bar.audio.toggleMuted();
                        else
                            bar.toggleMenu(volumeMenu);
                    }
                    onScrolled: steps => bar.audio.setVolume(bar.audio.volume + steps * 0.05)
                }
                BarBlock {
                    id: batteryButton
                    visible: bar.hasBattery
                    glyph: bar.batteryIcon
                    tooltip: Math.round(bar.charge * 100) + "% battery" + (bar.charging ? " - Charging" : "")
                    textColor: bar.charging ? Theme.green : (bar.charge <= 0.15 ? Theme.red : (bar.charge <= 0.3 ? Theme.yellow : Theme.foreground))
                    selected: batteryMenu.visible && !batteryMenu.closing
                    onActivated: bar.toggleMenu(batteryMenu)
                }
                BarBlock {
                    id: powerButton
                    glyph: "\uf011"
                    tooltip: "Power and session"
                    selected: powerMenu.visible && !powerMenu.closing
                    onActivated: bar.toggleMenu(powerMenu)
                }
            }
        }

        BarGroup {
            id: clockGroup
            implicitWidth: clockButton.implicitWidth
            BarBlock {
                id: clockButton
                text: Qt.formatDateTime(clock.date, bar.width < 800 ? "HH:mm" : "MMM dd  HH:mm")
                tooltip: Qt.formatDate(clock.date, "dddd, MMMM d")
                selected: calendarMenu.visible && !calendarMenu.closing
                onActivated: bar.toggleMenu(calendarMenu)
            }
        }
    }

    SystemClock {
        id: clock
        precision: SystemClock.Minutes
    }
    VolumeMenu {
        id: volumeMenu
        anchorItem: volumeButton
        audio: bar.audio
    }
    BatteryMenu {
        id: batteryMenu
        anchorItem: batteryButton
        battery: bar.battery
    }
    StocksMenu {
        id: stocksMenu
        anchorItem: stocksButton
        quotes: bar.markets.quotes
        updated: bar.markets.updated
        loading: bar.markets.loading
        error: bar.markets.error
        onRefreshRequested: bar.markets.refresh()
    }
    NetworkMenu {
        id: networkMenu
        anchorItem: networkButton
        network: bar.network
        onOpenEditorRequested: {
            bar.menuController.dismiss();
            networkEditor.startDetached();
        }
    }
    PowerMenu {
        id: powerMenu
        anchorItem: powerButton
    }
    CalendarMenu {
        id: calendarMenu
        anchorItem: clockButton
    }
    Process {
        id: networkEditor
        command: ["nm-connection-editor"]
    }
}
