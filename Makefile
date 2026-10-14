PREFIX ?= /usr/local
DESTDIR ?=
CARGO ?= cargo
.PHONY: all check install uninstall
all:
	$(CARGO) build --release --locked
check:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings
	$(CARGO) test --locked
install:
	install -Dm755 target/release/gif-player $(DESTDIR)$(PREFIX)/bin/gif-player
	ln -sf gif-player $(DESTDIR)$(PREFIX)/bin/noctalia-gif-player
	ln -sf gif-player $(DESTDIR)$(PREFIX)/bin/gif-picker
	ln -sf gif-player $(DESTDIR)$(PREFIX)/bin/gif-control
	install -Dm644 data/gif-player-picker.desktop $(DESTDIR)$(PREFIX)/share/applications/gif-player-picker.desktop
	install -Dm644 data/gif-player-control.desktop $(DESTDIR)$(PREFIX)/share/applications/gif-player-control.desktop
	install -Dm644 completions/gif-player.fish $(DESTDIR)$(PREFIX)/share/fish/vendor_completions.d/gif-player.fish
	mkdir -p $(DESTDIR)$(PREFIX)/share/noctalia-gif-player/plugins/gif-player
	cp -r plugins/gif-player/. $(DESTDIR)$(PREFIX)/share/noctalia-gif-player/plugins/gif-player/
	cp NOTICE.md $(DESTDIR)$(PREFIX)/share/noctalia-gif-player/plugins/gif-player/
	mkdir -p $(DESTDIR)$(PREFIX)/share/doc/noctalia-gif-player
	cp README.md NOTICE.md ARCHITECTURE.md PACKAGING.md $(DESTDIR)$(PREFIX)/share/doc/noctalia-gif-player/
	cp -r docs $(DESTDIR)$(PREFIX)/share/doc/noctalia-gif-player/
uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/gif-player $(DESTDIR)$(PREFIX)/bin/noctalia-gif-player $(DESTDIR)$(PREFIX)/bin/gif-picker $(DESTDIR)$(PREFIX)/bin/gif-control
	rm -f $(DESTDIR)$(PREFIX)/share/applications/gif-player-picker.desktop $(DESTDIR)$(PREFIX)/share/applications/gif-player-control.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/fish/vendor_completions.d/gif-player.fish
	rm -rf $(DESTDIR)$(PREFIX)/share/noctalia-gif-player $(DESTDIR)$(PREFIX)/share/doc/noctalia-gif-player
