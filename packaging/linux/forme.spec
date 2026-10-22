Name: forme
Version: 0.0.1
Release: 1%{?dist}
Summary: Governed proactive personal Agent kernel
License: MIT
BuildArch: x86_64

%description
A single-owner brain with a system service and browser workbench.

%files
/opt/forme
/etc/forme
/usr/lib/systemd/system/forme-host.service
/usr/lib/systemd/system/forme-web.service
/usr/lib/systemd/system/forme-brain.service

%pre
set -eu
[ "$1" -eq 1 ] || { echo 'forme: use the authenticated lifecycle path for upgrades' >&2; exit 1; }
. /etc/os-release
[ "$ID" = rocky ] && [ "${VERSION_ID%%.*}" = 9 ] && [ "$(uname -m)" = x86_64 ] || exit 1
for root in /opt/forme /etc/forme /var/lib/forme; do
    cursor=$root
    while [ "$cursor" != / ]; do
        [ ! -L "$cursor" ] || exit 1
        cursor=$(dirname "$cursor")
    done
    if [ -e "$root" ]; then
        [ -d "$root" ] && [ -z "$(find "$root" -mindepth 1 -print -quit)" ] || exit 1
    fi
done

%post
set -eu
if [ "$1" -ge 1 ]; then
    getent group forme-brain >/dev/null || groupadd --system forme-brain
    getent passwd forme-brain >/dev/null || useradd --system --gid forme-brain --home-dir /var/lib/forme/brain --shell /sbin/nologin forme-brain
    getent group forme-web >/dev/null || groupadd --system forme-web
    getent passwd forme-web >/dev/null || useradd --system --gid forme-web --home-dir /var/lib/forme/web-auth --shell /sbin/nologin forme-web
    getent group forme-bridge >/dev/null || groupadd --system forme-bridge
    /bin/sh /opt/forme/packaging/posix/service-access.sh linux prepare
    /opt/forme/bin/forme-hostctl prepare-installation \
      --install-root /opt/forme \
      --state-root /var/lib/forme \
      --config-root /etc/forme \
      --log-root /var/log/forme \
      --ipc-root /run/forme \
      --target rocky9_x86_64 \
      --bundle-inventory /opt/forme/bundle-inventory.json || exit 1
    /bin/sh /opt/forme/packaging/posix/service-access.sh linux apply
    /bin/sh /opt/forme/packaging/posix/service-access.sh linux verify
    systemctl daemon-reload
    systemctl enable forme-host.service forme-web.service forme-brain.service
    systemctl start forme-host.service forme-web.service
fi

%preun
set -eu
if [ "$1" -eq 0 ]; then
    /bin/sh /opt/forme/packaging/linux/forme.prerm remove
else
    echo 'forme: use the authenticated lifecycle path for upgrades' >&2
    exit 1
fi
