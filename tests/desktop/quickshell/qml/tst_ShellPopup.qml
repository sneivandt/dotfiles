import QtQuick
import QtTest
import Quickshell
import "../../../../symlinks/config/quickshell/components" as Components

Item {
    width: 480
    height: 600

    Component {
        id: popupComponent

        Components.ShellPopup {
            anchorItem: null
            panelWidth: 320

            Rectangle {
                width: parent.width
                height: 120
            }
        }
    }

    TestCase {
        name: "ShellPopup"
        when: windowShown

        function initTestCase() {
            verify(Quickshell.testMode === true, "Run with -import tests/qml/mocks; live Quickshell services are forbidden.");
        }

        function test_open_and_escape_dismissal() {
            const popup = createTemporaryObject(popupComponent, parent);
            verify(popup !== null);
            compare(popup.visible, false);

            popup.visible = true;
            tryCompare(popup, "reveal", 1);
            compare(popup.closing, false);

            keyClick(Qt.Key_Escape);
            tryCompare(popup, "visible", false);
            compare(popup.reveal, 0);
        }

        function test_escape_emits_signal_when_dismiss_on_escape_is_false() {
            const popup = createTemporaryObject(popupComponent, parent, {
                dismissOnEscape: false
            });
            verify(popup !== null);
            let escaped = 0;
            popup.escapePressed.connect(() => escaped++);

            popup.visible = true;
            tryCompare(popup, "reveal", 1);

            keyClick(Qt.Key_Escape);
            compare(escaped, 1);
            compare(popup.visible, true);
            compare(popup.closing, false);

            popup.close();
            tryCompare(popup, "visible", false);
        }
    }
}
