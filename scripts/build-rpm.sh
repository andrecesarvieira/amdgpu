#!/usr/bin/bash
set -euo pipefail

project_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$project_root/Cargo.toml" | head -n 1)
archive_root="amdgpu-control-$version"
dist_dir="$project_root/dist"
source_dir="$dist_dir/sources"
rpm_dir="$dist_dir/rpm"
srpm_dir="$dist_dir/srpm"
build_dir=$(mktemp -d "${TMPDIR:-/tmp}/amdgpu-control-rpmbuild.XXXXXX")
trap 'rm -rf -- "$build_dir"' EXIT

if ! command -v rpmbuild >/dev/null; then
  echo "rpmbuild não encontrado." >&2
  echo "Instale as dependências descritas no README.md." >&2
  exit 1
fi

if [[ ! -d "$project_root/vendor" || ! -f "$project_root/.cargo/config.toml" ]]; then
  echo "As dependências Cargo vendorizadas não foram encontradas." >&2
  echo "Execute: cargo vendor --locked vendor > .cargo/config.toml" >&2
  exit 1
fi

local_sysroot="$dist_dir/build-deps/sysroot"
if [[ -x "$local_sysroot/usr/bin/gcc" ]]; then
  local_sysroot=$(realpath "$local_sysroot")
  export PATH="$local_sysroot/usr/bin:$PATH"
  export PKG_CONFIG_PATH="$local_sysroot/usr/lib64/pkgconfig:/usr/lib64/pkgconfig:/usr/share/pkgconfig"
  export PKG_CONFIG_SYSROOT_DIR="$local_sysroot"
  export LIBRARY_PATH="$local_sysroot/usr/lib64:/usr/lib64"
  export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$local_sysroot/usr/bin/gcc"
  export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=--sysroot=$local_sysroot"
fi

mkdir -p "$source_dir" "$rpm_dir" "$srpm_dir" "$build_dir"
tar \
  --exclude='./dist' \
  --exclude='./target' \
  --exclude='./.git' \
  --transform="s,^\.,$archive_root," \
  -czf "$source_dir/$archive_root.tar.gz" \
  -C "$project_root" .

rpm_build_args=()
if [[ ${AMDGPU_RPM_SKIP_DEP_CHECK:-0} == 1 ]]; then
  rpm_build_args+=(--nodeps)
fi

rpmbuild -ba "${rpm_build_args[@]}" "$project_root/packaging/amdgpu-control.spec" \
  --define "_sourcedir $source_dir" \
  --define "_rpmdir $rpm_dir" \
  --define "_srcrpmdir $srpm_dir" \
  --define "_builddir $build_dir"

find "$rpm_dir" "$srpm_dir" -type f -name "amdgpu-control-$version-*.rpm" -print
