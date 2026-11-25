import QtQuick
import QtTest
import "../.." as Shell
import "../../Theme.js" as Theme

Item {
    width: 400
    height: 120

    Component {
        id: osdComponent
        Shell.OsdContent {}
    }

    TestCase {
        name: "OsdContent"
        when: windowShown

        function make(props) {
            const osd = createTemporaryObject(osdComponent, parent, props || {});
            verify(osd !== null);
            return osd;
        }

        function test_volume_states_and_amplification() {
            const osd = make({ kind: "volume", percent: 55, muted: false });
            compare(osd.labelText, "Volume");
            compare(osd.valueText, "55%");
            compare(osd.accentColor, Qt.color(Theme.blue));
            compare(osd.amplified, false);
            compare(findChild(osd, "osdNominalMark").visible, true);
            compare(findChild(osd, "osdAmplifiedZone").visible, true);
            verify(osd.fillFraction > 0.36 && osd.fillFraction < 0.37);

            osd.percent = 125;
            compare(osd.valueText, "125%");
            compare(osd.amplified, true);
            compare(osd.accentColor, Qt.color(Theme.purple));
            verify(osd.fillFraction > 0.83 && osd.fillFraction < 0.84);

            osd.percent = 220;
            compare(osd.clampedPercent, 150);
            compare(osd.valueText, "150%");
            compare(osd.fillFraction, 1);

            osd.muted = true;
            compare(osd.valueText, "Muted");
            compare(osd.fillFraction, 0);
            compare(osd.accentColor, Qt.color(Theme.mutedStrong));
            compare(osd.iconGlyph, "\uf6a9");
        }

        function test_microphone_and_brightness_states() {
            const mic = make({ kind: "microphone", percent: 80, muted: true });
            compare(mic.labelText, "Microphone");
            compare(mic.valueText, "Muted");
            compare(mic.iconGlyph, "\uf131");
            compare(mic.fillFraction, 0);
            compare(findChild(mic, "osdNominalMark").visible, false);

            mic.muted = false;
            compare(mic.valueText, "80%");
            compare(mic.iconGlyph, "\uf130");
            compare(mic.accentColor, Qt.color(Theme.blue));
            compare(mic.fillFraction, 0.8);

            const brightness = make({ kind: "brightness", percent: 140, muted: false });
            compare(brightness.labelText, "Brightness");
            compare(brightness.clampedPercent, 100);
            compare(brightness.valueText, "100%");
            compare(brightness.accentColor, Qt.color(Theme.yellow));
            compare(brightness.iconGlyph, "\uf185");
            compare(findChild(brightness, "osdNominalMark").visible, false);
            compare(findChild(brightness, "osdAmplifiedZone").visible, false);
        }
    }
}
