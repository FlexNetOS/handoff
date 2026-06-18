## Context

Die `App`-Struct nutzt `TuiConfig::load()` und `TuiConfig::save()`, die immer den hardcodierten Pfad `openspec/tui-config.yaml` verwenden. `TuiConfig` bietet bereits `load_from(path)` und `save_to(path)` — diese werden aber nur in den `config.rs`-Tests genutzt. Die `app.rs`-Tests durchlaufen den Save-Flow und überschreiben dabei die echte Config-Datei.

## Goals / Non-Goals

**Goals:**
- Config-Pfad als Feld in `App` speichern, sodass Tests einen Temp-Pfad nutzen können
- Bestehende `load_from`/`save_to`-API in `TuiConfig` weiterverwenden
- Minimale Änderung: nur `App`-Konstruktor und Save-Handler anpassen

**Non-Goals:**
- Änderung der `TuiConfig`-Struct selbst
- Einführung eines Trait-basierten Config-Backends oder Dependency Injection Framework
- Änderung des Config-Dateiformats oder -Pfads im Produktivbetrieb

## Decisions

### 1. `config_path: PathBuf` als Feld in `App`

`App` erhält ein `config_path: PathBuf`-Feld. `App::new()` setzt es auf den Standardpfad (`CONFIG_PATH`). Der Save-Handler nutzt `save_to(&self.config_path)`, der Konstruktor nutzt `load_from(&config_path)`.

**Alternative: CWD in Tests ändern** — Abgelehnt, weil CWD prozessglobal ist und Tests parallel laufen. Race conditions wären unvermeidlich.

**Alternative: Mock/Trait für Filesystem** — Abgelehnt, weil die existierenden `_from`/`_to`-Methoden das Problem bereits lösen. Ein Trait wäre Over-Engineering.

### 2. `CONFIG_PATH` als öffentliche Konstante exportieren

`CONFIG_PATH` wird `pub` gemacht, damit `App::new()` darauf zugreifen kann, ohne den String zu duplizieren.

**Alternative: Methode `TuiConfig::default_path()`** — Möglich, aber eine Konstante ist einfacher und reicht hier aus.

### 3. Test-Helper `App::new_with_config_path(path)` oder direktes Feld setzen

Tests konstruieren `App` mit einem Temp-Pfad. Da `App::new()` auch Daten lädt (Changes-Liste), wird ein separater Konstruktor oder Builder nicht eingeführt. Stattdessen wird `config_path` nach Konstruktion überschrieben oder ein `with_config_path`-Methode ergänzt — je nachdem was die bestehenden Test-Helper (`make_config_app`) bereits tun.

## Risks / Trade-offs

- **Risiko: Vergessen, den Pfad in neuen Features zu nutzen** → Mitigation: `save()` und `load()` ohne Pfad bleiben für CLI/Skript-Nutzung erhalten, aber `App` nutzt immer die `_from`/`_to`-Varianten.
- **Trade-off: Öffentliches Feld vs. Getter** → `config_path` wird `pub` gemacht, konsistent mit dem bestehenden `pub config`-Feld. Einfachheit über Kapselung.
