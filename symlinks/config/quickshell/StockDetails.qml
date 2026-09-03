import QtQuick
import QtQuick.Layouts
import "Theme.js" as Theme

Rectangle {
    id: root

    required property var quote
    readonly property bool hasHistory: Boolean(quote.history && quote.history.length > 1)
    readonly property color accentColor: quote.yearChange >= 0 ? Theme.green : Theme.red
    readonly property color accentBackground: quote.yearChange >= 0 ? Theme.greenSoft : Theme.redSoft

    function price(value) {
        return Number(value).toLocaleString(Qt.locale("en_US"), "f", 2);
    }

    function percent(value) {
        const number = Number(value);
        return (number >= 0 ? "+" : "") + number.toFixed(2) + "%";
    }

    function periodLabel() {
        const start = Number(root.quote.historyStart || 0) * 1000;
        const yearThreshold = 330 * 24 * 60 * 60 * 1000;
        if (start <= 0 || Date.now() - start >= yearThreshold)
            return "1 YEAR";

        return "SINCE " + Qt.formatDate(new Date(start), "MMM d").toUpperCase();
    }

    implicitHeight: 134
    color: "transparent"

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.leftMargin: 11
        anchors.rightMargin: 11
        height: 1
        color: Theme.borderSubtle
    }

    Row {
        id: chartHeader

        anchors.left: parent.left
        anchors.top: parent.top
        anchors.leftMargin: 13
        anchors.topMargin: 10
        spacing: 8

        Text {
            text: root.periodLabel()
            color: Theme.mutedStrong
            font.family: Theme.font
            font.pixelSize: 9
            font.weight: Font.DemiBold
        }

        Text {
            text: root.hasHistory ? root.percent(root.quote.yearChange) : "LOADING"
            color: root.hasHistory ? root.accentColor : Theme.mutedStrong
            font.family: Theme.font
            font.pixelSize: 10
            font.weight: Font.DemiBold
        }
    }

    Canvas {
        id: chart

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: chartHeader.bottom
        anchors.bottom: rangeRow.top
        anchors.leftMargin: 13
        anchors.rightMargin: 13
        anchors.topMargin: 6
        anchors.bottomMargin: 8
        property var values: root.quote.history || []

        onValuesChanged: requestPaint()
        onWidthChanged: requestPaint()
        onHeightChanged: requestPaint()
        onPaint: {
            const context = getContext("2d");
            context.reset();
            if (values.length < 2 || width <= 0 || height <= 0)
                return;

            let low = Number(values[0]);
            let high = low;
            for (let i = 1; i < values.length; ++i) {
                low = Math.min(low, Number(values[i]));
                high = Math.max(high, Number(values[i]));
            }
            const spread = Math.max(high - low, Math.max(high * 0.01, 0.01));
            const xFor = index => index * width / (values.length - 1);
            const yFor = value => 3 + (high - Number(value)) / spread * (height - 6);

            context.strokeStyle = Theme.borderSubtle;
            context.lineWidth = 1;
            for (let line = 1; line < 3; ++line) {
                const y = Math.round(line * height / 3) + 0.5;
                context.beginPath();
                context.moveTo(0, y);
                context.lineTo(width, y);
                context.stroke();
            }

            context.beginPath();
            context.moveTo(0, height);
            for (let point = 0; point < values.length; ++point)
                context.lineTo(xFor(point), yFor(values[point]));

            context.lineTo(width, height);
            context.closePath();
            const fill = context.createLinearGradient(0, 0, 0, height);
            fill.addColorStop(0, root.accentBackground);
            fill.addColorStop(1, "transparent");
            context.fillStyle = fill;
            context.fill();

            context.beginPath();
            context.moveTo(0, yFor(values[0]));
            for (let index = 1; index < values.length; ++index)
                context.lineTo(xFor(index), yFor(values[index]));

            context.strokeStyle = root.accentColor;
            context.lineWidth = 2;
            context.lineJoin = "round";
            context.lineCap = "round";
            context.stroke();
        }
    }

    RowLayout {
        id: rangeRow

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.leftMargin: 13
        anchors.rightMargin: 13
        anchors.bottomMargin: 9

        Text {
            visible: root.hasHistory
            text: "52W LOW  " + root.quote.prefix + root.price(root.quote.yearLow)
            color: Theme.mutedStrong
            font.family: Theme.font
            font.pixelSize: 9
        }

        Item {
            Layout.fillWidth: true
        }

        Text {
            visible: root.hasHistory
            text: "52W HIGH  " + root.quote.prefix + root.price(root.quote.yearHigh)
            color: Theme.mutedStrong
            font.family: Theme.font
            font.pixelSize: 9
        }
    }
}
