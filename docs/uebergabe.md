# PassKey: Übergabe für einen neuen Chat

Stand: 2026-10-25. Projektordner: `C:\Users\Fabian\Documents\Coding\PassKey` (Git-Repo auf `master`, noch nichts committet). Sprache im Chat und in der Oberfläche: Deutsch, per Du.

## Ziel

Ein vollständig offline laufender Passwortgenerator in reinem Rust mit kompaktem Fenster im Design-System **Kiesel** (das Standard-System des Accounts, im Artifact-Tool als „Kiesel“ gelistet: https://claude.ai/artifact/QDsupW2ACmmioBM25EYzBH). Der Verlauf der Passwörter liegt in einer verschlüsselten Datei. Zum Ansehen und Entsperren braucht es ein Master-Passwort. Ein Profilbutton führt zum Verlauf. Die App startet und öffnet ohne Passwortabfrage.

## Entscheidungen des Nutzers

- GUI-Framework: **iced 0.14**.
- Speichern **nur manuell** per Button, nur bei entsperrter Datei. Ein Eintrag hat Passwort, Zeitpunkt, Einstellungen und ein **optionales Etikett**.
- Passwortlänge **12 bis 32** Zeichen (Standard 20).
- Erzeugte, nicht gespeicherte Passwörter liegen nur im RAM. Sie werden **30 s nach dem Minimieren** gelöscht und überschrieben. Beim Schließen sofort (ein 30-s-Timer ergibt dort keinen Sinn, weil der Prozess endet). Kommt das Fenster vor Ablauf zurück, bricht der Timer ab.
- Eigene Titelleiste im Hauptfenster (kein Windows-Rahmen), abgerundete Ecken (16 px), Schließen-Knopf wird beim Hover rot (`clay-mark`).
- Animationen für Buttons, Slider, Scrollen usw. (siehe unten).
- Alle Panels gleich breit wie die Tab-Leiste.

## Stand

**Phase 1 (Design) und Phase 2 (verschlüsselte Datei) sind fertig und laufen.** Der Verlauf liegt in `%APPDATA%\PassKey\verlauf.pkv`. Gibt es die Datei nicht, startet die Ersteinrichtung, sonst die Sperre. Demo-Passwort und Demo-Einträge sind entfernt.

Starten: `cargo run` (oder `target\debug\passkey.exe`). Start-Flags für Screenshots und Tests: `--light`, `--dark`, `--verlauf` (Tab Verlauf), `--no-motion` (Übergänge aus), `--datei <pfad>` (andere Verlaufsdatei, damit Tests die echte nicht berühren; ein Pfad ohne Datei zeigt die Ersteinrichtung). `--entsperrt` und `--setup` gibt es nicht mehr.

Ansichten: **Erzeugen** (Ausgabe, Stärke, Slider, Schalter, Neu erzeugen, Kopieren, Etikett, Speichern), **Verlauf gesperrt** (Master-Passwort), **Verlauf entsperrt** („Nicht gespeichert“ aus dem RAM und „Gespeichert“), **Ersteinrichtung**. Screenshots aller acht Varianten liegen in `docs/screenshots` (sie stammen von vor den Animationen und der Titelleiste, das Aussehen ist aber im Kern gleich).

## Dateien

- `src/main.rs`: Fenster (400 × 700, nicht skalierbar, ohne Rahmen, transparent), Schriften, Start.
- `src/app.rs`: Zustand, Nachrichten, `update`, `view`, Titelleiste, weiches Scrollen. Entsperren und Anlegen laufen per `in_background` in einem eigenen Thread, das Ergebnis kommt als `Message::VaultReady(Handoff)` zurück.
- `src/store.rs`: verschlüsselte Datei (Format im Modulkommentar), `Vault::create/open/add`, atomares Schreiben, 11 Tests.
- `src/theme.rs`: Kiesel-Tokens (hell/dunkel) als Konstanten, Schriften, Kontrast-Test. `WINDOW_W` steuert die Tab-Breite.
- `src/generator.rs`: Passworterzeugung (verlangt `CryptoRng`, die App übergibt `SysRng` = Zufall direkt vom Betriebssystem), Stärke in Bit, Tests.
- `src/drafts.rs`: RAM-Entwürfe, 30-s-Timer (Zeit wird von außen übergeben, daher testbar), `zeroize`.
- `src/motion.rs`: Animationszustände (`Animation<bool>` pro Element), `Fx` für die Ansicht, Scroll-Glättung, Windows-Einstellung „Animationen reduzieren“ (per `SystemParametersInfoW`).
- `src/ui/widgets.rs`: Kiesel-Komponenten (Button mit Varianten Primary/Secondary/Outline/Ghost/Close, Field, Toggle, Slider, Tabs, Badge, Alert, Card, Progress, Trenner).
- `src/ui/generator.rs`, `vault.rs`, `setup.rs`: die Ansichten. In `ui/vault.rs` formatiert `when` den Zeitpunkt gespeicherter Einträge (offenes TODO: freundliche Form „Heute, 09:41“).
- `assets/fonts` (Inter Regular/Medium/SemiBold, Inter Tight SemiBold, OFL), `assets/icons` (Lucide, ISC, Strichstärke 1,8).
- `docs/screenshots`, `docs/uebergabe.md` (diese Datei).

## Technische Erkenntnisse (iced 0.14, Windows)

- **Debug-Build:** `Cargo.toml` optimiert alle Abhängigkeiten auch im Debug-Build (`opt-level = 3`, ohne `debug-assertions` und `overflow-checks`), PassKey selbst nicht. Grund: tiny-skia zeichnet beim Scrollen die ganze Fläche neu. Unoptimiert waren das etwa 100 ms pro Bild (Ruckeln), jetzt im Median 7 ms. Die Prüfungen allein machten fast die Hälfte aus. Argon2 entsperrt so in etwa 0,2 s.
- **Weiches Scrollen ohne Echo:** Solange die Glättung läuft, hängt kein `on_scroll` an der Scrollfläche. Sonst meldet jedes `scroll_to` den eigenen Versatz zurück, und iced baut die Ansicht pro Bild zweimal.
- **Hintergrund-Arbeit:** Der tokio-Executor von iced läuft ohne Multi-Thread-Runtime. Argon2 läuft deshalb in `std::thread` und meldet sich über `futures::channel::oneshot` zurück.
- **Software-Renderer nötig:** `Cargo.toml` nutzt `iced` mit `default-features = false` und `["tiny-skia", "svg", "tokio", "crisp", "web-colors"]`. Mit dem GPU-Renderer (wgpu) waren die Ecken des transparenten Fensters schwarz. tokio läuft nur mit `rt` und `time`, es gibt kein Netzwerk-Crate.
- **Minimieren erkennen:** iced meldet es nicht als Ereignis. Die App fragt alle 500 ms `window::is_minimized` ab.
- **Schriften:** Google liefert nur Variable-Fonts. Daraus wurden mit fontTools statische Schnitte erzeugt (iced wählt Gewichte nicht sicher aus Variable-Fonts).
- **Bewegung:** iced animiert Widget-Zustände nicht. Hover wird über `mouse_area` (`on_enter`/`on_exit`) gemeldet, `Animation<bool>` interpoliert, `window::frames()` liefert Bildschritte nur, solange etwas animiert (im Leerlauf keine Last).
- **Weiches Scrollen:** Eine durchsichtige `mouse_area` mit `on_scroll` liegt in einem `stack` über der Scrollfläche, fängt das Mausrad ab und setzt ein Ziel. Frame für Frame läuft `scroll_to` darauf zu. Klicks und Hover gehen an die Elemente darunter durch.
- **Layout:** Die Scrollfläche reicht über die volle Fensterbreite, der Inhalt hat 16 px Rand links und rechts, die Scrollleiste sitzt im rechten Rand.
- Eigene Schalter statt `toggler` (iced animiert den Knopf nicht).

## Prüfungen

- `cargo test`: **38 Tests bestehen** (Generator-Längen 12–32, Zeichenarten, Entwurfs-Timer mit simulierter Uhr, Kiesel-Kontrast in beiden Themes, Bewegung, Scroll-Glättung; Datei: Roundtrip, falsches Passwort, jedes geänderte Byte, geänderte und absurde Argon2-Parameter, fremde, abgeschnittene und neuere Datei, nie überschreiben, frische Nonce, kein Klartext, Rückrollen bei Schreibfehler, Decoder-Grenzen, übergroße Datei, Etikett- und Passwortregeln, ungültiger Inhalt).
- `cargo clippy --all-targets -- -D warnings` ist sauber (Lints in `Cargo.toml`, Test-Ausnahmen in `clippy.toml`). `cargo fmt --check` ist es nicht: Der Code war nie mit rustfmt formatiert (schon vor dem Sicherheits-Durchgang).
- Phase 2 im echten Fenster geprüft (mit `--datei` im Scratchpad): Ersteinrichtung (0,2 s), zwei Einträge mit Etikett speichern, Schließen, Neustart, falsches Passwort abgelehnt, richtiges zeigt beide Einträge in richtiger Reihenfolge, kein Klartext in der Datei. Die echte Datei unter `%APPDATA%` blieb unberührt.
- Im echten Fenster geprüft (per Skript mit simulierter Maus und Pixelmessung): 30-s-Löschen nach Minimieren (10 s Minimieren: Passwort bleibt; 36 s: gelöscht), normales Schließen, Ziehen/Minimieren/Schließen der Titelleiste, Hover-Übergänge, Schalter, Tabs, Scrollen inkl. Grenzen, abgerundete Ecken, gleiche Panel-Breiten (x 16–383 in allen Ansichten), `--no-motion`, Leerlauf-CPU gering.
- Die Prüfskripte (PowerShell) lagen im Scratchpad der Sitzung und sind nicht im Projekt.

## Abweichungen vom ursprünglichen Plan

- Fenster 400 × 700 statt 380 × 600 (Kiesels 24-px-Kartenrand und 44-px-Zielhöhen).
- Fehlermeldung beim falschen Passwort steht am Feld (Rand + Text) statt in einer Alert-Box.
- Schalter-Namen verkürzt: „Groß (A–Z)“, „Klein (a–z)“, „Zahlen“, „Symbole“, „Ähnliche Zeichen meiden“.

## Noch offen

**Phase 2 ist umgesetzt:** Argon2id (64 MiB, 3 Durchläufe, 1 Lane), XChaCha20-Poly1305, Kopf mit Kennung, Version, Parametern, Salt und Nonce als AAD, Parameter-Obergrenzen beim Lesen, atomares Schreiben, `zeroize`. Kleiner Rest: `when` in `ui/vault.rs` zeigt noch `25.10.2026, 14:08` statt „Heute, 14:08“.

**Phase 3:** Profil-Ablauf mit echter Datei, Sperren beim Schließen, Fehlerzustände, Packaging. Offene Idee: Auto-Sperre nach Inaktivität (z. B. 5 Minuten).

**Sicherheits-Durchgang (Skill rust-secure-coding, 2026-10-25):** Dateiparser ohne `expect` und ohne direkte Indizes, Datei höchstens 8 MiB, Etikett als geprüfter Typ `Label` (höchstens 64 Zeichen, keine Steuerzeichen, das Feld nimmt nicht mehr an), Master-Passwort höchstens 256 Zeichen, Passwörter, Etiketten und Schlüssel in `secrecy` (`SecretString`, `SecretBox`), `Message` ohne `Debug`, Casts über `from`/`try_from`, Puffer im Generator vorab reserviert, Release mit `overflow-checks` und `strip`. `unsafe_code = "deny"` gilt fürs ganze Crate. Einzige, vom Nutzer ausdrücklich freigegebene Ausnahme: `system_prefers_reduced_motion` in `motion.rs` (FFI für „Animationen anzeigen“) mit `#[expect(unsafe_code, reason = …)]` und SAFETY-Kommentar. Offen: zwei `#[allow(clippy::too_many_arguments)]` in `ui/widgets.rs`.

**Bewusst fehlend:**
- Anheben um 1 px beim Hover und Schrumpfen beim Drücken der Buttons (iced kann Widgets nicht fein verschieben).
- Weicher Fokus-Hof um Eingabefelder und Slider-Knopf (der Rand wechselt im Fokus sofort).
- Die Schalter sind per Tastatur nicht bedienbar.
- Die Zwischenablage wird nicht automatisch geleert. Eine Kopie dort bleibt, wenn die Entwürfe gelöscht werden.
- Einzelne gespeicherte Einträge lassen sich nicht löschen, das Master-Passwort nicht ändern.
- Falsches Passwort und manipulierte Datei zeigen dieselbe Meldung (AEAD kann beides nicht unterscheiden).

## Nutzerpräferenzen

Der Nutzer schreibt Deutsch und erwartet deutsche Texte in der App (Kiesel-Ton: kurz, konkret, per Du, keine Ausrufezeichen, kein Emoji). Er möchte die App nach Änderungen direkt starten und ausprobieren können. Änderungswünsche kommen kurz und visuell (zuletzt: Titelleiste integrieren, Ecken abrunden, Schließen-Knopf rot, Animationen, gleiche Panel-Breiten).
