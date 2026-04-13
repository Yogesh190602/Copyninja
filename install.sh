#!/usr/bin/env bash
# install.sh — One-shot installer for CopyNinja (Rust edition)
# Run as your normal user (NOT root).
set -euo pipefail

GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; CYAN='\033[0;36m'; NC='\033[0m'
info()  { echo -e "${GREEN}[✓]${NC} $*"; }
warn()  { echo -e "${YELLOW}[!]${NC} $*"; }
error() { echo -e "${RED}[✗]${NC} $*"; exit 1; }
step()  { echo -e "${CYAN}[→]${NC} $*"; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY_NAME="copyninja"
INSTALL_DIR="$HOME/.local/bin"

# ── 0. Detect session ─────────────────────────────────────────────────────
SESSION_TYPE="${XDG_SESSION_TYPE:-unknown}"
DESKTOP="${XDG_CURRENT_DESKTOP:-unknown}"

# If XDG vars aren't set (SSH, TTY, etc.), detect from running processes
if [[ "$SESSION_TYPE" == "unknown" || "$SESSION_TYPE" == "tty" ]]; then
    if command -v loginctl &>/dev/null; then
        GRAPHICAL_SESSION=$(loginctl list-sessions --no-legend 2>/dev/null \
            | awk '{print $1}' \
            | while read -r sid; do
                stype=$(loginctl show-session "$sid" -p Type --value 2>/dev/null)
                if [[ "$stype" == "wayland" || "$stype" == "x11" ]]; then
                    echo "$stype"
                    break
                fi
            done)
        if [[ -n "${GRAPHICAL_SESSION:-}" ]]; then
            SESSION_TYPE="$GRAPHICAL_SESSION"
        fi
    fi
    if [[ "$SESSION_TYPE" == "unknown" || "$SESSION_TYPE" == "tty" ]]; then
        if pgrep -x "Hyprland|sway|mutter|kwin_wayland|weston" &>/dev/null; then
            SESSION_TYPE="wayland"
        elif pgrep -x "Xorg|Xwayland|i3|openbox|xfwm4" &>/dev/null; then
            SESSION_TYPE="x11"
        fi
    fi
fi

if [[ "$DESKTOP" == "unknown" ]]; then
    if pgrep -x "gnome-shell" &>/dev/null; then
        DESKTOP="GNOME"
    elif pgrep -x "Hyprland" &>/dev/null; then
        DESKTOP="Hyprland"
    elif pgrep -x "sway" &>/dev/null; then
        DESKTOP="sway"
    elif pgrep -x "i3" &>/dev/null; then
        DESKTOP="i3"
    elif pgrep -x "plasmashell" &>/dev/null; then
        DESKTOP="KDE"
    elif pgrep -x "xfce4-session" &>/dev/null; then
        DESKTOP="XFCE"
    fi
fi

info "Detected session: $SESSION_TYPE ($DESKTOP)"

if [[ "$SESSION_TYPE" != "wayland" && "$SESSION_TYPE" != "x11" ]]; then
    warn "Could not detect session type. Proceeding anyway — installing both Wayland and X11 tools."
    SESSION_TYPE="both"
fi

# ── 1. Check runtime dependencies ─────────────────────────────────────────
# The Rust binary has no Python/PyGObject dependency.
# Only external tools used at runtime need to be present.
step "Checking runtime dependencies…"

MISSING=()
PACKAGES_TO_INSTALL=()

# notify-send (optional, for future use)
command -v notify-send &>/dev/null || { MISSING+=("notify-send"); PACKAGES_TO_INSTALL+=("libnotify"); }

# xclip + xdotool always needed (fallback for GNOME Wayland via XWayland + X11)
command -v xclip   &>/dev/null || { MISSING+=("xclip");   PACKAGES_TO_INSTALL+=("xclip"); }
command -v xdotool &>/dev/null || { MISSING+=("xdotool"); PACKAGES_TO_INSTALL+=("xdotool"); }

if [[ "$SESSION_TYPE" == "wayland" || "$SESSION_TYPE" == "both" ]]; then
    command -v wl-paste &>/dev/null || { MISSING+=("wl-paste"); PACKAGES_TO_INSTALL+=("wl-clipboard"); }
    command -v wtype    &>/dev/null || { MISSING+=("wtype");    PACKAGES_TO_INSTALL+=("wtype"); }
fi

# Check GTK4 system library (needed by the Rust binary at runtime)
if ! pkg-config --exists gtk4 2>/dev/null; then
    MISSING+=("gtk4")
    PACKAGES_TO_INSTALL+=("gtk4")
fi

# Deduplicate
if [[ ${#PACKAGES_TO_INSTALL[@]} -gt 0 ]]; then
    PACKAGES_TO_INSTALL=($(printf '%s\n' "${PACKAGES_TO_INSTALL[@]}" | sort -u))
fi

if [[ ${#MISSING[@]} -gt 0 ]]; then
    warn "Missing: ${MISSING[*]}"
    echo "Packages needed: ${PACKAGES_TO_INSTALL[*]}"
    echo ""
    read -rp "Auto-install now? [y/N] " answer
    if [[ "$answer" =~ ^[Yy]$ ]]; then
        sudo pacman -S --needed --noconfirm "${PACKAGES_TO_INSTALL[@]}"
        info "Dependencies installed."
    else
        error "Please install missing dependencies manually, then re-run."
    fi
else
    info "All runtime dependencies found."
fi

# ── 2. Build the Rust binary ──────────────────────────────────────────────
step "Building CopyNinja (release mode)…"

if ! command -v cargo &>/dev/null; then
    error "Rust toolchain not found. Install via: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi

cd "$SCRIPT_DIR"
cargo build --release 2>&1 | tail -3
BUILT_BINARY="$SCRIPT_DIR/target/release/$BINARY_NAME"

if [[ ! -f "$BUILT_BINARY" ]]; then
    error "Build failed — binary not found at $BUILT_BINARY"
fi

info "Build complete: $(du -h "$BUILT_BINARY" | cut -f1) binary"

# ── 3. Install binary ─────────────────────────────────────────────────────
step "Installing binary to $INSTALL_DIR…"
mkdir -p "$INSTALL_DIR"
cp "$BUILT_BINARY" "$INSTALL_DIR/$BINARY_NAME"
chmod +x "$INSTALL_DIR/$BINARY_NAME"

# Clean up legacy Python scripts if present
if [[ -f "$INSTALL_DIR/clipdaemon.py" ]]; then
    rm -f "$INSTALL_DIR/clipdaemon.py"
    rm -f "$INSTALL_DIR/clippick.py"
    info "Removed legacy Python scripts."
fi

# ── 4. Install systemd user service ───────────────────────────────────────
step "Installing systemd user service…"
SYSTEMD_DIR="$HOME/.config/systemd/user"
mkdir -p "$SYSTEMD_DIR"

cat > "$SYSTEMD_DIR/copyninja.service" << EOF
[Unit]
Description=CopyNinja — Clipboard History Daemon
Documentation=https://github.com/Yogesh190602/CopyNinja
PartOf=graphical-session.target
After=graphical-session.target

[Service]
Type=simple
ExecStart=%h/.local/bin/copyninja daemon
Restart=on-failure
RestartSec=3s
Environment=RUST_LOG=info

[Install]
WantedBy=graphical-session.target
EOF

systemctl --user daemon-reload
systemctl --user enable copyninja.service
systemctl --user restart copyninja.service
info "Daemon started and enabled on login."

if [[ "$SESSION_TYPE" != "wayland" && "$SESSION_TYPE" != "x11" ]]; then
    info "Note: Daemon will auto-detect the display and start monitoring once a graphical session is available."
fi

# ── 4b. GNOME Wayland: create default config ──────────────────────────────
# On GNOME Wayland, Mutter does not expose the focused window class to
# external tools, so auto-detection of "am I pasting into a terminal?"
# silently fails and pastes end up as Ctrl+V, which terminals ignore.
# Create a default config forcing terminal paste mode — only if the user
# doesn't already have a config file (never overwrite).
CONFIG_DIR="$HOME/.config/copyninja"
CONFIG_FILE="$CONFIG_DIR/config.toml"

if [[ "$SESSION_TYPE" == "wayland" && "$DESKTOP" == *GNOME* ]]; then
    if [[ -f "$CONFIG_FILE" ]]; then
        info "Config already exists at $CONFIG_FILE — leaving it alone."
    else
        step "Creating default GNOME Wayland config…"
        mkdir -p "$CONFIG_DIR"
        cat > "$CONFIG_FILE" <<'EOF'
# CopyNinja config — created by install.sh for GNOME Wayland.
#
# On GNOME Wayland, Mutter does not expose the focused window class to
# external apps (org.gnome.Shell.Introspect is locked down, xdotool
# returns "(null)" for native Wayland windows). This means paste_mode
# cannot auto-detect whether the focused window is a terminal, so it
# would silently default to Ctrl+V — which terminals ignore.
#
# "terminal" forces Ctrl+Shift+V, which works in all terminals and in
# most browsers (as paste-without-formatting). It does NOT work in GTK
# text fields, VS Code, or LibreOffice — change to "normal" if you
# mainly paste into those apps instead. Images always use Ctrl+V
# regardless of this setting.
paste_mode = "terminal"
EOF
        info "Wrote $CONFIG_FILE (paste_mode = \"terminal\")"
        warn "If you mainly paste into text fields (not terminals),"
        warn "  edit $CONFIG_FILE and change paste_mode to \"normal\"."
    fi
fi

# ── 5. Hotkey setup (DE-specific) ─────────────────────────────────────────
PICK_CMD="$INSTALL_DIR/$BINARY_NAME pick"
COPYNINJA_MARKER="# CopyNinja keybinding"

setup_gnome_keybinding() {
    if ! command -v gsettings &>/dev/null; then
        warn "gsettings not found — skipping GNOME keybinding setup."
        return
    fi
    step "Setting Super+Shift+V keybinding (GNOME)…"
    CUSTOM_PATH="/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings"

    EXISTING=$(gsettings get org.gnome.settings-daemon.plugins.media-keys custom-keybindings 2>/dev/null || echo "@as []")
    FOUND_SLOT=""
    for path_entry in $(echo "$EXISTING" | tr -d "[]',"); do
        slot_name=$(gsettings get "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:${path_entry}" name 2>/dev/null)
        if [[ "$slot_name" == "'Clipboard History'" ]]; then
            FOUND_SLOT="$path_entry"
            break
        fi
    done

    if [[ -n "$FOUND_SLOT" ]]; then
        NEW_PATH="$FOUND_SLOT"
    else
        SLOT=0
        while echo "$EXISTING" | grep -q "custom${SLOT}/" 2>/dev/null; do
            SLOT=$((SLOT + 1))
        done
        NEW_PATH="${CUSTOM_PATH}/custom${SLOT}/"

        if [[ "$EXISTING" == "@as []" ]]; then
            NEW_LIST="['${NEW_PATH}']"
        else
            NEW_LIST=$(echo "$EXISTING" | sed "s|]|, '${NEW_PATH}']|")
        fi
        gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings "$NEW_LIST"
    fi

    gsettings set "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:${NEW_PATH}" name 'Clipboard History'
    gsettings set "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:${NEW_PATH}" command "$PICK_CMD"
    gsettings set "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:${NEW_PATH}" binding '<Shift><Super>v'
    info "Keybinding set: Super+Shift+V → copyninja pick"
}

setup_wm_keybinding() {
    local config_file="$1"
    local bind_line="$2"

    if [[ ! -f "$config_file" ]]; then
        warn "Config file not found: $config_file — skipping keybinding setup."
        echo "  Add this line manually: $bind_line"
        return
    fi

    if grep -qF "$COPYNINJA_MARKER" "$config_file" 2>/dev/null; then
        # Update existing keybinding to point to the Rust binary
        sed -i "/$COPYNINJA_MARKER/{N;d;}" "$config_file"
        info "Updating existing keybinding in $config_file"
    fi

    echo "" >> "$config_file"
    echo "$COPYNINJA_MARKER" >> "$config_file"
    echo "$bind_line" >> "$config_file"
    info "Keybinding added to $config_file"
}

COPYNINJA_RULES_MARKER="# CopyNinja window rules"

setup_wm_window_rules() {
    local config_file="$1"
    shift
    local rules=("$@")

    if [[ ! -f "$config_file" ]]; then
        return
    fi

    # Avoid duplicates
    if grep -qF "$COPYNINJA_RULES_MARKER" "$config_file" 2>/dev/null; then
        info "Window rules already present in $config_file"
        return
    fi

    echo "" >> "$config_file"
    echo "$COPYNINJA_RULES_MARKER" >> "$config_file"
    for rule in "${rules[@]}"; do
        echo "$rule" >> "$config_file"
    done
    info "Window rules added to $config_file"
}

case "$DESKTOP" in
    *GNOME*)
        setup_gnome_keybinding
        ;;
    *Hyprland*|*hyprland*)
        setup_wm_keybinding \
            "$HOME/.config/hypr/hyprland.conf" \
            "bind = SUPER SHIFT, V, exec, $PICK_CMD"
        ;;
    *sway*|*Sway*)
        setup_wm_keybinding \
            "$HOME/.config/sway/config" \
            "bindsym Mod4+Shift+v exec $PICK_CMD"
        ;;
    *i3*|*I3*)
        setup_wm_keybinding \
            "$HOME/.config/i3/config" \
            "bindsym Mod4+Shift+v exec $PICK_CMD"
        ;;
    *)
        warn "Automatic keybinding setup not supported for '$DESKTOP'."
        echo "  Please bind Super+Shift+V to this command manually:"
        echo "  $PICK_CMD"
        ;;
esac

# ── 6. Window rules (DE-specific) ────────────────────────────────────────
case "$DESKTOP" in
    *Hyprland*|*hyprland*)
        setup_wm_window_rules \
            "$HOME/.config/hypr/hyprland.conf" \
            'windowrulev2 = float, class:^(com.copyninja.picker)$' \
            'windowrulev2 = dimaround, class:^(com.copyninja.picker)$' \
            'windowrulev2 = stayfocused, class:^(com.copyninja.picker)$' \
            'windowrulev2 = noborder, class:^(com.copyninja.picker)$'
        ;;
    *sway*|*Sway*)
        setup_wm_window_rules \
            "$HOME/.config/sway/config" \
            'for_window [app_id="com.copyninja.picker"] floating enable, border none'
        ;;
    *i3*|*I3*)
        setup_wm_window_rules \
            "$HOME/.config/i3/config" \
            'for_window [class="com.copyninja.picker"] floating enable, border none'
        ;;
esac

# ── Done ──────────────────────────────────────────────────────────────────
echo ""
info "Installation complete!"
echo ""
echo "  Binary:         $INSTALL_DIR/$BINARY_NAME ($(du -h "$INSTALL_DIR/$BINARY_NAME" | cut -f1))"
echo "  Daemon status:  systemctl --user status copyninja"
echo "  Live logs:      journalctl --user -u copyninja -f"
echo "  History file:   ~/.clipboard_history.json"
echo ""
echo "  Usage:"
echo "    - Copy text normally, it will be saved automatically"
echo "    - Press Super+Shift+V to open picker"
echo "    - Click on any entry to copy and auto-paste"
echo ""
info "Everything is ready — no logout needed. Enjoy CopyNinja!"
