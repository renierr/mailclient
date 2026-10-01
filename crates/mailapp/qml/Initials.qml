pragma Singleton

import QtQuick

// Avatar letters for a sender: the first letter of the name (or of the
// address when there is none) plus the first letter of the address's
// domain, so the many senders sharing one initial still tell apart. Mirrors
// Flutter `senderInitials()`; headless-tested by `tst_Initials.qml`.
QtObject {
    readonly property var secondLevel: ["co", "com", "net", "org", "ac", "gov", "edu"]

    function of(name, address) {
        var addr = address || "";
        var at = addr.lastIndexOf("@");
        var first = firstAlnum(name) || firstAlnum(addr) || "?";
        var second = at < 0 ? "" : firstAlnum(domainLabel(addr.substring(at + 1)));
        return first + second;
    }

    function firstAlnum(s) {
        var m = (s || "").match(/[a-zA-Z0-9]/);
        return m ? m[0].toUpperCase() : "";
    }

    // The name-bearing label of a domain: mail.example.co.uk -> example.
    function domainLabel(domain) {
        var labels = domain.replace(/[>\s]/g, "").toLowerCase().split(".").filter(l => l.length > 0);
        if (labels.length > 1)
            labels.pop();
        if (labels.length > 1 && secondLevel.indexOf(labels[labels.length - 1]) >= 0)
            labels.pop();
        return labels.length ? labels[labels.length - 1] : "";
    }
}
