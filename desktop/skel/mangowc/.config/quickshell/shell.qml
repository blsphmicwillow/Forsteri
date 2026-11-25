// Forsteri OS -- Quickshell for MangoWC
//
// A floating bar with the emblem, a clock, tray, battery and a power button.
// MangoWC is dwl-derived and has no shell of its own, so everything above the
// compositor is Quickshell.
//
// This file is a starting point, deliberately small: one bar, no plugin API,
// no external binaries except systemctl. Run `quickshell -c` to log errors.

import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import Quickshell.Services.SystemTray
import Quickshell.Services.UPower

ShellRoot {
    PanelWindow {
        id: bar

        anchors {
            top: true
            left: true
            right: true
        }

        margins {
            top: 8
            left: 12
            right: 12
        }

        implicitHeight: 34
        color: "transparent"
        exclusiveZone: 34

        Rectangle {
            anchors.fill: parent
            radius: 14
            color: Qt.rgba(0.055, 0.055, 0.071, 0.82)
            border.width: 1
            border.color: Qt.rgba(0.949, 0.761, 0.306, 0.22)

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 14
                anchors.rightMargin: 12
                spacing: 14

                Text {
                    text: "\u2744 forsteri"
                    color: "#f2c14e"
                    font.pixelSize: 12
                    font.bold: true
                    font.family: "JetBrains Mono"
                }

                Text {
                    text: "mango"
                    color: "#9fb6c8"
                    font.pixelSize: 11
                    font.family: "JetBrains Mono"
                }

                Item { Layout.fillWidth: true }

                Text {
                    id: clock

                    color: "#f7f1e1"
                    font.pixelSize: 12
                    font.family: "JetBrains Mono"

                    Timer {
                        interval: 10000
                        running: true
                        repeat: true
                        triggeredOnStart: true
                        onTriggered: clock.text = Qt.formatDateTime(new Date(), "ddd dd MMM  hh:mm")
                    }
                }

                RowLayout {
                    spacing: 6

                    Repeater {
                        model: SystemTray.items

                        Image {
                            required property var modelData
                            source: modelData.icon
                            width: 16
                            height: 16
                            sourceSize.width: 16
                            sourceSize.height: 16
                        }
                    }
                }

                Text {
                    visible: UPower.displayDevice !== null
                    text: UPower.displayDevice
                        ? UPower.displayDevice.percentage.toFixed(0) + "%"
                        : ""
                    color: "#d7d2c4"
                    font.pixelSize: 12
                    font.family: "JetBrains Mono"
                }

                Text {
                    text: "\u23fb"
                    color: "#e79a86"
                    font.pixelSize: 14

                    MouseArea {
                        anchors.fill: parent
                        onClicked: Quickshell.execDetached(["systemctl", "poweroff"])
                    }
                }
            }
        }
    }
}
