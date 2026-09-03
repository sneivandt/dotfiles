import QtQuick
import Quickshell
import Quickshell.Io

Scope {
    id: root

    property real volume: 0
    property bool muted: false
    property bool available: false

    function refresh() {
        if (!volumeQuery.running)
            volumeQuery.running = true;

        if (!muteQuery.running)
            muteQuery.running = true;

    }

    function setVolume(value) {
        const next = Math.max(0, Math.min(1.5, value));
        root.volume = next;
        volumeCommand.exec(["pactl", "set-sink-volume", "@DEFAULT_SINK@", Math.round(next * 100) + "%"]);
    }

    function toggleMuted() {
        root.muted = !root.muted;
        muteCommand.exec(["pactl", "set-sink-mute", "@DEFAULT_SINK@", "toggle"]);
    }

    Component.onCompleted: refresh()

    Process {
        id: volumeQuery

        command: ["pactl", "get-sink-volume", "@DEFAULT_SINK@"]
        onExited: (code) => {
            const match = volumeOutput.text.match(/(\d+)%/);
            if (code === 0 && match) {
                root.volume = Number(match[1]) / 100;
                root.available = true;
            } else {
                root.available = false;
            }
        }

        stdout: StdioCollector {
            id: volumeOutput
        }
    }

    Process {
        id: muteQuery

        command: ["pactl", "get-sink-mute", "@DEFAULT_SINK@"]
        onExited: (code) => {
            if (code === 0)
                root.muted = /yes\s*$/m.test(muteOutput.text);

        }

        stdout: StdioCollector {
            id: muteOutput
        }
    }

    Process {
        id: volumeCommand

        onExited: root.refresh()
    }

    Process {
        id: muteCommand

        onExited: root.refresh()
    }

    Timer {
        interval: 1000
        repeat: true
        running: true
        onTriggered: root.refresh()
    }
}
