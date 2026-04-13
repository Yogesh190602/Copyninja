#!/usr/bin/env bash
# doctor.sh — CopyNinja installation health check.
# Run with: ./doctor.sh
# Safe to run any time. Non-destructive. Reports PASS/FAIL for each check
# and tells you exactly how to fix any failure.

GREEN='\033[0;32m'; RED='\033[0;31m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'

FAIL_COUNT=0
WARN_COUNT=0

pass()  { echo -e "  ${GREEN}[OK]${NC}  $1"; }
fail()  { echo -e "  ${RED}[!!]${NC}  $1"; echo -e "        ${YELLOW}fix:${NC} $2"; FAIL_COUNT=$((FAIL_COUNT + 1)); }
warn()  { echo -e "  ${YELLOW}[??]${NC}  $1"; echo -e "        ${YELLOW}note:${NC} $2"; WARN_COUNT=$((WARN_COUNT + 1)); }
check() { if eval "$2"; then pass "$1"; else fail "$1" "$3"; fi; }

echo ""
echo -e "${CYAN}=== CopyNinja install health check ===${NC}"
echo ""
echo -e "${CYAN}── Binary & service ──${NC}"

check "binary at ~/.local/bin/copyninja" \
      "[ -x \"$HOME/.local/bin/copyninja\" ]" \
      "re-run ./install.sh"

check "daemon active (systemctl --user)" \
      "systemctl --user is-active copyninja.service >/dev/null" \
      "systemctl --user start copyninja.service"

check "daemon enabled at login" \
      "systemctl --user is-enabled copyninja.service >/dev/null" \
      "systemctl --user enable copyninja.service"

echo ""
echo -e "${CYAN}── Config & keybinding ──${NC}"

SESSION="${XDG_SESSION_TYPE:-unknown}"
DESKTOP="${XDG_CURRENT_DESKTOP:-unknown}"
echo "  session: $SESSION ($DESKTOP)"

if [[ "$SESSION" == "wayland" && "$DESKTOP" == *GNOME* ]]; then
    check "config at ~/.config/copyninja/config.toml" \
          "[ -f \"$HOME/.config/copyninja/config.toml\" ]" \
          "re-run ./install.sh (required on GNOME Wayland for terminal paste)"
    if [ -f "$HOME/.config/copyninja/config.toml" ]; then
        MODE=$(grep -oE 'paste_mode\s*=\s*"[a-z]+"' "$HOME/.config/copyninja/config.toml" 2>/dev/null | head -1)
        [ -n "$MODE" ] && echo "        current: $MODE"
    fi
fi

if command -v gsettings &>/dev/null && [[ "$DESKTOP" == *GNOME* ]]; then
    # Custom keybindings live under dynamic dconf paths — we have to query each one.
    FOUND_KEYBIND=""
    KEYBIND_PATHS=$(gsettings get org.gnome.settings-daemon.plugins.media-keys custom-keybindings 2>/dev/null | tr -d "[]',")
    for path in $KEYBIND_PATHS; do
        cmd=$(gsettings get "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:${path}" command 2>/dev/null)
        if [[ "$cmd" == *"copyninja"* ]]; then
            FOUND_KEYBIND="$path"
            break
        fi
    done
    if [[ -n "$FOUND_KEYBIND" ]]; then
        BIND=$(gsettings get "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:${FOUND_KEYBIND}" binding 2>/dev/null | tr -d "'")
        pass "GNOME keybinding registered ($BIND)"
    else
        warn "GNOME keybinding not found" "re-run ./install.sh or bind manually"
    fi
fi

echo ""
echo -e "${CYAN}── Auto-paste prerequisites (Wayland) ──${NC}"

if [[ "$SESSION" == "wayland" ]]; then
    check "ydotoold running (user service)" \
          "systemctl --user is-active ydotool.service >/dev/null" \
          "systemctl --user enable --now ydotool.service"

    if id -nG | tr ' ' '\n' | grep -qx input; then
        pass "user in 'input' group"
    else
        fail "user NOT in 'input' group" \
             "sudo usermod -aG input \$USER  →  then LOGOUT+LOGIN (required)"
    fi

    check "/dev/uinput exists" \
          "[ -e /dev/uinput ]" \
          "sudo modprobe uinput && echo uinput | sudo tee /etc/modules-load.d/uinput.conf"
else
    echo "  skipped (not on Wayland)"
fi

echo ""
echo -e "${CYAN}── Required tools ──${NC}"

check "wl-paste (Wayland clipboard)"    "command -v wl-paste &>/dev/null" "sudo pacman -S wl-clipboard"
check "xclip (X11 clipboard fallback)"  "command -v xclip &>/dev/null"    "sudo pacman -S xclip"
check "wtype (wlroots Wayland paste)"   "command -v wtype &>/dev/null"    "sudo pacman -S wtype"
check "xdotool (X11 paste)"             "command -v xdotool &>/dev/null"  "sudo pacman -S xdotool"
check "ydotool (Wayland paste via uinput)" "command -v ydotool &>/dev/null" "sudo pacman -S ydotool"

echo ""
echo -e "${CYAN}── Runtime status ──${NC}"

if command -v "$HOME/.local/bin/copyninja" &>/dev/null; then
    VERSION=$("$HOME/.local/bin/copyninja" --version 2>&1 || echo "FAILED")
    echo "  version: $VERSION"
fi

if [ -f "$HOME/.clipboard_history.json" ]; then
    if command -v python3 &>/dev/null; then
        ENTRY_COUNT=$(python3 -c "import json; print(len(json.load(open('$HOME/.clipboard_history.json'))))" 2>/dev/null || echo "?")
        echo "  clipboard entries captured: $ENTRY_COUNT"
    else
        SIZE=$(stat -c%s "$HOME/.clipboard_history.json" 2>/dev/null)
        echo "  history file size: ${SIZE} bytes"
    fi
else
    warn "no history file yet" "copy some text; it should appear within 1 second"
fi

echo ""
echo -e "${CYAN}── Recent daemon warnings/errors ──${NC}"
RECENT=$(journalctl --user -u copyninja --since "1 hour ago" --no-pager 2>/dev/null | grep -E 'WARN|ERROR' | tail -5)
if [ -z "$RECENT" ]; then
    echo "  (none in the last hour)"
else
    echo "$RECENT" | sed 's/^/  /'
fi

echo ""
echo -e "${CYAN}=== Summary ===${NC}"
if [[ $FAIL_COUNT -eq 0 && $WARN_COUNT -eq 0 ]]; then
    echo -e "  ${GREEN}All checks passed. CopyNinja is healthy.${NC}"
elif [[ $FAIL_COUNT -eq 0 ]]; then
    echo -e "  ${YELLOW}$WARN_COUNT warning(s), no failures. Probably fine.${NC}"
else
    echo -e "  ${RED}$FAIL_COUNT failure(s), $WARN_COUNT warning(s). Fix the failures above.${NC}"
    echo ""
    echo "  After fixing group membership or enabling ydotool, you MUST"
    echo "  log out and back in (or reboot) for the change to apply."
fi
echo ""
