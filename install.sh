#!/bin/sh
set -e

# 9router-mcp-web installer for Linux
# https://github.com/alfa-reza/9router-web-mcp

REPO="alfa-reza/9router-web-mcp"
BINARY_NAME="9router-mcp-web"

echo "=== 9router-mcp-web Installer ==="

# 1. Check OS
OS="$(uname -s)"
if [ "$OS" != "Linux" ]; then
    echo "Error: 9router-mcp-web Version 1 currently supports Linux only." >&2
    echo "Detected OS: $OS" >&2
    exit 1
fi

# 2. Check Architecture
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)
        TARGET_ARCH="linux-x86_64"
        ;;
    aarch64|arm64)
        TARGET_ARCH="linux-aarch64"
        ;;
    *)
        echo "Error: Unsupported CPU architecture: $ARCH" >&2
        echo "Supported architectures: x86_64, aarch64" >&2
        exit 1
        ;;
esac

echo "Detected platform: Linux ($ARCH)"

# 3. Determine Version
if [ -z "$VERSION" ]; then
    echo "Finding latest release..."
    VERSION="$(curl -sSL -H "Accept: application/vnd.github+json" "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')"
    if [ -z "$VERSION" ]; then
        echo "Error: Could not determine latest release version from GitHub API." >&2
        echo "You can set VERSION manually: VERSION=v0.1.0 sh install.sh" >&2
        exit 1
    fi
fi

echo "Selected version: $VERSION"

# 4. Setup temporary directory
TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t '9router-install')"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

TARBALL="${BINARY_NAME}-${TARGET_ARCH}.tar.gz"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${TARBALL}"
CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${VERSION}/SHA256SUMS.txt"

# 5. Download tarball and checksums
echo "Downloading ${TARBALL}..."
curl -sSL -f -o "${TMP_DIR}/${TARBALL}" "$DOWNLOAD_URL" || {
    echo "Error: Failed to download ${TARBALL} from ${DOWNLOAD_URL}" >&2
    exit 1
}

echo "Downloading SHA256SUMS.txt..."
curl -sSL -f -o "${TMP_DIR}/SHA256SUMS.txt" "$CHECKSUMS_URL" || {
    echo "Error: Failed to download SHA256SUMS.txt from ${CHECKSUMS_URL}" >&2
    exit 1
}

# 6. Verify Checksum
echo "Verifying archive integrity..."
cd "$TMP_DIR"
if command -v sha256sum >/dev/null 2>&1; then
    sha256sum --check --ignore-missing SHA256SUMS.txt || {
        echo "Error: SHA-256 verification failed for ${TARBALL}!" >&2
        exit 1
    }
elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 --check --ignore-missing SHA256SUMS.txt || {
        echo "Error: SHA-256 verification failed for ${TARBALL}!" >&2
        exit 1
    }
else
    echo "Error: Neither sha256sum nor shasum is available to verify integrity." >&2
    exit 1
fi
echo "Integrity verification passed."

# 7. Extract archive
tar -xzf "$TARBALL"

if [ ! -f "${TMP_DIR}/${BINARY_NAME}" ]; then
    echo "Error: Binary ${BINARY_NAME} not found in archive!" >&2
    exit 1
fi

# 8. Determine destination
if [ "$(id -u)" -eq 0 ]; then
    INSTALL_DIR="/usr/local/bin"
else
    INSTALL_DIR="${HOME}/.local/bin"
fi

mkdir -p "$INSTALL_DIR"
INSTALL_PATH="${INSTALL_DIR}/${BINARY_NAME}"

echo "Installing to ${INSTALL_PATH}..."
mv -f "${TMP_DIR}/${BINARY_NAME}" "$INSTALL_PATH"
chmod 0755 "$INSTALL_PATH"

echo "Successfully installed ${BINARY_NAME} to ${INSTALL_PATH}!"

# 9. Path advice
case ":$PATH:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
        echo ""
        echo "NOTE: ${INSTALL_DIR} is not in your current PATH."
        echo "Add it to your PATH by adding this line to your shell profile (~/.bashrc or ~/.zshrc):"
        echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
        ;;
esac

# 10. Initial configuration handoff
CONFIG_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/9router-mcp-web"
CONFIG_FILE="${CONFIG_DIR}/config.toml"

if [ ! -f "$CONFIG_FILE" ]; then
    echo ""
    echo "No existing configuration found at ${CONFIG_FILE}."
    if [ -t 0 ]; then
        printf "Would you like to configure 9router-mcp-web now? [Y/n] "
        read -r configure_choice
        case "$configure_choice" in
            [nN][oO]|[nN])
                echo "Skipping configuration. You can run '${INSTALL_PATH} configure' at any time."
                ;;
            *)
                "${INSTALL_PATH}" configure
                ;;
        esac
    else
        echo "Run '${INSTALL_PATH} configure' to create your initial configuration."
    fi
else
    echo "Existing configuration found at ${CONFIG_FILE}."
fi

echo ""
echo "Installation complete!"
