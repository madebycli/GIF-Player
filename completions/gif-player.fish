complete -c gif-player -f
complete -c gif-player -l gif-dir -r -d 'GIF-Verzeichnis'
complete -c gif-player -n '__fish_use_subcommand' -a 'run ipc all list edit lock stop-all kill-all picker control daemon' -d 'GIF Player Befehl'
complete -c gif-player -n '__fish_seen_subcommand_from run' -l id -r -d 'Explizite Widget-ID'
complete -c gif-player -n '__fish_seen_subcommand_from run' -l monitor -r -d 'Monitorindex'
complete -c gif-player -n '__fish_seen_subcommand_from run' -l state -r -d 'Startzustand als JSON'
complete -c gif -w gif-player
