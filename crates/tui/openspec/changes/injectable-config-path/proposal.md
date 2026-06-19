## Why

Tests in `app.rs` rufen `TuiConfig::save()` auf, das immer nach `openspec/tui-config.yaml` relativ zum CWD schreibt. Da Tests im Projekt-Root laufen, wird die echte Config-Datei bei jedem Testlauf mit Testdaten überschrieben. Die Config-Tests in `config.rs` nutzen bereits `save_to()`/`load_from()` mit Temp-Verzeichnissen — aber `App` hat keinen Weg, einen alternativen Pfad zu verwenden.

## What Changes

- `App`-Struct erhält ein `config_path: PathBuf`-Feld, das den Speicherort der Config bestimmt
- `App::new()` setzt `config_path` auf den Standard-Pfad (`openspec/tui-config.yaml`)
- Der Save-Handler in `handle_config_input` nutzt `save_to(&self.config_path)` statt `save()`
- Config-Laden beim App-Start nutzt `load_from(&config_path)` statt `load()`
- Tests können `App` mit einem Temp-Pfad konstruieren, sodass die echte Config unberührt bleibt

## Capabilities

### New Capabilities

### Modified Capabilities
- `tui-configuration`: Config-Pfad wird injizierbar statt hardcoded. Load/Save nutzen den konfigurierbaren Pfad.

## Impact

- `src/app.rs`: `App`-Struct bekommt neues Feld, Konstruktor und Save-Handler werden angepasst
- `src/config.rs`: Keine Änderung nötig (`load_from`/`save_to` existieren bereits)
- Tests in `app.rs`: Nutzen Temp-Verzeichnisse statt echten Pfad
- `openspec/tui-config.yaml`: Wird nicht mehr durch Tests überschrieben
