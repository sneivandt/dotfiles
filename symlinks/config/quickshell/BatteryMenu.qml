import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell.Services.UPower
import "Theme.js" as Theme

ShellPopup {
    id: root

    required property var battery
    readonly property bool available: Boolean(battery && battery.ready)
    readonly property real charge: available ? battery.percentage : -1
    readonly property bool charging: available && battery.state === UPowerDeviceState.Charging
    readonly property bool fullyCharged: available && battery.state === UPowerDeviceState.FullyCharged
    readonly property color accentColor: !available ? Theme.mutedStrong : (charging || fullyCharged ? Theme.green : (charge <= 0.15 ? Theme.red : (charge <= 0.3 ? Theme.yellow : Theme.blue)))

    function batteryIcon() {
        if (charging)
            return "\uf0e7";
        if (!available)
            return "\uf128";
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
        if (!available)
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
        if (!available)
            return "Waiting for battery data";
        if (charging && battery.timeToFull > 0)
            return duration(battery.timeToFull) + " until full";
        if (battery.state === UPowerDeviceState.Discharging && battery.timeToEmpty > 0)
            return duration(battery.timeToEmpty) + " remaining";
        return stateLabel();
    }

    ColumnLayout {
        width: parent.width
        spacing: Theme.padding

        MenuHeader {
            id: header

            Layout.fillWidth: true
            icon: root.batteryIcon()
            title: "Battery"
            subtitle: root.stateLabel()
            accentColor: root.accentColor
            accentBackground: "transparent"
        }

        Flickable {
            id: scroll

            Layout.fillWidth: true
            implicitHeight: Math.min(contentHeight, Math.max(0, root.availableContentHeight - header.implicitHeight - Theme.padding))
            contentHeight: batteryContent.implicitHeight
            contentWidth: width
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            flickableDirection: Flickable.VerticalFlick
            activeFocusOnTab: contentHeight > height
            Keys.onDownPressed: contentY = Math.min(Math.max(0, contentHeight - height), contentY + 40)
            Keys.onUpPressed: contentY = Math.max(0, contentY - 40)
            ScrollBar.vertical: ShellScrollBar {}

            ColumnLayout {
                id: batteryContent

                width: scroll.width
                spacing: Theme.spacing

                Text {
                    Layout.fillWidth: true
                    text: root.available ? Math.round(root.charge * 100) + "%" : "--"
                    color: root.available ? Theme.foreground : Theme.mutedStrong
                    font.family: Theme.font
                    font.pixelSize: 40
                    font.weight: Font.DemiBold
                    Accessible.name: root.available ? text + " charged" : "Charge unavailable"
                }

                Text {
                    Layout.fillWidth: true
                    text: root.timeSummary()
                    color: Theme.mutedStrong
                    font.family: Theme.font
                    font.pixelSize: Theme.textBody
                    wrapMode: Text.WordWrap
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.topMargin: Theme.spacing
                    Layout.bottomMargin: Theme.spacing
                    implicitHeight: 6
                    radius: 3
                    color: Theme.borderSubtle
                    visible: root.available

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

                Repeater {
                    model: [
                        {
                            "label": "Power",
                            "value": root.available && root.battery.changeRate > 0 ? root.battery.changeRate.toFixed(1) + " W" : "--"
                        },
                        {
                            "label": "Health",
                            "value": root.available && root.battery.healthSupported ? Math.round(root.battery.healthPercentage * 100) + "%" : "--"
                        },
                        {
                            "label": "Capacity",
                            "value": root.available && root.battery.energyCapacity > 0 ? root.battery.energyCapacity.toFixed(1) + " Wh" : "--"
                        }
                    ]

                    RowLayout {
                        id: stat

                        required property var modelData

                        Layout.fillWidth: true
                        spacing: Theme.spacing

                        Text {
                            Layout.fillWidth: true
                            text: stat.modelData.label
                            color: Theme.mutedStrong
                            font.family: Theme.font
                            font.pixelSize: Theme.textSmall
                        }

                        Text {
                            text: stat.modelData.value
                            color: Theme.mutedStrong
                            font.family: Theme.font
                            font.pixelSize: Theme.textSmall
                        }
                    }
                }

                Text {
                    Layout.fillWidth: true
                    Layout.topMargin: Theme.spacing
                    text: root.available && root.battery.model ? root.battery.model : "System battery"
                    color: Theme.mutedStrong
                    font.family: Theme.font
                    font.pixelSize: Theme.textSmall
                    wrapMode: Text.Wrap
                }
            }
        }
    }
}
