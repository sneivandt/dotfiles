import QtQuick
import QtTest
import "../../../../symlinks/config/quickshell/Battery.js" as Battery

TestCase {
    name: "Battery"

    function test_icon_states_and_thresholds() {
        compare(Battery.icon(false, false, false, -1), "\uf128");
        compare(Battery.icon(true, true, false, 0.1), "\uf0e7");
        compare(Battery.icon(true, false, true, 0.8), "\uf240");
        compare(Battery.icon(true, false, false, 0.15), "\uf244");
        compare(Battery.icon(true, false, false, 0.35), "\uf243");
        compare(Battery.icon(true, false, false, 0.6), "\uf242");
        compare(Battery.icon(true, false, false, 0.85), "\uf241");
        compare(Battery.icon(true, false, false, 0.9), "\uf240");
    }
}
