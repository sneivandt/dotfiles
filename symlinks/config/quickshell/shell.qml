import QtQml
import Quickshell

ShellRoot {
    Connections {
        target: Quickshell

        function onReloadCompleted() {
            Quickshell.inhibitReloadPopup();
        }
    }

    Variants {
        model: Quickshell.screens

        Bar {
        }
    }
}
