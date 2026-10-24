import QtQuick
import Quickshell
import Quickshell.Services.Pipewire

Scope {
    id: root

    readonly property var sink: Pipewire.defaultAudioSink
    readonly property var sinkNodes: Pipewire.nodes.values.filter(node => node.isSink && !node.isStream && node.audio !== null)
    readonly property bool available: Pipewire.ready && sink !== null && sink.ready && sink.audio !== null && sinkNodes.includes(sink)
    readonly property real volume: available ? Math.max(0, Math.min(1.5, sink.audio.volume)) : 0
    readonly property bool muted: available && sink.audio.muted
    readonly property string outputId: available ? sink.name : ""
    readonly property string outputName: available ? sink.description || sink.nickname || sink.name : ""
    readonly property var outputs: sinkNodes.map(node => ({
                id: node.name,
                name: node.description || node.nickname || node.name,
                detail: "",
                available: Pipewire.ready && node.ready
            }))
    readonly property bool loading: !Pipewire.ready || (sink !== null && !sink.ready)
    readonly property bool switchingOutput: _requestedOutput !== ""
    readonly property bool busy: loading || switchingOutput
    readonly property string error: _actionError || (!Pipewire.ready ? "Audio service unavailable; waiting for PipeWire." : "")
    property string _requestedOutput: ""
    property string _actionError: ""

    // Bind device audio properties once for the shared service, across all screens.
    PwObjectTracker {
        objects: root.sinkNodes
    }

    function refresh() {
        // PipeWire pushes state changes; menu-open and retry need no subprocess.
        _actionError = "";
    }

    function setVolume(value) {
        if (available && !switchingOutput && Number.isFinite(value)) {
            _actionError = "";
            sink.audio.volume = Math.max(0, Math.min(1.5, value));
        }
    }

    function toggleMuted() {
        if (available && !switchingOutput) {
            _actionError = "";
            sink.audio.muted = !sink.audio.muted;
        }
    }

    function selectOutput(id) {
        if (!Pipewire.ready || switchingOutput)
            return;
        const node = sinkNodes.find(item => item.name === id && item.ready);
        if (!node) {
            _actionError = "That output is no longer available.";
            return;
        }
        _actionError = "";
        if (id === outputId)
            return;
        _requestedOutput = id;
        switchDeadline.restart();
        Pipewire.preferredDefaultAudioSink = node;
        settleSwitch();
    }

    function settleSwitch() {
        if (!_requestedOutput)
            return;
        if (outputId === _requestedOutput) {
            _requestedOutput = "";
            switchDeadline.stop();
        } else if (!Pipewire.ready || !sinkNodes.some(node => node.name === _requestedOutput && node.ready)) {
            _requestedOutput = "";
            switchDeadline.stop();
            _actionError = "That output was disconnected.";
        }
    }

    onOutputIdChanged: settleSwitch()
    onOutputsChanged: settleSwitch()

    Timer {
        id: switchDeadline
        objectName: "audioSwitchDeadline"
        interval: 5000
        onTriggered: {
            root._requestedOutput = "";
            root._actionError = "Could not switch audio output. Try another device.";
        }
    }
}
