import QtQuick
import QtTest
import "../../MediaPlayer.js" as MediaPlayer

TestCase {
    name: "MediaPlayer"

    QtObject {
        id: spotify
        readonly property string dbusName: "org.mpris.MediaPlayer2.spotify"
        property string playbackState: "Paused"
    }
    QtObject {
        id: browser
        readonly property string dbusName: "org.mpris.MediaPlayer2.browser"
        property string playbackState: "Paused"
    }
    property var players: [browser, spotify]
    readonly property var selected: MediaPlayer.select(players, "Playing", "Paused")

    function test_switches_as_playback_changes() {
        compare(selected, spotify);
        browser.playbackState = "Playing";
        compare(selected, browser);
        spotify.playbackState = "Playing";
        compare(selected, spotify);
        spotify.playbackState = "Stopped";
        browser.playbackState = "Paused";
        compare(selected, browser);
        browser.playbackState = "Stopped";
        compare(selected, null);
    }
}
