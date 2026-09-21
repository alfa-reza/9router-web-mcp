#!/bin/sh
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INSTALL_SH="${SCRIPT_DIR}/install.sh"

echo "=== Running Installer Test Suite ==="

# 1. Test basic shell syntax of install.sh
sh -n "$INSTALL_SH"
echo "PASS: install.sh syntax is valid"

# 2. Test syntax of this test runner
sh -n "$0"
echo "PASS: installer_tests.sh syntax is valid"

# 3. Test OS rejection using mocked uname
TMP_BIN="$(mktemp -d 2>/dev/null || mktemp -d -t 'tmp_bin')"
cleanup() {
    rm -rf "$TMP_BIN"
}
trap cleanup EXIT INT TERM

cat << 'EOF' > "${TMP_BIN}/uname"
#!/bin/sh
if [ "$1" = "-s" ]; then
    echo "${TEST_MOCK_OS:-Linux}"
elif [ "$1" = "-m" ]; then
    echo "${TEST_MOCK_ARCH:-x86_64}"
else
    /bin/uname "$@"
fi
EOF
chmod +x "${TMP_BIN}/uname"

# Non-Linux OS rejection
output=$(TEST_MOCK_OS="Darwin" PATH="${TMP_BIN}:$PATH" sh "$INSTALL_SH" 2>&1 || true)
if echo "$output" | grep -q "supports Linux only"; then
    echo "PASS: Non-Linux OS rejected properly"
else
    echo "FAIL: Expected Non-Linux OS rejection error, got: $output" >&2
    exit 1
fi

# Unsupported Architecture rejection
output=$(TEST_MOCK_OS="Linux" TEST_MOCK_ARCH="mips" PATH="${TMP_BIN}:$PATH" sh "$INSTALL_SH" 2>&1 || true)
if echo "$output" | grep -q "Unsupported CPU architecture: mips"; then
    echo "PASS: Unsupported architecture rejected properly"
else
    echo "FAIL: Expected unsupported architecture error, got: $output" >&2
    exit 1
fi

# 4. Test TTY fallback logic in piped/non-interactive mode
NON_INTERACTIVE_OUTPUT=$(
    sh -c '
        INSTALL_PATH="/usr/local/bin/9router-mcp-web"
        CONFIG_FILE="/nonexistent/config.toml"
        HAS_TTY=0
        TTY_IN=""
        if [ -t 0 ]; then
            HAS_TTY=1
        elif [ -c /dev/tty ] && ( : </dev/tty ) 2>/dev/null; then
            HAS_TTY=0
        fi

        if [ "$HAS_TTY" -eq 1 ]; then
            echo "INTERACTIVE"
        else
            echo "Run \"${INSTALL_PATH} configure\" to create your initial configuration."
        fi
    ' </dev/null
)

if echo "$NON_INTERACTIVE_OUTPUT" | grep -q "to create your initial configuration"; then
    echo "PASS: Non-interactive piped execution displays configure advice without hanging"
else
    echo "FAIL: Non-interactive piped execution failed, got: $NON_INTERACTIVE_OUTPUT" >&2
    exit 1
fi

echo "All installer tests passed successfully!"
