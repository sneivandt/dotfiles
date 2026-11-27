import QtQuick
import QtTest
import Quickshell
import Quickshell.Services.UPower
import "../../../../symlinks/config/quickshell/menus" as Menus
import "../../../../symlinks/config/quickshell/lib/Theme.js" as Theme

Item {
    width: 400
    height: 500

    Component {
        id: batteryMenuComponent

        Menus.BatteryMenu {
            anchorItem: null
            battery: QtObject {
                property bool ready: true
                property real percentage: 0.72
                property int state: UPowerDeviceState.Discharging
                property real changeRate: 12.45
                property bool healthSupported: true
                property real healthPercentage: 91.6
                property real timeToFull: 0
                property real timeToEmpty: 5400
            }
        }
    }

    TestCase {
        name: "BatteryMenu"
        when: windowShown

        function initTestCase() {
            verify(Quickshell.testMode === true, "Run with -import tests/qml/mocks; live Quickshell services are forbidden.");
        }

        function test_discharging_charging_and_low_battery_states() {
            const menu = createTemporaryObject(batteryMenuComponent, parent);
            verify(menu !== null);
            menu.visible = true;
            tryCompare(menu, "reveal", 1);

            compare(menu.available, true);
            compare(menu.stateLabel(), "On battery");
            compare(menu.timeSummary(), "1h 30m remaining");
            compare(menu.powerSummary(), "12.4 W draw");
            compare(menu.accentColor, Qt.color(Theme.blue));
            compare(findChild(menu, "batteryPowerRate").visible, true);
            compare(findChild(menu, "batteryHealth").text, "92% health");

            menu.battery.percentage = 0.25;
            compare(menu.accentColor, Qt.color(Theme.yellow));
            menu.battery.percentage = 0.12;
            compare(menu.accentColor, Qt.color(Theme.red));

            menu.battery.state = UPowerDeviceState.Charging;
            menu.battery.changeRate = -24.2;
            menu.battery.timeToFull = 2700;
            compare(menu.stateLabel(), "Charging");
            compare(menu.timeSummary(), "45m until full");
            compare(menu.powerSummary(), "24.2 W charging");
            compare(menu.accentColor, Qt.color(Theme.green));

            menu.battery.state = UPowerDeviceState.FullyCharged;
            menu.battery.changeRate = 0.04;
            menu.battery.healthSupported = false;
            compare(menu.stateLabel(), "Fully charged");
            compare(menu.timeSummary(), "");
            compare(menu.powerSummary(), "");
            compare(findChild(menu, "batteryPowerRate").visible, false);
            compare(findChild(menu, "batteryHealth").visible, false);
        }

        function test_pending_empty_and_unavailable_states() {
            const menu = createTemporaryObject(batteryMenuComponent, parent);
            verify(menu !== null);

            menu.battery.state = UPowerDeviceState.PendingCharge;
            compare(menu.stateLabel(), "Waiting to charge");
            menu.battery.state = UPowerDeviceState.PendingDischarge;
            compare(menu.stateLabel(), "Waiting to discharge");
            menu.battery.state = UPowerDeviceState.Empty;
            compare(menu.stateLabel(), "Empty");
            compare(menu.duration(3600), "1h");
            compare(menu.duration(20), "1m");

            menu.battery.ready = false;
            compare(menu.available, false);
            compare(menu.stateLabel(), "Battery unavailable");
            compare(menu.timeSummary(), "");
            compare(menu.powerSummary(), "");
            compare(menu.accentColor, Qt.color(Theme.mutedStrong));
        }
    }
}
