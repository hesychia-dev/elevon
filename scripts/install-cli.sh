#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-latest}"

REPO_PATH="hesychia-dev/elevon"

CRATE_NAME="elevon-cli"
BIN_NAME="elevon"
INSTALL_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/elevon/bin"
LINK_PATH="$HOME/.local/bin/$BIN_NAME"

die() {
	echo "error: $*" >&2
	exit 1
}
need() {
	command -v "$1" >/dev/null || die "missing dependency: $1"
}

resolve_version() {
	if [[ "$VERSION" != "latest" ]]; then
		echo "${VERSION#v}"
		return
	fi

	local api_url="https://api.github.com/repos/${REPO_PATH}/releases?per_page=100"

	echo "Fetching releases from: $api_url" >&2

	curl --fail --location --silent --show-error "$api_url" |
		awk -F'"' '
            /"tag_name"[[:space:]]*:[[:space:]]*"elevon-cli-v/ && !found {
                for (i = 1; i <= NF; i++) {
                    if ($i ~ /^elevon-cli-v/) {
                        sub(/^elevon-cli-v/, "", $i)
                        print $i
                        found = 1
                    }
                }
            }
        '
}

release_url() {
	local version="$1"
	local asset="$2"

	echo "https://github.com/${REPO_PATH}/releases/download/elevon-cli-v${version}/${asset}"
}

resolve_asset() {
	local version="$1"
	local os arch

	os="$(uname -s | tr '[:upper:]' '[:lower:]')"
	arch="$(uname -m)"

	case "$os" in
	linux | darwin) ;;
	*) die "unsupported operating system: $os" ;;
	esac

	case "$arch" in
	x86_64 | amd64) arch="x86_64" ;;
	arm64 | aarch64) arch="aarch64" ;;
	*) die "unsupported architecture: $arch" ;;
	esac

	echo "${CRATE_NAME}-v${version}-${os}-${arch}.tar.gz"
}

main() {
	need curl
	need tar
	need install

	local version asset url tmp_dir source archive_binary
	version="$(resolve_version)"
	[[ -n "$version" ]] || die "could not resolve the latest CLI release"

	asset="$(resolve_asset "$version")"
	url="$(release_url "$version" "$asset")"
	tmp_dir="$(mktemp -d)"
	trap 'rm -rf "${tmp_dir:-}"' EXIT

	echo "Downloading ${url}"
	curl --fail --location --show-error "$url" --output "$tmp_dir/$asset"
	tar -xzf "$tmp_dir/$asset" -C "$tmp_dir"

	archive_binary="${asset%.tar.gz}"
	source="$(find "$tmp_dir" -type f \( -name "$archive_binary" -o -name "$BIN_NAME" \) -print -quit)"
	[[ -n "$source" ]] || die "binary '$BIN_NAME' not found in archive"

	mkdir -p "$INSTALL_DIR" "$(dirname "$LINK_PATH")"
	install -m 0755 "$source" "$INSTALL_DIR/$BIN_NAME"
	ln -sfn "$INSTALL_DIR/$BIN_NAME" "$LINK_PATH"

	echo "Installed $INSTALL_DIR/$BIN_NAME"
	if command -v "$BIN_NAME" >/dev/null; then
		echo "Ready: $(command -v "$BIN_NAME")"
		echo "Run \`elevon --help\` to view commands"
	else
		echo "Add $HOME/.local/bin to PATH, then run \`elevon --help\` to view commands."
	fi
}

main "$@"
