import QtQuick
import QtTest

import Mailclient

// Headless unit tests for the Initials singleton. Run via
// `scripts/qml-check.sh`.
TestCase {
    name: "Initials"

    function test_name_plus_domain() {
        compare(Initials.of("Alice", "alice@example.com"), "AE");
        compare(Initials.of("Alice", "news@mail.example.org"), "AE");
        compare(Initials.of("Alice", "a@shop.example.co.uk"), "AE");
        compare(Initials.of("", "bob@example.net"), "BE");
        compare(Initials.of("\"Carol\"", "c@example.com"), "CE");
    }

    function test_without_domain_one_letter() {
        compare(Initials.of("Alice", ""), "A");
        compare(Initials.of("Alice", "alice"), "A");
        compare(Initials.of("", ""), "?");
        compare(Initials.of(undefined, undefined), "?");
    }
}
