#!/bin/sh
# gg installer for Linux and macOS (x86_64 / aarch64). For Windows see install.ps1.
#
#   curl -fsSL https://raw.githubusercontent.com/ChenezDigital/gg/main/install.sh | sh
#
# Environment variables:
#   GG_VERSION      release tag to install, e.g. v0.1.0 (default: latest)
#   GG_INSTALL_DIR  where to put the binary (default: ~/.local/bin)
#   GG_REPO         GitHub repository to download from (default: ChenezDigital/gg)
#   GG_BASE_URL     download base URL; overrides GG_REPO/GG_VERSION (mirrors, testing)
set -eu

REPO="${GG_REPO:-ChenezDigital/gg}"
VERSION="${GG_VERSION:-latest}"
INSTALL_DIR="${GG_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "gg-install: $*"; }
die() { printf '%s\n' "gg-install: error: $*" >&2; exit 1; }
has() { command -v "$1" >/dev/null 2>&1; }

detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$arch" in
        x86_64 | amd64) arch=x86_64 ;;
        aarch64 | arm64) arch=aarch64 ;;
        *) die "unsupported CPU architecture: $arch (supported: x86_64, aarch64)" ;;
    esac
    case "$os" in
        Linux) echo "$arch-unknown-linux-musl" ;;
        Darwin)
            # A shell running under Rosetta reports x86_64 on Apple silicon; prefer the native build.
            if [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
                arch=aarch64
            fi
            echo "$arch-apple-darwin"
            ;;
        MINGW* | MSYS* | CYGWIN*)
            die "on Windows run this in PowerShell instead: irm https://raw.githubusercontent.com/$REPO/main/install.ps1 | iex" ;;
        *) die "unsupported OS: $os (supported: Linux, macOS; Windows via install.ps1)" ;;
    esac
}

download() {
    if has curl; then
        curl --proto '=https' --tlsv1.2 -fsSL "$1" -o "$2"
    elif has wget; then
        wget --https-only -qO "$2" "$1"
    else
        die "curl or wget is required"
    fi
}

# Some URLs used in testing are plain http on localhost; allow those explicitly.
download_any() {
    case "$1" in
        https://*) download "$1" "$2" ;;
        http://127.0.0.1* | http://localhost*)
            if has curl; then curl -fsSL "$1" -o "$2"; else wget -qO "$2" "$1"; fi ;;
        *) die "refusing non-HTTPS URL: $1" ;;
    esac
}

verify() {
    # $1 = directory containing the archive and its .sha256 file, $2 = checksum file name
    if has sha256sum; then
        (cd "$1" && sha256sum -c "$2" >/dev/null)
    elif has shasum; then
        (cd "$1" && shasum -a 256 -c "$2" >/dev/null)
    else
        die "sha256sum or shasum is required to verify the download"
    fi
}

main() {
    target=$(detect_target)
    if [ -n "${GG_BASE_URL:-}" ]; then
        base="$GG_BASE_URL"
    elif [ "$VERSION" = latest ]; then
        base="https://github.com/$REPO/releases/latest/download"
    else
        base="https://github.com/$REPO/releases/download/$VERSION"
    fi
    archive="gg-$target.tar.gz"

    tmp=$(mktemp -d 2>/dev/null || mktemp -d -t gg)
    trap 'rm -rf "$tmp"' EXIT INT TERM

    say "downloading $archive ($VERSION)"
    download_any "$base/$archive" "$tmp/$archive" || die "download failed: $base/$archive"
    download_any "$base/$archive.sha256" "$tmp/$archive.sha256" || die "checksum download failed"
    verify "$tmp" "$archive.sha256" || die "checksum mismatch for $archive"
    say "checksum verified"

    tar -xzf "$tmp/$archive" -C "$tmp"
    mkdir -p "$INSTALL_DIR"
    if has install; then
        install -m 755 "$tmp/gg-$target/gg" "$INSTALL_DIR/gg"
    else
        cp "$tmp/gg-$target/gg" "$INSTALL_DIR/gg" && chmod 755 "$INSTALL_DIR/gg"
    fi
    say "installed $("$INSTALL_DIR/gg" --version) to $INSTALL_DIR/gg"

    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *) say "note: $INSTALL_DIR is not on your PATH; add it in your shell profile:"
           say "  export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
    esac
    say "next: run \`gg setup\` (shell completions: \`gg completions --help\`)"
}

main "$@"
