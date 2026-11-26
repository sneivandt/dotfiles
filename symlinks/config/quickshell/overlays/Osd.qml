pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Hyprland
import Quickshell.Wayland
import "../lib/Theme.js" as Theme

PanelWindow {
    id: root

    required property var audio
    property bool suppressAudio: false
    property real reveal: 0
    readonly property int shadowMargin: 8
    property bool _audioInitialized: false
    property string _lastOutputId: ""
    property int _lastVolumePercent: -1
    property bool _lastMuted: false

    function syncAudioBaseline() {
        if (!audio || !audio.available) {
            _audioInitialized = false;
            _lastOutputId = "";
            return;
        }
        _lastOutputId = audio.outputId;
        _lastVolumePercent = Math.round(audio.volume * 100);
        _lastMuted = audio.muted;
        _audioInitialized = true;
    }

    function handleAudioChange() {
        if (!audio || !audio.available || audio.switchingOutput) {
            if (!audio || !audio.available)
                _audioInitialized = false;
            return;
        }
        const percent = Math.round(audio.volume * 100);
        const muted = audio.muted;
        if (!_audioInitialized || audio.outputId !== _lastOutputId) {
            syncAudioBaseline();
            return;
        }
        if (percent === _lastVolumePercent && muted === _lastMuted)
            return;
        _lastVolumePercent = percent;
        _lastMuted = muted;
        if (!suppressAudio)
            show("volume", percent, muted);
    }

    function show(kind, percent, muted) {
        const normalizedKind = kind === "brightness" || kind === "microphone" ? kind : "volume";
        const parsedPercent = Number(percent);
        const numericPercent = Number.isFinite(parsedPercent) ? Math.round(parsedPercent) : 0;
        const normalizedMuted = muted === true || muted === "true" || muted === "1";

        const focused = Hyprland.focusedMonitor;
        const output = Quickshell.screens.find(screen => focused && screen.name === focused.name);
        if (output)
            root.screen = output;
        else if (!root.screen && Quickshell.screens.length > 0)
            root.screen = Quickshell.screens[0];

        content.kind = normalizedKind;
        content.percent = numericPercent;
        content.muted = normalizedMuted;
        if (normalizedKind === "volume" && audio && audio.available) {
            _lastVolumePercent = numericPercent;
            _lastMuted = normalizedMuted;
            _lastOutputId = audio.outputId;
            _audioInitialized = true;
        }

        closeAnimation.stop();
        if (!visible) {
            reveal = 0;
            visible = true;
        }
        openAnimation.restart();
        hideTimer.restart();
    }

    function hide() {
        if (!visible || closeAnimation.running)
            return;
        openAnimation.stop();
        closeAnimation.start();
    }

    visible: false
    color: "transparent"
    implicitWidth: content.implicitWidth + shadowMargin * 2
    implicitHeight: content.implicitHeight + shadowMargin * 2
    exclusionMode: ExclusionMode.Ignore
    exclusiveZone: 0
    mask: Region {}
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "dotfiles-osd"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    anchors {
        bottom: true
    }

    margins {
        bottom: 56
    }

    Component.onCompleted: syncAudioBaseline()

    Connections {
        target: root.audio
        function onAvailableChanged() {
            root.syncAudioBaseline();
        }
        function onOutputIdChanged() {
            root.syncAudioBaseline();
        }
        function onVolumeChanged() {
            root.handleAudioChange();
        }
        function onMutedChanged() {
            root.handleAudioChange();
        }
    }

    Timer {
        id: hideTimer
        interval: 1600
        onTriggered: root.hide()
    }

    NumberAnimation {
        id: openAnimation
        target: root
        property: "reveal"
        to: 1
        duration: Theme.animationNormal
        easing.type: Easing.OutCubic
    }

    NumberAnimation {
        id: closeAnimation
        target: root
        property: "reveal"
        to: 0
        duration: Theme.animationFast
        easing.type: Easing.OutCubic
        onFinished: root.visible = false
    }

    Item {
        anchors.fill: parent
        opacity: root.reveal
        scale: 0.96 + root.reveal * 0.04
        transform: Translate {
            y: 6 * (1 - root.reveal)
        }

        Repeater {
            model: 3
            Rectangle {
                required property int index
                anchors.fill: content
                anchors.margins: -(index + 1) * 3
                radius: Theme.menuRadius + (index + 1) * 3
                color: index === 0 ? Theme.shadow : "#18000000"
            }
        }

        OsdContent {
            id: content
            anchors.centerIn: parent
        }
    }
}
