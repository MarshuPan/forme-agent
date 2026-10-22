#!/bin/sh
# Original fixed first-install policy. No caller-selected paths or repair mode.
set -eu
umask 077
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH

fail() { echo "forme: native service access requires explicit repair: $*" >&2; exit 1; }
[ "$(id -u)" -eq 0 ] || fail 'administrator identity required'
[ "$#" -eq 2 ] || fail 'expected platform and prepare/apply/verify phase'
platform=$1
phase=$2
case "$platform" in
  linux)
    state=/var/lib/forme; config=/etc/forme; logs=/var/log/forme; ipc=/run/forme
    brain=forme-brain; web=forme-web; bridge=forme-bridge
    ;;
  macos)
    state='/Library/Application Support/ForMe'; config="$state/config"
    logs="$state/logs"; ipc="$state/ipc"
    brain=_formebrain; web=_formeweb; bridge=_formebridge
    ;;
  *) fail 'unsupported access platform' ;;
esac
case "$phase" in prepare|apply|verify) ;; *) fail 'unsupported access phase' ;; esac

direct() {
  cursor=$1
  while [ "$cursor" != / ]; do
    [ ! -L "$cursor" ] || fail 'linked path'
    cursor=$(dirname "$cursor")
  done
}

attrs() {
  if [ "$platform" = linux ]; then stat -c '%U:%G:%a:%h' "$1"
  else stat -f '%Su:%Sg:%Lp:%l' "$1"; fi
}

check_tree() {
  for tree in "$state" "$config" "$logs" "$ipc"; do
    direct "$tree"
    if [ -e "$tree" ]; then
      [ -d "$tree" ] || fail 'expected a directory'
      inventory=$(find "$tree" -xdev -print) || fail 'cannot inspect tree'
      [ "$(printf '%s\n' "$inventory" | wc -l | tr -d ' ')" -le 4096 ] || fail 'file inventory bound'
      linked=$(find "$tree" -xdev -type l -print -quit) || fail 'cannot inspect links'
      [ -z "$linked" ] || fail 'linked tree entry'
      linked=$(find "$tree" -xdev -type f -links +1 -print -quit) || fail 'cannot inspect hardlinks'
      [ -z "$linked" ] || fail 'hardlinked tree entry'
    fi
  done
}

directory() {
  access_path=$1; access_owner=$2; access_group=$3; access_mode=$4
  direct "$access_path"
  if [ "$phase" != verify ]; then
    if [ ! -e "$access_path" ]; then mkdir -m 0700 "$access_path"; fi
    [ -d "$access_path" ] || fail 'expected directory'
    chown "$access_owner:$access_group" "$access_path"
    chmod "$access_mode" "$access_path"
  fi
  [ -d "$access_path" ] || fail 'missing directory'
  access_actual=$(attrs "$access_path")
  case "$access_actual" in "$access_owner:$access_group:$access_mode:"*) ;; *) fail 'directory ownership or mode mismatch' ;; esac
}

file() {
  access_path=$1; access_owner=$2; access_group=$3; access_mode=$4
  direct "$access_path"
  [ -f "$access_path" ] || fail 'missing required regular file'
  if [ "$phase" != verify ]; then
    chown "$access_owner:$access_group" "$access_path"
    chmod "$access_mode" "$access_path"
  fi
  [ "$(attrs "$access_path")" = "$access_owner:$access_group:$access_mode:1" ] || fail 'file ownership, mode or link count mismatch'
}

check_tree
if [ "$phase" = prepare ]; then
  # Nothing in these roots may be adopted, chowned or replaced implicitly.
  for root in "$state" "$config" "$logs" "$ipc"; do
    if [ -e "$root" ]; then
      [ -z "$(find "$root" -mindepth 1 -print -quit)" ] || fail 'existing installation or data'
    fi
  done
fi

# Linux root group is root; macOS uses wheel. Service identities stay distinct.
admin=root
if [ "$platform" = macos ]; then admin=wheel; fi
directory "$state" root "$admin" 711
directory "$config" root "$admin" 711
directory "$logs" root "$admin" 711
directory "$ipc" root "$admin" 700
directory "$state/keys" root "$admin" 711
directory "$state/host-state" root "$admin" 700
directory "$state/backups" root "$admin" 700
directory "$state/brain" "$brain" "$brain" 700
directory "$state/workspaces" "$brain" "$brain" 700
directory "$state/web-auth" "$web" "$web" 700
if [ "$phase" != prepare ]; then
  directory "$config/core" root "$brain" 2750
  directory "$state/keys/model-credentials" root "$brain" 2750
fi
directory "$logs/host" root "$admin" 700
directory "$logs/brain" "$brain" "$brain" 700
directory "$logs/web" "$web" "$web" 700

if [ "$phase" = prepare ]; then exit 0; fi
for name in bootstrap.json host.json product-template.json installation.json; do
  file "$config/$name" root "$admin" 600
done
file "$config/web.json" root "$web" 640
for name in bootstrap.key host-maintenance.token; do
  file "$state/keys/$name" root "$admin" 600
done
for name in core-owner.token agent-execution.token; do
  file "$state/keys/$name" root "$brain" 640
done
file "$state/keys/core-web.token" root "$bridge" 640
for name in bootstrap-web.token configuration.token web-assertion.key; do
  file "$state/web-auth/$name" "$web" "$web" 600
done
