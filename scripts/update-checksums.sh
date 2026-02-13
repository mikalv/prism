#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:?Usage: $0 <version>}"
REPO="mikalv/prism"
BASE_URL="https://github.com/${REPO}/releases/download/v${VERSION}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
DL_DIR="$(mktemp -d)"
trap 'rm -rf "${DL_DIR}"' EXIT

echo "==> Downloading archives for v${VERSION}..."

# Download binary archives (Homebrew)
for label in darwin-aarch64 darwin-x86_64 linux-aarch64-static linux-x86_64-static; do
  archive="prism-v${VERSION}-${label}.tar.gz"
  echo "    ${archive}"
  curl -fSL -o "${DL_DIR}/${archive}" "${BASE_URL}/${archive}"
done

# Download source tarball (AUR / RPM)
echo "    v${VERSION}.tar.gz (source)"
curl -fSL -o "${DL_DIR}/source.tar.gz" "https://github.com/${REPO}/archive/v${VERSION}.tar.gz"

# Compute checksums
SHA_DARWIN_AARCH64="$(shasum -a 256 "${DL_DIR}/prism-v${VERSION}-darwin-aarch64.tar.gz" | awk '{print $1}')"
SHA_DARWIN_X86_64="$(shasum -a 256 "${DL_DIR}/prism-v${VERSION}-darwin-x86_64.tar.gz" | awk '{print $1}')"
SHA_LINUX_AARCH64="$(shasum -a 256 "${DL_DIR}/prism-v${VERSION}-linux-aarch64-static.tar.gz" | awk '{print $1}')"
SHA_LINUX_X86_64="$(shasum -a 256 "${DL_DIR}/prism-v${VERSION}-linux-x86_64-static.tar.gz" | awk '{print $1}')"
SHA_SOURCE="$(shasum -a 256 "${DL_DIR}/source.tar.gz" | awk '{print $1}')"

echo ""
echo "==> Checksums:"
echo "    darwin-aarch64:       ${SHA_DARWIN_AARCH64}"
echo "    darwin-x86_64:        ${SHA_DARWIN_X86_64}"
echo "    linux-aarch64-static: ${SHA_LINUX_AARCH64}"
echo "    linux-x86_64-static:  ${SHA_LINUX_X86_64}"
echo "    source tarball:       ${SHA_SOURCE}"

# --- Update Homebrew formula ---
FORMULA="${ROOT_DIR}/packaging/homebrew/prism.rb"
echo ""
echo "==> Updating ${FORMULA}"

sed -i.bak \
  -e "s/^  version \".*\"/  version \"${VERSION}\"/" \
  "${FORMULA}"

sed -i.bak \
  "/darwin-aarch64/,/sha256/{s/sha256 \".*\"/sha256 \"${SHA_DARWIN_AARCH64}\"/;}" \
  "${FORMULA}"

sed -i.bak \
  "/darwin-x86_64/,/sha256/{s/sha256 \".*\"/sha256 \"${SHA_DARWIN_X86_64}\"/;}" \
  "${FORMULA}"

sed -i.bak \
  "/linux-aarch64-static/,/sha256/{s/sha256 \".*\"/sha256 \"${SHA_LINUX_AARCH64}\"/;}" \
  "${FORMULA}"

sed -i.bak \
  "/linux-x86_64-static/,/sha256/{s/sha256 \".*\"/sha256 \"${SHA_LINUX_X86_64}\"/;}" \
  "${FORMULA}"

rm -f "${FORMULA}.bak"

# --- Update AUR PKGBUILD ---
PKGBUILD="${ROOT_DIR}/packaging/aur/PKGBUILD"
echo "==> Updating ${PKGBUILD}"

sed -i.bak \
  -e "s/^pkgver=.*/pkgver=${VERSION}/" \
  -e "s/^sha256sums=.*/sha256sums=('${SHA_SOURCE}')/" \
  "${PKGBUILD}"
rm -f "${PKGBUILD}.bak"

# --- Update AUR .SRCINFO ---
SRCINFO="${ROOT_DIR}/packaging/aur/.SRCINFO"
echo "==> Updating ${SRCINFO}"

sed -i.bak \
  -e "s/pkgver = .*/pkgver = ${VERSION}/" \
  -e "s|source = .*|source = prismsearch-${VERSION}.tar.gz::https://github.com/mikalv/prism/archive/v${VERSION}.tar.gz|" \
  -e "s/sha256sums = .*/sha256sums = ${SHA_SOURCE}/" \
  "${SRCINFO}"
rm -f "${SRCINFO}.bak"

echo ""
echo "==> Done! Updated packaging files for v${VERSION}."
echo "    - packaging/homebrew/prism.rb"
echo "    - packaging/aur/PKGBUILD"
echo "    - packaging/aur/.SRCINFO"
