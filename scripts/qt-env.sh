# shellcheck shell=bash
# Locate Qt 6 and export what cxx-qt-build expects. Sourced by dev.sh/build.sh.
#
# cxx-qt-build (qt-build-utils) finds Qt in exactly two ways: the QMAKE env var
# first, then a `qmake` on PATH. Everything here only serves to produce those,
# which is why crates/mailapp/build.rs stays free of OS branching.
#
# Also exports:
#   EXE_SUFFIX   ""  on Linux, ".exe" under MSYS2/Git Bash
#   QT_BIN_DIR   Qt's bin/ (windeployqt lives there)

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) qt_env_windows=1 ;;
    *) qt_env_windows=0 ;;
esac

if [ "$qt_env_windows" = 1 ]; then
    EXE_SUFFIX=".exe"
else
    EXE_SUFFIX=""
fi
export EXE_SUFFIX

# Newest Qt kit under the usual roots whose ABI matches the Rust host toolchain
# (an MSVC Rust target cannot link a MinGW Qt, or vice versa). Qt's installer
# puts nothing on PATH on Windows, hence the search.
_qt_find_windows_qmake() {
    local host kit root roots candidate found newest

    host="$(rustc -vV | awk '/^host:/ { print $2 }')"
    case "$host" in
        *-msvc) kit='msvc*_64' ;;
        *) kit='mingw*_64' ;;
    esac

    roots=""
    for root in "${QT_ROOT_DIR:-}" "${QTDIR:-}" /c/Qt /d/Qt \
                "$(cygpath "${USERPROFILE:-$HOME}" 2>/dev/null)/Qt"; do
        [ -n "$root" ] && [ -d "$root" ] && roots="$roots $root"
    done

    # Shell globbing, not `ls`: its output may be aliased or wrapped.
    # `sort -V` so 6.10 beats 6.9.
    found=""
    # shellcheck disable=SC2086
    for root in $roots; do
        for candidate in $root/6.*/$kit/bin/qmake.exe; do
            [ -f "$candidate" ] || continue
            newest="$(printf '%s\n%s\n' "$found" "$candidate" | sort -V | tail -n 1)"
            [ "$newest" = "$candidate" ] && found="$candidate"
        done
    done

    [ -n "$found" ] && printf '%s\n' "$found"
}

if [ -z "${QMAKE:-}" ]; then
    if command -v qmake >/dev/null 2>&1; then
        QMAKE="$(command -v qmake)"
    elif [ "$qt_env_windows" = 1 ]; then
        QMAKE="$(_qt_find_windows_qmake)"
        if [ -z "$QMAKE" ]; then
            cat >&2 <<'EOF'
No Qt 6 installation found (searched QT_ROOT_DIR, QTDIR, /c/Qt, /d/Qt,
$USERPROFILE/Qt for 6.*/<kit>/bin/qmake.exe).

Install Qt 6 with the modules this app links -- Quick, QuickControls2, Network,
WebEngineQuick -- in a kit matching the Rust host ABI (msvc*_64 for the default
x86_64-pc-windows-msvc). No admin rights needed; any user-writable directory
works:

    py -m pip install --user aqtinstall
    py -m aqt install-qt windows desktop <version> win64_msvc2022_64 \
        -m qtwebengine qtwebchannel qtpositioning -O D:/Qt

Or point QMAKE at an existing install:
    export QMAKE=/d/Qt/<version>/msvc2022_64/bin/qmake.exe
EOF
            exit 1
        fi
    elif [ -x /usr/lib/qt6/bin/qmake ]; then
        # Arch/Omarchy keeps qmake off PATH.
        QMAKE=/usr/lib/qt6/bin/qmake
    else
        echo "No Qt 6 found: set QMAKE or put qmake on PATH." >&2
        exit 1
    fi
fi

[ -x "$QMAKE" ] || { echo "QMAKE=$QMAKE is not executable." >&2; exit 1; }

QT_BIN_DIR="$(cd "$(dirname "$QMAKE")" && pwd)"
export QT_BIN_DIR

if [ "$qt_env_windows" = 1 ]; then
    # No rpath on Windows: Qt's DLLs are only found at runtime via PATH.
    case ":$PATH:" in
        *":$QT_BIN_DIR:"*) ;;
        *) PATH="$QT_BIN_DIR:$PATH" ;;
    esac
    export PATH
    # The build script is a native Windows process and cannot open an MSYS
    # path, so hand it a drive-letter path.
    QMAKE="$(cygpath -m "$QMAKE")"
fi

export QMAKE
export QT_VERSION_MAJOR="${QT_VERSION_MAJOR:-6}"

echo "==> Qt $("$QT_BIN_DIR/qmake$EXE_SUFFIX" -query QT_VERSION)  ($QMAKE)"
