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

# Setup temporary sandbox for mocks and fake environment
TMP_SANDBOX="$(mktemp -d 2>/dev/null || mktemp -d -t 'installer_sandbox')"
cleanup() {
    rm -rf "$TMP_SANDBOX"
}
trap cleanup EXIT INT TERM

TMP_BIN="${TMP_SANDBOX}/bin"
MOCK_PKG="${TMP_SANDBOX}/pkg"
MOCK_ARTIFACTS="${TMP_SANDBOX}/artifacts"
FAKE_HOME="${TMP_SANDBOX}/home"
FAKE_CONFIG="${TMP_SANDBOX}/config"

mkdir -p "$TMP_BIN" "$MOCK_PKG" "$MOCK_ARTIFACTS" "$FAKE_HOME" "$FAKE_CONFIG"

# 3. Create mock uname
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

# 3a. Non-Linux OS rejection
output=$(TEST_MOCK_OS="Darwin" PATH="${TMP_BIN}:$PATH" sh "$INSTALL_SH" 2>&1 || true)
if echo "$output" | grep -q "supports Linux only"; then
    echo "PASS: Non-Linux OS rejected properly"
else
    echo "FAIL: Expected Non-Linux OS rejection error, got: $output" >&2
    exit 1
fi

# 3b. Unsupported Architecture rejection
output=$(TEST_MOCK_OS="Linux" TEST_MOCK_ARCH="mips" PATH="${TMP_BIN}:$PATH" sh "$INSTALL_SH" 2>&1 || true)
if echo "$output" | grep -q "Unsupported CPU architecture: mips"; then
    echo "PASS: Unsupported architecture rejected properly"
else
    echo "FAIL: Expected unsupported architecture error, got: $output" >&2
    exit 1
fi

# 4. Setup mock 9router-mcp-web binary, tarball release, and mock curl
cat << 'EOF' > "${MOCK_PKG}/9router-mcp-web"
#!/bin/sh
TRACE="${MOCK_TRACE_FILE:-/tmp/trace.log}"
if [ "$1" = "configure" ]; then
    echo "CONFIGURE_INVOKED" >> "$TRACE"
    if read -r line; then
        echo "CONFIGURE_INPUT:$line" >> "$TRACE"
    fi
    exit 0
fi
echo "9router-mcp-web mock binary"
exit 0
EOF
chmod +x "${MOCK_PKG}/9router-mcp-web"

# Package tarball and create SHA256SUMS.txt
tar -czf "${MOCK_ARTIFACTS}/9router-mcp-web-linux-x86_64.tar.gz" -C "$MOCK_PKG" 9router-mcp-web
(
    cd "$MOCK_ARTIFACTS"
    sha256sum 9router-mcp-web-linux-x86_64.tar.gz > SHA256SUMS.txt
)

# Mock curl to return local artifacts without network access
cat << 'EOF' > "${TMP_BIN}/curl"
#!/bin/sh
OUT=""
while [ $# -gt 0 ]; do
    case "$1" in
        -o)
            OUT="$2"
            shift 2
            ;;
        *)
            shift
            ;;
    esac
done

if [ -n "$OUT" ]; then
    case "$OUT" in
        *.tar.gz)
            cp "${TEST_MOCK_ARTIFACTS}/9router-mcp-web-linux-x86_64.tar.gz" "$OUT"
            ;;
        *SHA256SUMS.txt)
            cp "${TEST_MOCK_ARTIFACTS}/SHA256SUMS.txt" "$OUT"
            ;;
        *)
            touch "$OUT"
            ;;
    esac
    exit 0
fi

echo '{"tag_name":"v0.1.0"}'
exit 0
EOF
chmod +x "${TMP_BIN}/curl"

# Mock id to ensure safe non-root install destination even if test runner runs as root
cat << 'EOF' > "${TMP_BIN}/id"
#!/bin/sh
if [ "$1" = "-u" ]; then
    echo 1000
else
    if [ -x /usr/bin/id ]; then
        exec /usr/bin/id "$@"
    elif [ -x /bin/id ]; then
        exec /bin/id "$@"
    else
        exec id "$@"
    fi
fi
EOF
chmod +x "${TMP_BIN}/id"

# 5. Piped installer without a TTY (exercising actual install.sh)
TRACE_NOTTY="${TMP_SANDBOX}/trace_notty.log"
NON_INTERACTIVE_OUTPUT=$(
    TEST_MOCK_ARTIFACTS="$MOCK_ARTIFACTS" \
    MOCK_TRACE_FILE="$TRACE_NOTTY" \
    HOME="$FAKE_HOME" \
    XDG_CONFIG_HOME="$FAKE_CONFIG" \
    VERSION="v0.1.0" \
    PATH="${TMP_BIN}:$PATH" \
    setsid sh -c "cat \"$INSTALL_SH\" | sh" </dev/null 2>&1
)

if echo "$NON_INTERACTIVE_OUTPUT" | grep -q "to create your initial configuration"; then
    echo "PASS: Non-interactive piped execution displays configure advice without hanging"
else
    echo "FAIL: Non-interactive piped execution failed, got: $NON_INTERACTIVE_OUTPUT" >&2
    exit 1
fi

if [ -f "$TRACE_NOTTY" ]; then
    echo "FAIL: Configure was unexpectedly invoked in non-interactive piped mode!" >&2
    exit 1
fi

if [ ! -x "${FAKE_HOME}/.local/bin/9router-mcp-web" ]; then
    echo "FAIL: 9router-mcp-web binary was not installed to expected location" >&2
    exit 1
fi
echo "PASS: Non-interactive piped installer completed installation without invoking configure"

# 6. Piped installer with a controlling TTY scenarios (curl ... | sh in interactive terminal)
python3 - "$INSTALL_SH" "$TMP_BIN" "$MOCK_ARTIFACTS" "$FAKE_HOME" "$FAKE_CONFIG" "$TMP_SANDBOX" << 'PY_EOF'
import os, sys, pty, select

install_sh = sys.argv[1]
tmp_bin = sys.argv[2]
mock_artifacts = sys.argv[3]
fake_home = sys.argv[4]
fake_config = sys.argv[5]
sandbox = sys.argv[6]

with open(install_sh, "rb") as f:
    install_script_bytes = f.read()

def run_piped_installer(trace_path, terminal_input, close_master_early=False):
    if os.path.exists(trace_path):
        os.remove(trace_path)
    installed_bin = os.path.join(fake_home, ".local", "bin", "9router-mcp-web")
    if os.path.exists(installed_bin):
        os.remove(installed_bin)

    env = os.environ.copy()
    env["PATH"] = f"{tmp_bin}:{env['PATH']}"
    env["TEST_MOCK_ARTIFACTS"] = mock_artifacts
    env["MOCK_TRACE_FILE"] = trace_path
    env["HOME"] = fake_home
    env["XDG_CONFIG_HOME"] = fake_config
    env["VERSION"] = "v0.1.0"

    master, slave = pty.openpty()
    pipe_r, pipe_w = os.pipe()

    pid = os.fork()
    if pid == 0:
        os.close(master)
        os.setsid()
        import fcntl, termios
        fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

        # stdin is the pipe (simulating curl ... | sh)
        os.dup2(pipe_r, 0)
        os.close(pipe_r)
        os.close(pipe_w)

        # stdout and stderr are attached to terminal
        os.dup2(slave, 1)
        os.dup2(slave, 2)
        os.close(slave)

        os.execvpe("sh", ["sh"], env)

    os.close(slave)
    os.close(pipe_r)

    # Feed install script to shell stdin pipe
    os.write(pipe_w, install_script_bytes)
    os.close(pipe_w)

    if terminal_input:
        os.write(master, terminal_input)

    if close_master_early:
        os.close(master)

    out = b""
    if not close_master_early:
        while True:
            r, _, _ = select.select([master], [], [], 5.0)
            if not r:
                break
            try:
                chunk = os.read(master, 1024)
                if not chunk:
                    break
                out += chunk
            except OSError:
                break
        os.close(master)

    _, status = os.waitpid(pid, 0)
    exit_code = os.waitstatus_to_exitcode(status) if hasattr(os, "waitstatus_to_exitcode") else (status >> 8)
    return exit_code, out.decode("utf-8", errors="replace"), installed_bin

# 6a. Accept configuration ('y') and provide input
trace_y = os.path.join(sandbox, "trace_y.log")
code_y, out_y, bin_y = run_piped_installer(trace_y, b"y\nsecret_user_input_from_tty\n")
if code_y != 0:
    sys.stderr.write(f"FAIL: 6a exited with non-zero status {code_y}\nOutput: {out_y}\n")
    sys.exit(1)
if "Would you like to configure 9router-mcp-web now? [Y/n]" not in out_y:
    sys.stderr.write(f"FAIL: Expected prompt not found in 6a output: {out_y}\n")
    sys.exit(1)
if not os.path.exists(trace_y):
    sys.stderr.write("FAIL: 6a configure child process was not invoked!\n")
    sys.exit(1)
with open(trace_y) as f:
    trace_content = f.read()
if "CONFIGURE_INVOKED" not in trace_content:
    sys.stderr.write(f"FAIL: 6a trace missing CONFIGURE_INVOKED: {trace_content}\n")
    sys.exit(1)
if "CONFIGURE_INPUT:secret_user_input_from_tty" not in trace_content:
    sys.stderr.write(f"FAIL: 6a trace missing terminal input: {trace_content}\n")
    sys.exit(1)
if not os.path.exists(bin_y):
    sys.stderr.write("FAIL: 6a binary was not installed\n")
    sys.exit(1)
print("PASS: Piped installer with controlling TTY offered configuration, invoked configure, and accepted input")

# 6b. Decline configuration ('n')
trace_n = os.path.join(sandbox, "trace_n.log")
code_n, out_n, bin_n = run_piped_installer(trace_n, b"n\n")
if code_n != 0:
    sys.stderr.write(f"FAIL: 6b exited with non-zero status {code_n}\nOutput: {out_n}\n")
    sys.exit(1)
if "Skipping configuration. You can run" not in out_n:
    sys.stderr.write(f"FAIL: Expected skipping message not found in 6b output: {out_n}\n")
    sys.exit(1)
if os.path.exists(trace_n):
    sys.stderr.write("FAIL: 6b configure child process was unexpectedly invoked!\n")
    sys.exit(1)
if not os.path.exists(bin_n):
    sys.stderr.write("FAIL: 6b binary was not installed\n")
    sys.exit(1)
print("PASS: Piped installer with controlling TTY declined configuration without invoking configure")

# 6c. EOF on controlling terminal (e.g. Ctrl-D)
trace_eof = os.path.join(sandbox, "trace_eof.log")
code_eof, out_eof, bin_eof = run_piped_installer(trace_eof, b"\x04")
if code_eof != 0:
    sys.stderr.write(f"FAIL: 6c exited with non-zero status {code_eof}\nOutput: {out_eof}\n")
    sys.exit(1)
if "Installation complete!" not in out_eof:
    sys.stderr.write(f"FAIL: Expected installation complete not found in 6c output: {out_eof}\n")
    sys.exit(1)
if os.path.exists(trace_eof):
    sys.stderr.write("FAIL: 6c configure child process was unexpectedly invoked on EOF!\n")
    sys.exit(1)
if not os.path.exists(bin_eof):
    sys.stderr.write("FAIL: 6c binary was not installed\n")
    sys.exit(1)
print("PASS: Piped installer with controlling TTY handled EOF without crashing or invoking configure")

PY_EOF

echo "All installer tests passed successfully!"
