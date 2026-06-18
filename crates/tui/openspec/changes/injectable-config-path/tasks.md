## 1. Config-Pfad in App-Struct aufnehmen

- [x] 1.1 `CONFIG_PATH` in `config.rs` auf `pub` ändern, damit `app.rs` darauf zugreifen kann
- [x] 1.2 `config_path: PathBuf`-Feld zur `App`-Struct in `app.rs` hinzufügen
- [x] 1.3 `App::new()` anpassen: `config_path` auf `PathBuf::from(config::CONFIG_PATH)` setzen und `TuiConfig::load_from(&config_path)` statt `TuiConfig::load()` verwenden

## 2. Save-Handler auf konfigurierbaren Pfad umstellen

- [x] 2.1 In `handle_config_input` bei `KeyCode::Char('S')`: `save_to(&self.config_path)` statt `save()` verwenden

## 3. Tests auf Temp-Pfad umstellen

- [x] 3.1 `make_config_app()` Test-Helper anpassen: `config_path` auf einen Temp-Pfad setzen (z.B. `std::env::temp_dir().join("openspec-tui-test-config.yaml")`)
- [x] 3.2 Test `test_config_save_returns_to_changelist` validieren: sicherstellen, dass nach dem Test keine echte Config-Datei geschrieben wird
- [x] 3.3 Prüfen, ob die echte `openspec/tui-config.yaml` korrekte Produktivdaten enthält und die Testdaten (`Xtest-tool`) entfernen
