import QtQuick
import "../lib/Theme.js" as Theme

Text {
    textFormat: Text.PlainText
    color: Theme.mutedStrong
    font.family: Theme.font
    font.pixelSize: 11
    font.weight: Font.DemiBold
    font.letterSpacing: 0.8
    font.capitalization: Font.AllUppercase
    topPadding: 4
    elide: Text.ElideRight
}
