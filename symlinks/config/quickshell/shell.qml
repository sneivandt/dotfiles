//@ pragma UseQApplication
pragma ComponentBehavior: Bound

import QtQml
import Quickshell
import Quickshell.Io
import "bar"
import "menus"
import "overlays"
import "state"

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
        menuOpen: shell.openMenu instanceof NetworkMenu && shell.openMenu.visible
    }
    MarketState {
        id: marketState
        menuOpen: shell.openMenu instanceof StocksMenu && shell.openMenu.visible
    }
    SystemClock {
        id: clock
        precision: SystemClock.Minutes
    }
    ReloadNotice {
        id: reloadNotice
    }
    Osd {
        id: osd
        audio: audioState
        suppressAudio: shell.openMenu instanceof VolumeMenu && shell.openMenu.visible
    }
    IpcHandler {
        target: "osd"
        function display(kind: string, percent: string, muted: string): void {
            osd.show(kind, percent, muted);
        }
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
            currentDate: clock.date
            menuController: shell
        }
    }
}
