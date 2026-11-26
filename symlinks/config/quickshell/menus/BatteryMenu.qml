import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell.Services.UPower
import "../components"
import "../lib/Battery.js" as Battery
import "../lib/Theme.js" as Theme

ShellPopup {
    id: root

    required property var battery
    panelWidth: 304
    readonly property bool available: Boolean(battery && battery.ready)
    readonly property real charge: available ? battery.percentage : -1
    readonly property bool charging: available && battery.state === UPowerDeviceState.Charging
    readonly property bool fullyCharged: available && battery.state === UPowerDeviceState.FullyCharged
    readonly property real watts: available && Number.isFinite(battery.changeRate) ? Math.abs(battery.changeRate) : 0
    readonly property bool hasWatts: available && watts >= 0.1
    readonly property real health: available && battery.healthSupported && Number.isFinite(battery.healthPercentage) ? battery.healthPercentage : -1
    readonly property bool hasHealth: available && health > 0
    readonly property color accentColor: !available ? Theme.mutedStrong : (charging || fullyCharged ? Theme.green : (charge <= 0.15 ? Theme.red : (charge <= 0.3 ? Theme.yellow : Theme.blue)))

    function powerSummary() {
        if (!hasWatts)
            return "";
        return watts.toFixed(1) + " W " + (charging ? "charging" : "draw");
    }

    function batteryIcon() {
        return Battery.icon(available, charging, fullyCharged, charge);
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
            return "";

        if (charging && battery.timeToFull > 0)
            return duration(battery.timeToFull) + " until full";

        if (battery.state === UPowerDeviceState.Discharging && battery.timeToEmpty > 0)
            return duration(battery.timeToEmpty) + " remaining";

        return "";
    }

    ColumnLayout {
        width: parent.width
        spacing: Theme.spacing

        MenuHeader {
            id: header

            Layout.fillWidth: true
            icon: root.batteryIcon()
            title: "Battery"
            subtitle: root.stateLabel()
            accentColor: root.accentColor
            accentBackground: !root.available ? Theme.raised : (root.charging || root.fullyCharged ? Theme.greenSoft : (root.charge <= 0.15 ? Theme.redSoft : (root.charge <= 0.3 ? Theme.yellowSoft : Theme.blueSoft)))
        }

        Flickable {
            id: scroll

            Layout.fillWidth: true
            implicitHeight: Math.min(contentHeight, Math.max(0, root.availableContentHeight - header.implicitHeight - Theme.spacing))
            contentHeight: batteryContent.implicitHeight
            contentWidth: width
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            flickableDirection: Flickable.VerticalFlick
            activeFocusOnTab: contentHeight > height
            Keys.onDownPressed: contentY = Math.min(Math.max(0, contentHeight - height), contentY + 40)
            Keys.onUpPressed: contentY = Math.max(0, contentY - 40)

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
                    visible: text.length > 0
                    color: Theme.mutedStrong
                    font.family: Theme.font
                    font.pixelSize: Theme.textBody
                    wrapMode: Text.WordWrap
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.topMargin: 4
                    Layout.bottomMargin: 4
                    implicitHeight: 10
                    radius: 5
                    color: Theme.borderSubtle
                    visible: root.available

                    Rectangle {
                        width: Math.max(0, Math.min(1, root.charge)) * parent.width
                        height: parent.height
                        radius: parent.radius
                        color: root.accentColor

                        Rectangle {
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            width: parent.width > 8 ? 4 : 0
                            height: parent.height - 4
                            radius: 2
                            color: Theme.foreground
                            opacity: 0.75
                        }

                        Behavior on width {
                            NumberAnimation {
                                duration: Theme.animationNormal
                                easing.type: Easing.OutCubic
                            }

                        }

                    }

                }

                RowLayout {
                    visible: root.hasWatts || root.hasHealth
                    Layout.fillWidth: true
                    Layout.topMargin: 2
                    spacing: Theme.spacing

                    Text {
                        objectName: "batteryPowerRate"
                        visible: root.hasWatts
                        Layout.fillWidth: true
                        text: root.powerSummary()
                        textFormat: Text.PlainText
                        color: Theme.mutedStrong
                        font.family: Theme.font
                        font.pixelSize: Theme.textSmall
                        elide: Text.ElideRight
                    }

                    Text {
                        objectName: "batteryHealth"
                        visible: root.hasHealth
                        Layout.alignment: Qt.AlignRight
                        text: Math.round(root.health) + "% health"
                        textFormat: Text.PlainText
                        color: Theme.mutedStrong
                        font.family: Theme.font
                        font.pixelSize: Theme.textSmall
                    }
                }

            }

            ScrollBar.vertical: ShellScrollBar {
            }

        }

    }

}
