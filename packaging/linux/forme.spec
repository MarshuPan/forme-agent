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

%post
if [ "$1" -ge 1 ]; then
    getent group forme-brain >/dev/null || groupadd --system forme-brain
    getent passwd forme-brain >/dev/null || useradd --system --gid forme-brain --home-dir /var/lib/forme/brain --shell /sbin/nologin forme-brain
    getent group forme-web >/dev/null || groupadd --system forme-web
    getent passwd forme-web >/dev/null || useradd --system --gid forme-web --home-dir /var/lib/forme/web-auth --shell /sbin/nologin forme-web
    install -d -o forme-brain -g forme-brain -m 0750 /var/lib/forme/brain /var/lib/forme/keys /var/lib/forme/workspaces
    install -d -o forme-web -g forme-web -m 0750 /var/lib/forme/web-auth
    install -d -o root -g root -m 0750 /etc/forme /var/lib/forme/host-state /var/log/forme /run/forme
    /opt/forme/bin/forme-hostctl prepare-installation \
      --install-root /opt/forme \
      --state-root /var/lib/forme \
      --config-root /etc/forme \
      --log-root /var/log/forme \
      --ipc-root /run/forme \
      --target rocky9_x86_64 \
      --bundle-inventory /opt/forme/bundle-inventory.json || exit 1
    chown root:forme-web /etc/forme /etc/forme/web.json /var/lib/forme/web-auth/bootstrap-web.token
    chmod 0750 /etc/forme
    chmod 0640 /etc/forme/web.json /var/lib/forme/web-auth/bootstrap-web.token
    systemctl daemon-reload
    systemctl enable forme-host.service forme-web.service
    systemctl start forme-host.service forme-web.service
fi

%preun
if [ "$1" -eq 0 ]; then
    systemctl disable --now forme-web.service forme-host.service >/dev/null 2>&1 || :
fi
