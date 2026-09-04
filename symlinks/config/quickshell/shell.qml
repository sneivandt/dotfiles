pragma ComponentBehavior: Bound

import QtQml
import Quickshell

ShellRoot {
    id: shell
    property var openMenu: null

    function toggle(menu) {
        if (menu.visible && !menu.closing) {
            menu.close();
        } else {
            dismiss();
            openMenu = menu;
            menu.visible = true;
        }
    }

    function dismiss() {
        if (openMenu)
            openMenu.visible = false;
        openMenu = null;
    }

    AudioState {
        id: audioState
    }
    NetworkState {
        id: networkState
    }
    MarketState {
        id: marketState
    }
    ReloadNotice {
        id: reloadNotice
    }

    Connections {
        target: Quickshell

        function onReloadCompleted() {
            Quickshell.inhibitReloadPopup();
            reloadNotice.showResult("");
        }

        function onReloadFailed(errorString) {
            Quickshell.inhibitReloadPopup();
            reloadNotice.showResult(errorString);
        }
    }

    Variants {
        model: Quickshell.screens

        Bar {
            audio: audioState
            network: networkState
            markets: marketState
            menuController: shell
        }
    }
}
