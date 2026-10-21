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
