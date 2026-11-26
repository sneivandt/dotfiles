import QtQuick
import QtTest
import Quickshell
import Quickshell.Services.Pipewire
import "../../../../symlinks/config/quickshell/state" as State
import "../../../../symlinks/config/quickshell/menus" as Menus

Item {
    Component {
        id: audioComponent
        State.AudioState {}
    }
    Component {
        id: volumeMenuComponent
        Menus.VolumeMenu {
            anchorItem: null
        }
    }
    Component {
        id: nodeComponent
        QtObject {
            property string name: "fixture-speaker"
            property string description: "Fixture speaker"
            property string nickname: ""
            property bool isSink: true
            property bool isStream: false
            property bool ready: true
            property QtObject audio: QtObject {
                property real volume: 0.5
                property bool muted: false
            }
        }
    }

    TestCase {
        name: "AudioState"

        function initTestCase() {
            verify(Quickshell.testMode === true, "Run with inert mocks; live audio services are forbidden.");
        }

        function init() {
            Pipewire.ready = false;
            Pipewire.defaultAudioSink = null;
            Pipewire.preferredDefaultAudioSink = null;
            Pipewire.nodes.values = [];
        }

        function cleanup() {
            init();
        }

        function node(properties) {
            return createTemporaryObject(nodeComponent, parent, properties || {});
        }

        function make(nodes) {
            Pipewire.nodes.values = nodes;
            Pipewire.defaultAudioSink = nodes[0] || null;
            Pipewire.ready = true;
            return createTemporaryObject(audioComponent, parent);
        }

        function test_reactive_volume_and_mute_follow_external_changes() {
            const sink = node();
            const audio = make([sink]);
            compare(audio.available, true);
            compare(audio.volume, 0.5);
            sink.audio.volume = 0.8;
            sink.audio.muted = true;
            compare(audio.volume, 0.8);
            compare(audio.muted, true);
            audio.setVolume(2);
            compare(sink.audio.volume, 1.5);
            audio.setVolume(-1);
            compare(sink.audio.volume, 0);
            audio.setVolume(NaN);
            compare(sink.audio.volume, 0);
            audio.toggleMuted();
            compare(sink.audio.muted, false);
        }

        function test_only_ready_device_outputs_are_selectable() {
            const speaker = node();
            const stream = node({
                name: "stream",
                isStream: true
            });
            const input = node({
                name: "input",
                isSink: false
            });
            const pending = node({
                name: "pending",
                ready: false
            });
            const audio = make([speaker, stream, input, pending]);
            compare(audio.outputs.length, 2);
            compare(audio.outputs[1].available, false);
            audio.selectOutput("pending");
            verify(audio.error.includes("no longer available"));
            compare(Pipewire.preferredDefaultAudioSink, null);
        }

        function test_output_switch_waits_for_server_and_blocks_writes() {
            const speaker = node();
            const headset = node({
                name: "headset",
                description: "Fixture headset"
            });
            const audio = make([speaker, headset]);
            audio.selectOutput("headset");
            compare(Pipewire.preferredDefaultAudioSink, headset);
            compare(audio.switchingOutput, true);
            compare(audio.outputId, speaker.name);
            audio.setVolume(1);
            audio.toggleMuted();
            compare(speaker.audio.volume, 0.5);
            compare(speaker.audio.muted, false);
            Pipewire.defaultAudioSink = headset;
            compare(audio.switchingOutput, false);
            compare(audio.outputName, "Fixture headset");
            audio.setVolume(0.7);
            compare(headset.audio.volume, 0.7);
            compare(speaker.audio.volume, 0.5);
        }

        function test_disconnected_target_and_service_loss_clear_pending_switch() {
            const speaker = node();
            const headset = node({
                name: "headset"
            });
            const audio = make([speaker, headset]);
            audio.selectOutput("headset");
            Pipewire.nodes.values = [speaker];
            compare(audio.switchingOutput, false);
            verify(audio.error.includes("disconnected"));
            audio.refresh();
            compare(audio.error, "");
            Pipewire.ready = false;
            compare(audio.available, false);
            compare(audio.volume, 0);
            verify(audio.error.includes("unavailable"));
            audio.setVolume(1);
            compare(speaker.audio.volume, 0.5);
            Pipewire.ready = true;
            compare(audio.available, true);
        }

        function test_missing_default_and_node_binding_are_unavailable() {
            const sink = node({
                ready: false
            });
            const audio = make([sink]);
            compare(audio.available, false);
            compare(audio.loading, true);
            sink.ready = true;
            compare(audio.available, true);
            Pipewire.defaultAudioSink = null;
            compare(audio.available, false);
            compare(audio.outputId, "");
        }

        function test_switch_timeout_keeps_actual_device_and_allows_retry() {
            const speaker = node();
            const headset = node({
                name: "headset"
            });
            const audio = make([speaker, headset]);
            audio.selectOutput("headset");
            const deadline = findChild(audio, "audioSwitchDeadline");
            deadline.stop();
            deadline.triggered();
            compare(audio.switchingOutput, false);
            compare(audio.outputId, speaker.name);
            verify(audio.error.includes("Could not switch"));
            audio.selectOutput("headset");
            compare(audio.switchingOutput, true);
            compare(audio.error, "");
        }

        function test_volume_menu_dismiss_button_only_shows_for_action_errors() {
            const speaker = node();
            const audio = createTemporaryObject(audioComponent, parent);
            const menu = createTemporaryObject(volumeMenuComponent, parent, { audio: audio });
            menu.visible = true;
            const dismissButton = findChild(menu, "volumeDismissErrorButton");
            verify(dismissButton !== null);

            // PipeWire unavailable is not a dismissible action error.
            compare(Pipewire.ready, false);
            verify(audio.error.includes("unavailable"));
            compare(audio.actionError, "");
            compare(dismissButton.visible, false);

            // Action errors show a Dismiss button that clears the error cleanly.
            Pipewire.nodes.values = [speaker];
            Pipewire.defaultAudioSink = speaker;
            Pipewire.ready = true;
            audio.selectOutput("missing-sink");
            verify(audio.actionError.includes("no longer available"));
            compare(dismissButton.visible, true);
            compare(dismissButton.label, "Dismiss");
            dismissButton.triggered();
            compare(audio.actionError, "");
            compare(audio.error, "");
            compare(dismissButton.visible, false);
        }
    }
}
