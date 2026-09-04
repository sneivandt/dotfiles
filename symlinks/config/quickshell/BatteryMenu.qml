import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Services.UPower
import "Theme.js" as Theme

PopupWindow {
    id: root

    required property Item anchorItem
    required property var battery
    readonly property real charge: battery && battery.ready ? battery.percentage : 0
    readonly property bool charging: battery && battery.state === UPowerDeviceState.Charging
    readonly property bool fullyCharged: battery && battery.state === UPowerDeviceState.FullyCharged
    readonly property color accentColor: charging || fullyCharged ? Theme.green : (charge <= 0.15 ? Theme.red : (charge <= 0.3 ? Theme.yellow : Theme.blue))
    readonly property color accentBackground: charging || fullyCharged ? Theme.greenSoft : (charge <= 0.15 ? Theme.redSoft : (charge <= 0.3 ? Theme.yellowSoft : Theme.blueSoft))

    function batteryIcon() {
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

    function stateLabel() {
        if (!battery || !battery.ready)
            return "Battery unavailable";

        switch (battery.state) {
        case UPowerDeviceState.Charging:
            return "Charging";
        case UPowerDeviceState.Discharging:
            return "On battery";
        case UPowerDeviceState.Empty:
            return "Empty";
        case UPowerDeviceState.FullyCharged:
            return "Fully charged";
        case UPowerDeviceState.PendingCharge:
            return "Waiting to charge";
        case UPowerDeviceState.PendingDischarge:
            return "Waiting to discharge";
        default:
            return "Battery status unknown";
        }
    }

    function duration(seconds) {
        const totalMinutes = Math.max(1, Math.round(seconds / 60));
        const hours = Math.floor(totalMinutes / 60);
        const minutes = totalMinutes % 60;

        if (hours === 0)
            return minutes + "m";

        if (minutes === 0)
            return hours + "h";

        return hours + "h " + minutes + "m";
    }

    function timeSummary() {
        if (!battery || !battery.ready)
            return "Waiting for battery data";

        if (charging && battery.timeToFull > 0)
            return duration(battery.timeToFull) + " until full";

        if (battery.state === UPowerDeviceState.Discharging && battery.timeToEmpty > 0)
            return duration(battery.timeToEmpty) + " remaining";

        return stateLabel();
    }

    implicitWidth: 370
    implicitHeight: 244
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
            spacing: 6

            MenuHeader {
                Layout.fillWidth: true
                Layout.leftMargin: 2
                Layout.rightMargin: 2
                Layout.bottomMargin: 4
                icon: root.batteryIcon()
                title: "Battery"
                subtitle: root.battery && root.battery.model.length > 0 ? root.battery.model : "System battery"
                accentColor: root.accentColor
                accentBackground: root.accentBackground
                trailingItem: Rectangle {
                    implicitWidth: percentageText.implicitWidth + 18
                    implicitHeight: 28
                    radius: Theme.controlRadius
                    color: Theme.raised
                    border.width: 1
                    border.color: Theme.borderSubtle

                    Text {
                        id: percentageText

                        anchors.centerIn: parent
                        text: Math.round(root.charge * 100) + "%"
                        color: root.accentColor
                        font.family: Theme.font
                        font.pixelSize: 11
                        font.weight: Font.DemiBold
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 94
                radius: Theme.itemRadius
                color: Theme.raised
                border.width: 1
                border.color: Theme.borderSubtle

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 12
                    spacing: 8

                    RowLayout {
                        Layout.fillWidth: true

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 1

                            Text {
                                Layout.fillWidth: true
                                text: "Charge level"
                                color: Theme.foreground
                                font.family: Theme.font
                                font.pixelSize: 12
                                font.weight: Font.DemiBold
                            }

                            Text {
                                Layout.fillWidth: true
                                text: root.stateLabel()
                                color: root.accentColor
                                font.family: Theme.font
                                font.pixelSize: 10
                            }
                        }

                        Text {
                            text: root.timeSummary()
                            color: Theme.mutedStrong
                            font.family: Theme.font
                            font.pixelSize: 10
                        }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: 8
                        radius: 4
                        color: Theme.borderSubtle

                        Rectangle {
                            width: Math.max(0, Math.min(1, root.charge)) * parent.width
                            height: parent.height
                            radius: parent.radius
                            color: root.accentColor

                            Behavior on width {
                                NumberAnimation {
                                    duration: Theme.animationNormal
                                    easing.type: Easing.OutCubic
                                }
                            }
                        }
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 6

                Repeater {
                    model: [
                        {
                            "label": "POWER",
                            "value": root.battery && root.battery.changeRate > 0 ? root.battery.changeRate.toFixed(1) + " W" : "--",
                            "color": Theme.cyan
                        },
                        {
                            "label": "HEALTH",
                            "value": root.battery && root.battery.healthSupported ? Math.round(root.battery.healthPercentage * 100) + "%" : "--",
                            "color": Theme.green
                        },
                        {
                            "label": "CAPACITY",
                            "value": root.battery && root.battery.energyCapacity > 0 ? root.battery.energyCapacity.toFixed(1) + " Wh" : "--",
                            "color": Theme.purple
                        }
                    ]

                    Rectangle {
                        id: statCard

                        required property var modelData

                        Layout.fillWidth: true
                        implicitHeight: 58
                        radius: Theme.controlRadius
                        color: Theme.raised
                        border.width: 1
                        border.color: Theme.borderSubtle

                        ColumnLayout {
                            anchors.centerIn: parent
                            spacing: 2

                            Text {
                                Layout.alignment: Qt.AlignHCenter
                                text: statCard.modelData.value
                                color: statCard.modelData.color
                                font.family: Theme.font
                                font.pixelSize: 12
                                font.weight: Font.DemiBold
                            }

                            Text {
                                Layout.alignment: Qt.AlignHCenter
                                text: statCard.modelData.label
                                color: Theme.mutedStrong
                                font.family: Theme.font
                                font.pixelSize: 9
                                font.weight: Font.DemiBold
                            }
                        }
                    }
                }
            }
        }
    }
}
