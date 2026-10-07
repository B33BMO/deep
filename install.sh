#!/bin/sh
# Installs the latest deep release to ~/.local/bin.
#   curl -fsSL https://github.com/B33BMO/deep/releases/latest/download/install.sh | sh
set -eu

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) asset=deep-linux-x64 ;;
  *) echo "No prebuilt deep for $(uname -s) $(uname -m). Build it with: cargo install --git https://github.com/B33BMO/deep" >&2; exit 1 ;;
esac

dir="${DEEP_INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$dir"
url="https://github.com/B33BMO/deep/releases/latest/download/$asset"
echo "Downloading $url"
curl -fsSL "$url" -o "$dir/deep.new"
chmod +x "$dir/deep.new"
mv -f "$dir/deep.new" "$dir/deep"

"$dir/deep" --version
echo "Installed to $dir/deep"
case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "Note: $dir isn't on your PATH. Add it, or run $dir/deep" ;;
esac
