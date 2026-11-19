#!/bin/bash
# Installs a runtime that is not published on npm from its official download,
# pinned by version, and checks each download's SHA-256 before it is run or
# unpacked.
#   install_pinned.sh <grok|antigravity|hermes> <package directory> <python>
# Grok and Antigravity land in <package directory>/bin. Hermes runs its official
# installer at a pinned commit into $HOME, as a user's install would.
set -euo pipefail
runtime=${1:?runtime}
pkg=${2:?package directory}
python=${3:?python}
platform="$(uname -s)-$(uname -m)"
hermes_commit=ae2a81ccc00e9b7c06301523e5bb09608397149c

fetch() {
    url=$1
    sum=$2
    destination=$3
    curl -fsSL --retry 3 --proto '=https' -o "$destination.part" "$url"
    "$python" -I -c 'import hashlib, sys
digest = hashlib.sha256(open(sys.argv[1], "rb").read()).hexdigest()
if digest != sys.argv[2]:
    sys.exit("%s: sha256 %s, pinned %s" % (sys.argv[1], digest, sys.argv[2]))' "$destination.part" "$sum"
    mv "$destination.part" "$destination"
}

mkdir -p "$pkg/bin"
case "$runtime-$platform" in
    # x.ai publishes no checksum: these are the hashes taken at the first download.
    grok-Darwin-arm64)
        fetch https://x.ai/cli/grok-1.0.88-macos-aarch64 \
            b0185133b3362c406069653e40c9864876342fd6d667555e0438f7f26a1f785e "$pkg/bin/grok" ;;
    grok-Linux-x86_64)
        fetch https://x.ai/cli/grok-1.0.88-linux-x86_64 \
            a33013fe73ea63d2f18df636f90f84060a88712f9932b77cda0447dd83b4bfe5 "$pkg/bin/grok" ;;
    # The archives' SHA-512 values match Antigravity's published manifest for 1.5.11.
    antigravity-Darwin-arm64)
        fetch https://storage.googleapis.com/antigravity-public/antigravity-cli/1.5.11-6132547114106880/darwin-arm/cli_mac_arm64.tar.gz \
            74d7eed163e25a77f159b1e595a0e17dd8834ed540f8df6125d1889a7c886774 "$pkg/antigravity.tar.gz" ;;
    antigravity-Linux-x86_64)
        fetch https://storage.googleapis.com/antigravity-public/antigravity-cli/1.5.11-6132547114106880/linux-x64/cli_linux_x64.tar.gz \
            df652c98fdf519a87c017b4e1d92f3fdb66c344de16e5e685c596637bbd28334 "$pkg/antigravity.tar.gz" ;;
    hermes-Darwin-arm64 | hermes-Linux-x86_64)
        fetch "https://raw.githubusercontent.com/NousResearch/hermes-agent/$hermes_commit/scripts/install.sh" \
            c73dad4dcf7c9ecadb98578e28cc763d23d7be3fa1b513545319038795493214 "$pkg/hermes-install.sh" ;;
    *)
        echo "no pinned $runtime download for $platform" >&2
        exit 1 ;;
esac
case "$runtime" in
    grok)
        chmod 0755 "$pkg/bin/grok" ;;
    antigravity)
        # The archive holds one program, antigravity; CCCC's default command calls it agy.
        tar -xzf "$pkg/antigravity.tar.gz" -C "$pkg/bin" antigravity
        mv "$pkg/bin/antigravity" "$pkg/bin/agy" ;;
    hermes)
        bash "$pkg/hermes-install.sh" --commit "$hermes_commit" --non-interactive \
            --skip-browser --skip-computer-use ;;
esac
