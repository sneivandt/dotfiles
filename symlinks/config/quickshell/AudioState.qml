import QtQuick
import Quickshell
import Quickshell.Io

Scope {
    id: root

    property real volume: 0
    property bool muted: false
    property bool available: false
    property string outputId: ""
    property string outputName: ""
    property var outputs: []
    readonly property bool loading: _refreshing || refreshDelay.running
    readonly property bool busy: loading || _commandActive || _commands.length > 0
    readonly property bool switchingOutput: (_currentCommand !== null && _currentCommand.kind === "output") || _commands.some(item => item.kind === "output")
    readonly property string error: _commandError || _queryError || _subscriptionError

    property bool _refreshing: false
    property bool _refreshAgain: false
    property bool _queryActive: false
    property string _queryPhase: ""
    property string _snapshotDefault: ""
    property string _queryError: ""
    property string _subscriptionError: ""
    property string _commandError: ""
    property bool _subscriptionActive: false
    property int _retryDelay: 1000
    property bool _commandActive: false
    property var _commands: []
    property var _currentCommand: null

    function refresh(retry) {
        if (retry) {
            _commandError = "";
            if (!_subscriptionActive) {
                reconnect.stop();
                startSubscription();
            }
        }
        _refreshAgain = true;
        if (!_refreshing && !refreshDelay.running)
            refreshDelay.start();
    }

    function beginRefresh() {
        if (_refreshing)
            return;
        _refreshAgain = false;
        _refreshing = true;
        startQuery("info");
    }

    function startQuery(phase) {
        _queryPhase = phase;
        _queryActive = true;
        query.command = phase === "info" ? ["pactl", "--format=json", "info"] : ["pactl", "--format=json", "list", "sinks"];
        queryDeadline.restart();
        query.running = true;
    }

    function queryFailed(message) {
        _queryActive = false;
        _refreshing = false;
        queryDeadline.stop();
        _queryError = message;
        available = false;
        outputId = "";
        outputName = "";
        outputs = [];
        _commands = [];
        recovery.restart();
    }

    function queryFinished(code) {
        if (!_queryActive)
            return;
        _queryActive = false;
        queryDeadline.stop();
        if (code !== 0) {
            queryFailed("Cannot read audio devices. " + failureDetail(queryErrors.text, code));
            return;
        }

        try {
            const result = JSON.parse(queryOutput.text);
            if (_queryPhase === "info") {
                if (!result || (typeof result.default_sink_name !== "string" && result.default_sink_name !== null))
                    throw new Error("Missing default output in server response.");
                _snapshotDefault = result.default_sink_name || "";
                Qt.callLater(() => root.startQuery("sinks"));
                return;
            }
            if (!Array.isArray(result))
                throw new Error("Invalid output device list.");

            const devices = result.map(sink => {
                if (!sink || typeof sink.name !== "string" || typeof sink.mute !== "boolean")
                    throw new Error("Invalid output device.");
                const ports = Array.isArray(sink.ports) ? sink.ports : [];
                const port = ports.find(item => item.name === sink.active_port);
                return {
                    id: sink.name,
                    name: sink.description || sink.name,
                    detail: port ? port.description || port.name : "",
                    available: !port || (port.availability !== "not available" && port.availability !== "no")
                };
            });
            const current = result.find(sink => sink.name === _snapshotDefault);
            const device = devices.find(item => item.id === _snapshotDefault);
            let currentVolume = 0;
            if (current) {
                const channels = Object.values(current.volume || {});
                if (channels.length === 0 || channels.some(channel => typeof channel.value !== "number" || !Number.isFinite(channel.value)))
                    throw new Error("Invalid output volume.");
                // PulseAudio's fixed-point unity is 65536; never parse localized percentages.
                currentVolume = channels.reduce((sum, channel) => sum + channel.value, 0) / channels.length / 65536;
            }

            outputs = devices;
            outputId = current ? current.name : "";
            outputName = device ? device.name : "";
            available = !!current && device.available && _subscriptionActive;
            if (!_commandActive && _commands.length === 0) {
                volume = Math.max(0, Math.min(1.5, currentVolume));
                muted = current ? current.mute : false;
            }
            _refreshing = false;
            _queryError = "";
            recovery.stop();
            if (_subscriptionActive)
                _retryDelay = 1000;
            if (_refreshAgain)
                refresh();
        } catch (exception) {
            queryFailed("Cannot read audio state: " + exception.message);
        }
    }

    function failureDetail(text, code) {
        const detail = text.trim().replace(/\s+/g, " ");
        return detail ? detail.slice(0, 180) : "pactl exited with code " + code + ".";
    }

    function enqueue(kind, target, value) {
        _commandError = "";
        // Coalesce slider/wheel updates instead of cancelling an in-flight pactl.
        _commands = _commands.filter(item => item.kind !== kind || item.target !== target).concat([
            {
                kind: kind,
                target: target,
                value: value
            }
        ]);
        Qt.callLater(root.runNextCommand);
    }

    function setVolume(value) {
        if (!available || switchingOutput || !Number.isFinite(value))
            return;
        const next = Math.max(0, Math.min(1.5, value));
        volume = next;
        enqueue("volume", outputId, Math.round(next * 100) + "%");
    }

    function toggleMuted() {
        if (!available || switchingOutput)
            return;
        muted = !muted;
        enqueue("mute", outputId, muted ? "1" : "0");
    }

    function selectOutput(id) {
        if (!_subscriptionActive)
            return;
        const device = outputs.find(item => item.id === id && item.available);
        if (!device) {
            _commandError = "That output is no longer available.";
            refresh();
            return;
        }
        if (id === outputId || switchingOutput)
            return;
        enqueue("output", id, "");
    }

    function runNextCommand() {
        if (_commandActive || command.running || _commands.length === 0)
            return;
        _currentCommand = _commands[0];
        _commands = _commands.slice(1);
        const item = _currentCommand;
        // Pin writes to the device selected when the gesture began, not a new default.
        if (!outputs.some(device => device.id === item.target && device.available)) {
            commandFailed("That output was disconnected.");
            return;
        }
        _commandActive = true;
        command.command = item.kind === "output" ? ["pactl", "set-default-sink", item.target] : ["pactl", item.kind === "mute" ? "set-sink-mute" : "set-sink-volume", item.target, item.value];
        commandDeadline.restart();
        command.running = true;
    }

    function commandFailed(message) {
        _commandActive = false;
        _currentCommand = null;
        _commands = [];
        commandDeadline.stop();
        _commandError = message;
        refresh();
    }

    function startSubscription() {
        _subscriptionActive = true;
        subscription.running = true;
    }

    function subscriptionFailed(message) {
        if (!_subscriptionActive)
            return;
        _subscriptionActive = false;
        _subscriptionError = message;
        available = false;
        outputId = "";
        outputName = "";
        outputs = [];
        _commands = [];
        reconnect.interval = _retryDelay;
        _retryDelay = Math.min(30000, _retryDelay * 2);
        reconnect.restart();
    }

    Component.onCompleted: {
        startSubscription();
        refresh();
    }

    Process {
        id: subscription

        command: ["pactl", "subscribe"]
        environment: ({
                LC_ALL: "C"
            })
        onStarted: {
            root._subscriptionError = "";
            root.refresh();
        }
        onExited: code => root.subscriptionFailed("Audio updates disconnected; retrying. " + root.failureDetail(subscriptionErrors.text, code))
        onRunningChanged: {
            if (!running && root._subscriptionActive)
                root.subscriptionFailed("Cannot start audio updates. Check that pactl is installed; retrying.");
        }
        stdout: SplitParser {
            onRead: data => {
                if (/ on (sink|server|card)(?: |$)/.test(data)) {
                    if (/ on server|Event 'remove' on sink/.test(data))
                        root.available = false;
                    root.refresh();
                }
            }
        }
        stderr: StdioCollector {
            id: subscriptionErrors
        }
    }

    Process {
        id: query

        environment: ({
                LC_ALL: "C"
            })
        onExited: code => root.queryFinished(code)
        onRunningChanged: {
            if (!running && root._queryActive)
                root.queryFailed("Cannot start audio query. Check that pactl is installed.");
        }
        stdout: StdioCollector {
            id: queryOutput
        }
        stderr: StdioCollector {
            id: queryErrors
        }
    }

    Process {
        id: command

        environment: ({
                LC_ALL: "C"
            })
        onExited: code => {
            if (!root._commandActive)
                return;
            commandDeadline.stop();
            if (code !== 0) {
                root.commandFailed("Audio change failed. " + root.failureDetail(commandErrors.text, code));
                return;
            }
            if (root._currentCommand.kind === "output")
                root.available = false;
            root._commandActive = false;
            root._currentCommand = null;
            root.refresh();
            Qt.callLater(root.runNextCommand);
        }
        onRunningChanged: {
            if (!running && root._commandActive)
                root.commandFailed("Cannot change audio. Check that pactl is installed.");
        }
        stderr: StdioCollector {
            id: commandErrors
        }
    }

    Timer {
        id: refreshDelay

        interval: 60
        onTriggered: root.beginRefresh()
    }

    Timer {
        id: queryDeadline

        interval: 5000
        onTriggered: {
            root.queryFailed("Audio query timed out; retrying.");
            query.signal(9);
        }
    }

    Timer {
        id: commandDeadline

        interval: 5000
        onTriggered: {
            root.commandFailed("Audio change timed out.");
            command.signal(9);
        }
    }

    Timer {
        id: reconnect

        onTriggered: root.startSubscription()
    }

    Timer {
        id: recovery

        interval: 5000
        onTriggered: root.refresh()
    }
}
