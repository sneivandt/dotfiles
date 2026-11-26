import QtQuick
import QtQuick.Layouts
import "../lib/Theme.js" as Theme

Item {
    id: root

    required property var quote
    readonly property bool hasHistory: Boolean(quote.history && quote.history.length > 1)
    readonly property bool hasRange: Number.isFinite(Number(quote.yearLow)) && Number.isFinite(Number(quote.yearHigh)) && Number(quote.yearHigh) > Number(quote.yearLow)
    readonly property real rangePosition: hasRange ? Math.max(0, Math.min(1, (Number(quote.price) - Number(quote.yearLow)) / (Number(quote.yearHigh) - Number(quote.yearLow)))) : 0
    readonly property color accentColor: Number(quote.yearChange) === 0 ? Theme.mutedStrong : (quote.yearChange > 0 ? Theme.green : Theme.red)
    readonly property color accentBackground: Number(quote.yearChange) === 0 ? "transparent" : (quote.yearChange > 0 ? Theme.greenSoft : Theme.redSoft)

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
            return "1 year";
        return "Since " + Qt.formatDate(new Date(start), "MMM d");
    }

    implicitHeight: detailsColumn.implicitHeight + Theme.spacing * 2

    ColumnLayout {
        id: detailsColumn

        x: Theme.spacing
        y: Theme.spacing
        width: Math.max(0, parent.width - Theme.spacing * 2)
        spacing: Theme.spacing

        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.spacing

            Text {
                Layout.fillWidth: true
                text: root.hasHistory ? root.periodLabel() : "Price history"
                color: Theme.mutedStrong
                font.family: Theme.font
                font.pixelSize: Theme.textSmall
            }

            Text {
                text: root.hasHistory ? root.percent(root.quote.yearChange) : "Unavailable"
                color: root.hasHistory ? root.accentColor : Theme.mutedStrong
                font.family: Theme.font
                font.pixelSize: Theme.textSmall
            }
        }

        Canvas {
            id: chart

            Layout.fillWidth: true
            Layout.preferredHeight: 88
            visible: root.hasHistory
            property var values: root.quote.history || []

            onValuesChanged: requestPaint()
            onWidthChanged: requestPaint()
            onHeightChanged: requestPaint()
            onVisibleChanged: {
                if (visible)
                    requestPaint();
            }
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
                const rightPad = 5;
                const plotWidth = Math.max(1, width - rightPad);
                const xFor = index => index * plotWidth / (values.length - 1);
                const yFor = value => 5 + (high - Number(value)) / spread * (height - 10);

                context.strokeStyle = Theme.borderSubtle;
                context.lineWidth = 1;
                for (let line = 1; line < 3; ++line) {
                    const y = Math.round(line * height / 3) + 0.5;
                    context.beginPath();
                    context.moveTo(0, y);
                    context.lineTo(plotWidth, y);
                    context.stroke();
                }

                context.beginPath();
                context.moveTo(0, height);
                for (let point = 0; point < values.length; ++point)
                    context.lineTo(xFor(point), yFor(values[point]));
                context.lineTo(plotWidth, height);
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

                const lastX = xFor(values.length - 1);
                const lastY = yFor(values[values.length - 1]);
                context.beginPath();
                context.arc(lastX, lastY, 5, 0, Math.PI * 2);
                context.fillStyle = root.accentBackground;
                context.fill();
                context.beginPath();
                context.arc(lastX, lastY, 2.75, 0, Math.PI * 2);
                context.fillStyle = root.accentColor;
                context.fill();
            }
        }

        Item {
            Layout.fillWidth: true
            visible: root.hasHistory && root.hasRange
            implicitHeight: 10

            Rectangle {
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                height: 4
                radius: 2
                color: Theme.borderSubtle

                Rectangle {
                    width: parent.width * root.rangePosition
                    height: parent.height
                    radius: parent.radius
                    color: root.accentColor
                    opacity: 0.75
                }
            }

            Rectangle {
                objectName: "stockRangeMarker"
                x: Math.max(0, Math.min(parent.width - width, root.rangePosition * (parent.width - width)))
                anchors.verticalCenter: parent.verticalCenter
                width: 8
                height: 8
                radius: width / 2
                color: Theme.foreground
                border.width: 2
                border.color: root.accentColor
            }
        }

        RowLayout {
            Layout.fillWidth: true
            visible: root.hasHistory
            spacing: Theme.spacing

            ColumnLayout {
                Layout.alignment: Qt.AlignLeft
                spacing: 2

                Text {
                    text: "52W low"
                    color: Theme.mutedStrong
                    font.family: Theme.font
                    font.pixelSize: Theme.textSmall
                }

                Text {
                    text: root.quote.prefix + root.price(root.quote.yearLow)
                    color: Theme.foreground
                    font.family: Theme.font
                    font.pixelSize: Theme.textSmall
                }
            }

            Item {
                Layout.fillWidth: true
            }

            ColumnLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 2

                Text {
                    Layout.alignment: Qt.AlignRight
                    text: "52W high"
                    color: Theme.mutedStrong
                    font.family: Theme.font
                    font.pixelSize: Theme.textSmall
                }

                Text {
                    Layout.alignment: Qt.AlignRight
                    text: root.quote.prefix + root.price(root.quote.yearHigh)
                    color: Theme.foreground
                    font.family: Theme.font
                    font.pixelSize: Theme.textSmall
                }
            }
        }
    }
}
