pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Services.Mpris
import Quickshell.Services.SystemTray
import Quickshell.Services.UPower
import Quickshell.Wayland
import "Battery.js" as Battery
import "MediaPlayer.js" as MediaPlayer
import "Theme.js" as Theme

PanelWindow {
    id: bar
    required property var modelData
    required property var audio
    required property var network
    required property var markets
    required property date currentDate
    required property var menuController
    readonly property var hyprMonitor: Hyprland.monitorFor(screen)
    readonly property var activeWorkspace: hyprMonitor ? hyprMonitor.activeWorkspace : null
    readonly property bool hasFullscreen: Boolean(activeWorkspace && activeWorkspace.hasFullscreen)
    readonly property var player: MediaPlayer.select(Mpris.players.values, MprisPlaybackState.Playing, MprisPlaybackState.Paused)
    readonly property string track: player ? (player.trackArtist ? player.trackArtist + " - " : "") + (player.trackTitle || "Unknown track") : ""
    readonly property var battery: UPower.displayDevice
    readonly property bool hasBattery: battery && battery.ready && battery.isLaptopBattery
    readonly property real charge: hasBattery ? battery.percentage : 0
    readonly property bool charging: hasBattery && battery.state === UPowerDeviceState.Charging
    readonly property bool fullyCharged: hasBattery && battery.state === UPowerDeviceState.FullyCharged
    readonly property string batteryIcon: Battery.icon(hasBattery, charging, fullyCharged, charge)

    function toggleMenu(menu) {
        menuController.toggle(menu);
    }

    screen: modelData
    implicitHeight: Theme.barHeight
    exclusiveZone: Theme.barHeight
    color: "transparent"
    // Reserve tiled space, but let floating, dragged, and fullscreen windows cover the bar.
    WlrLayershell.layer: WlrLayer.Bottom
    WlrLayershell.namespace: "dotfiles-bar"
    onHasFullscreenChanged: {
        if (hasFullscreen && menuController.openMenu && menuController.openMenu.anchorWindow === bar)
            menuController.dismiss();
    }
    anchors {
        top: true
        left: true
        right: true
    }

    RowLayout {
        id: barLayout
        anchors.fill: parent
        anchors.topMargin: Theme.barTopMargin
        anchors.bottomMargin: Theme.barBottomMargin
        anchors.leftMargin: Theme.windowGap
        anchors.rightMargin: Theme.windowGap
        spacing: Theme.spacing

        WorkspaceGroup {
            id: workspaceGroup
            workspaces: Hyprland.workspaces.values
            activeWorkspace: bar.activeWorkspace
            activeToplevel: Hyprland.activeToplevel
            maximumWorkspaceWidth: Math.max(68, bar.width - controls.implicitWidth - clockGroup.implicitWidth - Math.min(112, trayGroup.naturalWidth) - 48)
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
            visible: bar.player !== null && room >= 44
            Layout.preferredWidth: Math.max(36, Math.min(mediaBlock.implicitWidth, 340, room))
            BarBlock {
                id: mediaBlock
                anchors.fill: parent
                glyph: bar.player && bar.player.isPlaying ? "\uf001" : "\uf04c"
                text: mediaGroup.room >= 140 ? bar.track : ""
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

        TrayGroup {
            id: trayGroup
            maximumWidth: Math.max(68, bar.width - controls.implicitWidth - clockGroup.implicitWidth - workspaceGroup.workspaceWidth - 48)
            items: SystemTray.items.values
            anchorWindow: bar
            menuController: bar.menuController
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
                    tooltip: !bar.network.available ? "Network status unavailable" : (bar.network.connected ? (bar.network.connectionName || "Connected") : "Network")
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
                    id: powerButton
                    glyph: "\uf011"
                    tooltip: "Power and session"
                    selected: powerMenu.visible && !powerMenu.closing
                    onActivated: bar.toggleMenu(powerMenu)
                }
                BarBlock {
                    id: batteryButton
                    visible: bar.hasBattery
                    glyph: bar.batteryIcon
                    tooltip: Math.round(bar.charge * 100) + "% battery" + (bar.charging ? " - Charging" : (bar.fullyCharged ? " - Fully charged" : ""))
                    textColor: bar.charging || bar.fullyCharged ? Theme.green : (bar.charge <= 0.15 ? Theme.red : (bar.charge <= 0.3 ? Theme.yellow : Theme.foreground))
                    selected: batteryMenu.visible && !batteryMenu.closing
                    onActivated: bar.toggleMenu(batteryMenu)
                }
            }
        }

        BarGroup {
            id: clockGroup
            implicitWidth: clockButton.implicitWidth
            BarBlock {
                id: clockButton
                text: Qt.formatDateTime(bar.currentDate, bar.width < 800 ? "HH:mm" : "ddd MMM dd  HH:mm")
                tooltip: Qt.formatDate(bar.currentDate, "dddd, MMMM d")
                selected: calendarMenu.visible && !calendarMenu.closing
                onActivated: bar.toggleMenu(calendarMenu)
            }
        }
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
        missingSymbols: bar.markets.missingSymbols
        updated: bar.markets.updated
        currentDate: bar.currentDate
        loading: bar.markets.loading
        error: bar.markets.error
        onRefreshRequested: bar.markets.refresh(true)
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
        currentDate: bar.currentDate
    }
    Process {
        id: networkEditor
        command: ["sh", "-c", "if command -v uwsm >/dev/null 2>&1 && uwsm check is-active >/dev/null 2>&1; then exec uwsm app -t service -- nm-connection-editor; fi; exec nm-connection-editor"]
    }
}
