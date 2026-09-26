import QtQuick
import QtQuick.Window

Window {
    id: win
    width: 1920; height: 1080; visible: true; color: "#0A0F24"
    title: "mailclient promo"

    property int frame: 0
    property int sceneLen: 300
    property int scene: Math.min(5, Math.floor(frame / sceneLen))
    property int lt: frame - scene * sceneLen
    property string outDir: "/tmp/opencode/promo2/frames"
    property int maxFrames: 1800
    property bool record: false

    Component.onCompleted: {
        let start = 0;
        for (let i = 0; i < Qt.application.arguments.length; i++) {
            const a = Qt.application.arguments[i];
            if (a.indexOf("--out=") === 0) outDir = a.slice(6);
            if (a.indexOf("--frames=") === 0) maxFrames = parseInt(a.slice(9), 10);
            if (a.indexOf("--start=") === 0) start = parseInt(a.slice(8), 10);
            if (a === "--record") record = true;
        }
        frame = start;
        maxFrames = start + maxFrames;
    }

    // ---------- easing ----------
    function clamp01(v) { return v < 0 ? 0 : v > 1 ? 1 : v; }
    function pr(x, a, b) { return clamp01((x - a) / (b - a)); }
    function easeOutCubic(t) { t = clamp01(t); return 1 - Math.pow(1 - t, 3); }
    function easeInOut(t) { t = clamp01(t); return t < 0.5 ? 4*t*t*t : 1 - Math.pow(-2*t+2, 3)/2; }
    function easeOutBack(t) { t = clamp01(t); const c = 1.70158; return 1 + (c+1)*Math.pow(t-1,3) + c*Math.pow(t-1,2); }
    function rise(delay, dur, dist) { return (1 - easeOutCubic(pr(lt, delay, delay + dur))) * dist; }
    function sceneAlpha() { return Math.min(easeOutCubic(pr(lt, 0, 14)), 1 - easeInOut(pr(lt, sceneLen - 18, sceneLen))); }

    function pad(n) { let s = "000" + n; return s.slice(-4); }

    function recordTick() {
        if (frame >= maxFrames) { Qt.quit(); return; }
        timer.stop();
        win.grabToImage(function(res) {
            res.save(outDir + "/" + pad(frame) + ".png");
            frame++;
            if (frame % 120 === 0) console.log("frame " + frame + "/" + maxFrames);
            if (frame >= maxFrames) Qt.quit(); else timer.start();
        });
    }
    Timer { id: timer; interval: 40; repeat: true; running: record; onTriggered: recordTick() }

    // ---------- background (static, calm) ----------
    Image { anchors.fill: parent; source: "assets/grain.png"; opacity: 0.5 }
    Image { anchors.fill: parent; source: "assets/vignette.png" }

    // ---------- shared chrome ----------
    Text { x: 80; y: 44; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 34; color: "#7DD3FC"; text: "mailclient" }
    Text {
        anchors.right: parent.right; anchors.rightMargin: 80; y: 48
        font.family: "Noto Sans"; font.pixelSize: 28; color: "#8A94AD"
        text: "0" + (scene + 1) + " / 06 · " + ["Meet mailclient","Effortless setup","A calm workspace","Blazing search","Compose and read","Get mailclient"][scene]
    }
    Rectangle { x: 0; y: 0; width: 1920; height: 5; color: "#ffffff"; opacity: 0.1 }
    Rectangle { x: 0; y: 0; width: 1920 * frame / 1799; height: 5; color: "#38BDF8" }

    TextMetrics { id: capMetrics; font.family: "Noto Sans"; font.pixelSize: 30
        text: ["Meet mailclient — email that respects your attention","Three accounts, one calm place — setup takes a minute","Sidebar, list, reader — responsive down to narrow screens","Full-text search across everything — even offline","Expressive writing, protective reading — by default","mailclient — inbox, minus the chaos"][scene] }
    Rectangle {
        x: 960 - capMetrics.advanceWidth / 2 - 42; y: 912; width: capMetrics.advanceWidth + 84; height: 66; radius: 33
        color: "#040814"; opacity: 0.6; border.color: "#ffffff"; border.width: 2
    }
    // (border opacity workaround: separate faint rect)
    Text { anchors.horizontalCenter: parent.horizontalCenter; y: 918; font.family: "Noto Sans"; font.pixelSize: 30; color: "white"; text: capMetrics.text }

    // ---------- scene 1 ----------
    Item {
        opacity: scene === 0 ? sceneAlpha() : 0; visible: opacity > 0
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 190 + rise(0,40,30); font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 30; font.letterSpacing: 6; color: "#7DD3FC"; text: "A DESKTOP MAIL CLIENT FOR LINUX" }
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 268 + rise(8,44,44); font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 104; color: "white"; text: "Inbox, minus" }
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 380 + rise(14,44,44); font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 104; color: "white"; text: "the chaos." }
        Rectangle { x: 960 - (300 + 240*easeOutCubic(pr(lt,40,90)))/2; y: 512; width: 300 + 240*easeOutCubic(pr(lt,40,90)); height: 7; radius: 3; color: "#38BDF8" }
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 540 + rise(40,40,26); font.family: "Noto Sans"; font.pixelSize: 36; color: "#C9D4EA"; text: "Rust core  ·  SQLite cache  ·  Qt Quick interface" }
        Repeater {
            model: 3
            delegate: Item {
                property real a: pr(lt, 70 + index*14, 110 + index*14)
                property real dy: rise(70 + index*14, 50, 120)
                opacity: a
                Rectangle { x: [470,790,1130][index]; y: 648 + dy; width: [300,340,300][index]; height: 218; radius: 22; color: "#ffffff"; opacity: 0.06 }
                Rectangle { x: [470,790,1130][index]; y: 648 + dy; width: [300,340,300][index]; height: 218; radius: 22; color: "#00000000"; border.color: "#ffffff"; border.width: 2; opacity: 0.2 }
                Rectangle { x: [470,790,1130][index] + 30; y: 688 + dy; width: [300,340,300][index] - 60; height: 22; radius: 9; color: index === 1 ? "#38BDF8" : "#ffffff"; opacity: index === 1 ? 1 : 0.27 }
                Rectangle { x: [470,790,1130][index] + 30; y: 728 + dy; width: ([300,340,300][index] - 60) * 0.72; height: 18; radius: 9; color: "#ffffff"; opacity: 0.16 }
                Rectangle { x: [470,790,1130][index] + 30; y: 760 + dy; width: ([300,340,300][index] - 60) * 0.55; height: 18; radius: 9; color: "#ffffff"; opacity: 0.16 }
            }
        }
        Rectangle { x: 505; y: 688 + rise(70,50,120); width: 15; height: 15; radius: 7; color: "#7DD3FC"; opacity: pr(lt,70,110) }
        Rectangle { x: 825; y: 728 + rise(84,50,120); width: 15; height: 15; radius: 7; color: "#7DD3FC"; opacity: pr(lt,84,124) }
        Text { anchors.right: parent.right; anchors.rightMargin: 24; y: 330; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 470; color: "#00000000"; style: Text.Outline; styleColor: "#ffffff"; opacity: 0.05; text: "01" }
    }

    // ---------- scene 2 ----------
    Item {
        opacity: scene === 1 ? sceneAlpha() : 0; visible: opacity > 0
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 128 + rise(0,40,30); font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 30; font.letterSpacing: 6; color: "#7DD3FC"; text: "MULTI-ACCOUNT IMAP + SMTP" }
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 206 + rise(8,44,44); font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 100; color: "white"; text: "Set up in seconds." }
        Repeater {
            model: ["Add an account — host, port, encryption","Folders map themselves — Inbox, Sent, Drafts, Archive","Passwords live in the OS keyring — never in the database"]
            delegate: Item {
                property real a: pr(lt, 30 + index*26, 60 + index*26)
                property real r: rise(30 + index*26, 40, 34)
                opacity: a
                Rectangle { x: 300; y: 408 + index*78 + r; width: 64; height: 64; radius: 14; color: "#38BDF8"; opacity: 0.27 }
                Text { x: 300; y: 408 + index*78 + r; width: 64; height: 64; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 34; color: "#7DD3FC"; text: (index+1) }
                Text { x: 392; y: 410 + index*78 + r; font.family: "Noto Sans"; font.pixelSize: 37; color: "white"; text: modelData }
            }
        }
        Repeater {
            model: [["you@example.com","#38BDF8"],["work account","#A78BFA"],["side project","#34D399"]]
            delegate: Item {
                property real t: easeOutBack(pr(lt, 130 + index*30, 170 + index*30))
                property real s: Math.max(0.01, t)
                opacity: pr(lt, 130 + index*30, 150 + index*30)
                transform: Scale { origin.x: 340 + index*426 + 193; origin.y: 770; xScale: s; yScale: s }
                Rectangle { x: 340 + index*426; y: 716; width: 386; height: 108; radius: 22; color: "#ffffff"; opacity: 0.06 }
                Rectangle { x: 340 + index*426; y: 716; width: 386; height: 108; radius: 22; color: modelData[1]; opacity: 0.18 }
                Text { x: 340 + index*426; y: 716; width: 386; height: 108; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 33; color: "white"; text: modelData[0] }
            }
        }
        Text { anchors.right: parent.right; anchors.rightMargin: 24; y: 330; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 470; color: "#00000000"; style: Text.Outline; styleColor: "#ffffff"; opacity: 0.05; text: "02" }
    }

    // ---------- scene 3 ----------
    Item {
        id: s3
        opacity: scene === 2 ? sceneAlpha() : 0; visible: opacity > 0
        property int idx: Math.floor(lt / 45) % 5
        property int prev: (idx + 4) % 5
        property real hy: 352 + prev*96 + ((352 + idx*96) - (352 + prev*96)) * easeInOut(pr(lt % 45, 0, 12))
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 96 + rise(0,40,30); font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 30; font.letterSpacing: 6; color: "#7DD3FC"; text: "SIDEBAR  ·  LIST  ·  READER" }
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 164 + rise(8,44,44); font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 92; color: "white"; text: "Three panes. Zero noise." }
        Item {
            opacity: pr(lt,30,80); y: rise(30,60,90)
            Rectangle { x: 150; y: 330; width: 320; height: 520; radius: 22; color: "#ffffff"; opacity: 0.06 }
            Rectangle { x: 150; y: 330; width: 320; height: 520; radius: 22; color: "#00000000"; border.color: "#ffffff"; border.width: 2; opacity: 0.2 }
            Text { x: 182; y: 348; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 28; color: "#7DD3FC"; text: "Folders" }
            Repeater {
                model: ["Inbox","Sent","Drafts","Archive","Trash"]
                delegate: Item {
                    Rectangle { x: 150; y: 396 + index*62; width: 320; height: 56; radius: 12; color: "#38BDF8"; opacity: index === 0 ? 0.23 : 0 }
                    Text { x: 182; y: 398 + index*62; font.family: "Noto Sans"; font.weight: index === 0 ? Font.Bold : Font.Normal; font.pixelSize: 29; color: index === 0 ? "white" : "#C9D4EA"; text: modelData }
                }
            }
            Rectangle { x: 392; y: 410; width: 46; height: 30; radius: 8; color: "#38BDF8"; opacity: 0.9 }
            Text { x: 392; y: 408; width: 46; height: 30; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 24; color: "white"; text: "12" }
            Rectangle { x: 392; y: 534; width: 38; height: 30; radius: 8; color: "#A78BFA"; opacity: 0.78 }
            Text { x: 392; y: 532; width: 38; height: 30; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 24; color: "white"; text: "3" }
            Rectangle { x: 494; y: 330; width: 640; height: 520; radius: 22; color: "#ffffff"; opacity: 0.06 }
            Rectangle { x: 494; y: 330; width: 640; height: 520; radius: 22; color: "#00000000"; border.color: "#ffffff"; border.width: 2; opacity: 0.2 }
            Rectangle { x: 494; y: s3.hy; width: 640; height: 88; radius: 14; color: "#38BDF8"; opacity: 0.26 }
            Rectangle { x: 494; y: s3.hy; width: 7; height: 88; color: "#38BDF8" }
            Repeater {
                model: ["Quarterly invoice attached","Re: launch plan Friday","Photos from the cabin trip","Your receipt from Example","Welcome to the beta group"]
                delegate: Item {
                    Rectangle { x: 526; y: 372 + index*96; width: 15; height: 15; radius: 7; color: "#38BDF8"; opacity: index < 2 ? 1 : 0.35 }
                    Text { x: 556; y: 362 + index*96; font.family: "Noto Sans"; font.weight: index < 2 ? Font.Bold : Font.Normal; font.pixelSize: 28; color: index === 4 ? "#8A94AD" : "white"; text: modelData }
                    Rectangle { x: 556; y: 404 + index*96; width: 300 - index*22; height: 15; radius: 7; color: "#ffffff"; opacity: 0.17 }
                }
            }
            Rectangle { x: 1158; y: 330; width: 612; height: 520; radius: 22; color: "#ffffff"; opacity: 0.06 }
            Rectangle { x: 1158; y: 330; width: 612; height: 520; radius: 22; color: "#00000000"; border.color: "#ffffff"; border.width: 2; opacity: 0.2 }
            Rectangle { x: 1194; y: 362; width: 380; height: 32; radius: 8; color: "#ffffff"; opacity: 0.35 }
            Rectangle { x: 1194; y: 408; width: 240; height: 20; radius: 8; color: "#ffffff"; opacity: 0.2 }
            Rectangle { x: 1194; y: 452; width: 520; height: 15; radius: 7; color: "#ffffff"; opacity: 0.16 }
            Rectangle { x: 1194; y: 486; width: 520; height: 15; radius: 7; color: "#ffffff"; opacity: 0.16 }
            Rectangle { x: 1194; y: 520; width: 520; height: 15; radius: 7; color: "#ffffff"; opacity: 0.16 }
            Rectangle { x: 1194; y: 554; width: 350; height: 15; radius: 7; color: "#ffffff"; opacity: 0.16 }
            Rectangle { x: 1194; y: 620; width: 540; height: 66; radius: 14; color: "#FBBF24"; opacity: 0.23 }
            Text { anchors.horizontalCenter: undefined; x: 1194; y: 626; width: 540; horizontalAlignment: Text.AlignHCenter; font.family: "Noto Sans"; font.pixelSize: 26; color: "#FBBF24"; text: "Remote images blocked — show once" }
            Rectangle { x: 1194; y: 716; width: 150; height: 46; radius: 12; color: "#38BDF8"; opacity: 0.47 }
            Rectangle { x: 1358; y: 716; width: 150; height: 46; radius: 12; color: "#ffffff"; opacity: 0.16 }
            Text { x: 1194; y: 716; width: 150; height: 46; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 26; color: "white"; text: "Open" }
            Text { x: 1358; y: 716; width: 150; height: 46; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter; font.family: "Noto Sans"; font.pixelSize: 26; color: "#C9D4EA"; text: "Save" }
        }
        Text { anchors.right: parent.right; anchors.rightMargin: 24; y: 330; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 470; color: "#00000000"; style: Text.Outline; styleColor: "#ffffff"; opacity: 0.05; text: "03" }
    }

    // ---------- scene 4 ----------
    Item {
        opacity: scene === 3 ? sceneAlpha() : 0; visible: opacity > 0
        property int n: Math.min(7, Math.floor(easeInOut(pr(lt,30,110)) * 8))
        property string shown: "invoice".slice(0, n)
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 128 + rise(0,40,30); font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 30; font.letterSpacing: 6; color: "#7DD3FC"; text: "OFFLINE-FIRST SQLITE CACHE" }
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 206 + rise(8,44,44); font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 100; color: "white"; text: "Find anything, instantly." }
        Rectangle { x: 460; y: 400; width: 1000; height: 108; radius: 22; color: "#ffffff"; opacity: 0.06 }
        Rectangle { x: 460; y: 400; width: 1000; height: 108; radius: 22; color: "#00000000"; border.color: "#ffffff"; border.width: 2; opacity: 0.25 }
        Text { x: 510; y: 404; width: 900; height: 100; verticalAlignment: Text.AlignVCenter; font.family: "Noto Sans"; font.pixelSize: 52; color: "white"; text: shown }
        Rectangle { x: 515 + shown.length * 30; y: 428; width: 5; height: 56; color: "#7DD3FC"; visible: (Math.floor(lt/15) % 2 === 0) && n < 7 }
        Item {
            opacity: pr(lt,120,140)
            Text { x: 480; y: 540; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 40; color: "#6EE7B7"; text: "128 hits · 0.02 s" }
            Repeater {
                model: ["Quarterly invoice attached — Today","Invoice #2418 — Tuesday","Re: invoice correction — Monday"]
                delegate: Item {
                    opacity: pr(lt, 135 + index*22, 159 + index*22)
                    y: rise(135 + index*22, 30, 30)
                    Rectangle { x: 460; y: 600 + index*84; width: 1000; height: 70; radius: 16; color: "#ffffff"; opacity: 0.09 }
                    Rectangle { x: 492; y: 622 + index*84; width: 13; height: 13; radius: 6; color: "#38BDF8" }
                    Text { x: 520; y: 608 + index*84; font.family: "Noto Sans"; font.pixelSize: 29; color: "white"; text: modelData }
                }
            }
        }
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 846 + rise(150,40,26); font.family: "Noto Sans"; font.pixelSize: 33; color: "#C9D4EA"; text: "Type 3 letters — the FTS index answers over subject, sender, body" }
        Text { anchors.right: parent.right; anchors.rightMargin: 24; y: 330; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 470; color: "#00000000"; style: Text.Outline; styleColor: "#ffffff"; opacity: 0.05; text: "04" }
    }

    // ---------- scene 5 ----------
    Item {
        opacity: scene === 4 ? sceneAlpha() : 0; visible: opacity > 0
        property real lx: -780 + 940 * easeOutCubic(pr(lt,20,70))
        property real rx: 1920 - 940 * easeOutCubic(pr(lt,40,90))
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 108 + rise(0,40,30); font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 30; font.letterSpacing: 6; color: "#7DD3FC"; text: "COMPOSE  ·  READ" }
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 178 + rise(8,44,44); font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 88; color: "white"; text: "Write and read with confidence." }
        Item {
            opacity: pr(lt,20,45); x: lx
            Rectangle { x: 160; y: 350; width: 780; height: 500; radius: 22; color: "#ffffff"; opacity: 0.06 }
            Rectangle { x: 160; y: 350; width: 780; height: 500; radius: 22; color: "#00000000"; border.color: "#ffffff"; border.width: 2; opacity: 0.2 }
            Text { x: 204; y: 376; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 30; color: "#7DD3FC"; text: "COMPOSE" }
            Repeater {
                model: ["Rich-text editor, attachments, drafts","Smart send format, plain twin optional","From-domain guard keeps SPF, DKIM","and DMARC aligned","Queued locally — sends even if","you close the window"]
                delegate: Item {
                    Rectangle { x: 204; y: 446 + index*56; width: 14; height: 14; radius: 7; color: index < 4 ? "#7DD3FC" : "#6EE7B7" }
                    Text { x: 234; y: 438 + index*56; font.family: "Noto Sans"; font.pixelSize: 30; color: "white"; text: modelData }
                }
            }
        }
        Item {
            opacity: pr(lt,40,65); x: rx
            Rectangle { x: 980; y: 350; width: 780; height: 500; radius: 22; color: "#ffffff"; opacity: 0.06 }
            Rectangle { x: 980; y: 350; width: 780; height: 500; radius: 22; color: "#00000000"; border.color: "#ffffff"; border.width: 2; opacity: 0.2 }
            Text { x: 1024; y: 376; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 30; color: "#6EE7B7"; text: "READ SAFELY" }
            Repeater {
                model: ["Sanitized HTML, remote images blocked","Link-verify dialog before opening","Reply-To shown inline — no surprises","Raw headers on demand","Attachments download on demand,","then stay cached offline"]
                delegate: Item {
                    Rectangle { x: 1024; y: 446 + index*56; width: 14; height: 14; radius: 7; color: index < 4 ? "#6EE7B7" : "#7DD3FC" }
                    Text { x: 1054; y: 438 + index*56; font.family: "Noto Sans"; font.pixelSize: 30; color: "white"; text: modelData }
                }
            }
        }
        Text { anchors.right: parent.right; anchors.rightMargin: 24; y: 330; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 470; color: "#00000000"; style: Text.Outline; styleColor: "#ffffff"; opacity: 0.05; text: "05" }
    }

    // ---------- scene 6 ----------
    Item {
        opacity: scene === 5 ? sceneAlpha() : 0; visible: opacity > 0
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 118 + rise(0,40,30); font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 30; font.letterSpacing: 6; color: "#7DD3FC"; text: "BACKGROUND SYNC + OMARCHY WIDGET" }
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 194 + rise(8,44,44); font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 100; color: "white"; text: "Quietly in sync." }
        Repeater {
            model: ["Startup, folder-open and background polling","Omarchy bar widget with unread badge","Headless --sync-once and --status JSON for scripts"]
            delegate: Item {
                opacity: pr(lt, 30 + index*24, 58 + index*24)
                y: rise(30 + index*24, 36, 30)
                Rectangle { x: 960 - 560; y: 392 + index*66; width: 14; height: 14; radius: 7; color: "#6EE7B7" }
                Text { x: 960 - 530; y: 384 + index*66; font.family: "Noto Sans"; font.pixelSize: 34; color: "white"; text: modelData }
            }
        }
        Item {
            opacity: pr(lt,120,160)
            Rectangle { x: 960 - 470; y: 600; width: 940; height: 120; radius: 60; color: "#34D399"; opacity: 0.10 + 0.06 * Math.sin(lt * 0.07) }
            Rectangle { x: 960 - 470; y: 600; width: 940; height: 120; radius: 60; color: "#00000000"; border.color: "#34D399"; border.width: 3; opacity: 0.85 }
            Text { anchors.horizontalCenter: parent.horizontalCenter; y: 622; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 44; color: "white"; text: "Free and open — run ./dev.sh to try it" }
        }
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 748 + rise(150,44,30); font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 110; color: "#7DD3FC"; text: "mailclient" }
        Text { anchors.horizontalCenter: parent.horizontalCenter; y: 836; font.family: "Noto Sans"; font.pixelSize: 32; color: "#C9D4EA"; text: "Rust  ·  Qt 6  ·  SQLite  ·  Omarchy first" }
        Text { anchors.right: parent.right; anchors.rightMargin: 24; y: 330; font.family: "Noto Sans"; font.weight: Font.Bold; font.pixelSize: 470; color: "#00000000"; style: Text.Outline; styleColor: "#ffffff"; opacity: 0.05; text: "06" }
    }
}
