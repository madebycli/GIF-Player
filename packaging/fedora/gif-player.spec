Name:           gif-player
Version:        0.3.0
Release:        1%{?dist}
Summary:        GTK3 layer-shell GIF overlay supervisor for Wayland
License:        LicenseRef-Unlicensed
URL:            https://github.com/madebycli/GIF-Player
Source0:        %{name}-%{version}.tar.gz

BuildArch:      noarch
BuildRequires:  python3-devel
BuildRequires:  python3dist(setuptools) >= 69
BuildRequires:  python3dist(wheel)
BuildRequires:  pyproject-rpm-macros
BuildRequires:  desktop-file-utils

Requires:       python3-gobject
Requires:       python3-cairo
Requires:       python3-pillow
Requires:       gtk3
Requires:       gtk-layer-shell
Requires:       gobject-introspection

%description
GIF Player displays animated GIF files as GTK3 layer-shell overlays on
compatible Wayland compositors. One supervisor process manages multiple
windows and communicates with clients through JSON Unix-socket protocol v2.

This is an independently developed implementation. No open-source license or
general redistribution permission is currently granted; see NOTICE.md.

%prep
%autosetup -n %{name}-%{version}

%generate_buildrequires
%pyproject_buildrequires

%build
%pyproject_wheel

%install
%pyproject_install
%pyproject_save_files gif_player_paths gif_player_ipc gif_player_runtime gif_player_runtime_guard gif_player_runtime_patch gif_player_bootstrap gif_player_cli gif_picker_entry gif_control_entry

%check
%{python3} -m compileall -q .
%{python3} -m unittest discover -s tests -v
PYTHONPATH=%{buildroot}%{python3_sitelib} %{buildroot}%{_bindir}/gif-player --help >/dev/null

desktop-file-validate %{buildroot}%{_datadir}/applications/gif-player-picker.desktop
desktop-file-validate %{buildroot}%{_datadir}/applications/gif-player-control.desktop

test -x %{buildroot}%{_bindir}/gif-player
test -x %{buildroot}%{_bindir}/gif-picker
test -x %{buildroot}%{_bindir}/gif-control
test -f %{buildroot}%{_libexecdir}/gif-player/gif-script.py
test -f %{buildroot}%{_libexecdir}/gif-player/gif-picker.py
test -f %{buildroot}%{_libexecdir}/gif-player/gif-control.py
test ! -e %{buildroot}%{_libexecdir}/gif-player/Gifs

%files -f %{pyproject_files}
%license NOTICE.md
%{_bindir}/gif-player
%{_bindir}/gif-picker
%{_bindir}/gif-control
%{_libexecdir}/gif-player/
%{_datadir}/applications/gif-player-picker.desktop
%{_datadir}/applications/gif-player-control.desktop
%{_datadir}/fish/vendor_functions.d/gif.fish
%{_datadir}/fish/vendor_completions.d/gif-player.fish

%changelog
* Tue Jul 21 2026 GIF Player maintainers <madebycli@users.noreply.github.com> - 0.3.0-1
- Add clean cross-distribution RPM packaging
