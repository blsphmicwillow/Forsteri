// Forsteri OS -- SDDM login
//
// The supplied lockup sits at the top of the background; username, password
// and the gold sign-in button form a single column below it. Sessions are
// picked from the session model so Hyprland, MangoWC, GNOME and Plasma all
// appear in the same list.

import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root

    width: 1920
    height: 1080
    color: "#0e0e12"

    property int sessionIndex: sessionModel.lastIndex

    Image {
        anchors.fill: parent
        source: config.background
        fillMode: Image.PreserveAspectCrop
    }

    ColumnLayout {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 84
        spacing: 14
        width: 320

        TextField {
            id: userField

            Layout.fillWidth: true
            placeholderText: qsTr("Username")
            text: userModel.lastUser
            font.pixelSize: 14
            color: "#f7f1e1"
            placeholderTextColor: "#9fb6c8"
            background: Rectangle {
                radius: 8
                color: Qt.rgba(0.10, 0.10, 0.13, 0.85)
                border.width: 1
                border.color: userField.activeFocus ? "#f2c14e" : "#33333f"
            }
            onAccepted: passwordField.forceActiveFocus()
        }

        TextField {
            id: passwordField

            Layout.fillWidth: true
            placeholderText: qsTr("Password")
            echoMode: TextInput.Password
            font.pixelSize: 14
            color: "#f7f1e1"
            placeholderTextColor: "#9fb6c8"
            background: Rectangle {
                radius: 8
                color: Qt.rgba(0.10, 0.10, 0.13, 0.85)
                border.width: 1
                border.color: passwordField.activeFocus ? "#f2c14e" : "#33333f"
            }
            onAccepted: root.login()
        }

        Button {
            Layout.fillWidth: true
            Layout.topMargin: 6
            text: qsTr("Sign in")
            font.pixelSize: 14

            contentItem: Text {
                text: parent.text
                color: "#14140f"
                font: parent.font
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
            }

            background: Rectangle {
                radius: 8
                color: parent.down ? "#d8a93f" : "#f2c14e"
            }

            onClicked: root.login()
        }

        Text {
            id: message

            Layout.alignment: Qt.AlignHCenter
            text: ""
            color: "#e79a86"
            font.pixelSize: 12
        }
    }

    Connections {
        target: sddm
        function onLoginFailed() {
            message.text = qsTr("Wrong username or password")
            passwordField.text = ""
            passwordField.forceActiveFocus()
        }
    }

    Timer {
        interval: 100
        running: true
        repeat: false
        onTriggered: passwordField.forceActiveFocus()
    }

    function login() {
        if (userField.text.length === 0) {
            message.text = qsTr("Enter a username")
            return
        }
        message.text = ""
        sddm.login(userField.text, passwordField.text, root.sessionIndex)
    }
}
