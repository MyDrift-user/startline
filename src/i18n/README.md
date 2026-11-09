# Startline in other languages

Startline shows its text in English, German or Italian (Settings > Appearance > Language, or the `Language` policy).
The default is the language Windows is shown in when it's one of these, and English otherwise. The English in the code
is the key; the catalogs in `de/` and `it/` hold the translations, one file per part of the app.

## Marking text

| What | How |
|---|---|
| Plain text | `tr("Open")` |
| Text with values in it | `trf!("Version {version} is available.", version = v)`, named `{values}` only |
| Text for a count | `trn!(count, "{n} file", "{n} files")`, the first for exactly one; `{n}` is the count, written as the language writes numbers |
| Text kept in a table | `tx("Recent files")` where it's written, `tr(entry)` where it's shown |
| Text in a table that reaches the screen as a `String` | `tr_dyn(&s)` |
| Text for a language other than the one shown (the policy templates) | `tr_in(lang, tx("…"))` |
| A string that isn't text for a person | the line ends with `// i18n-ignore: <why>` |

Never build a sentence from pieces (`tr("Search") + " " + tr("the web")`): word order differs between languages. Give
the whole sentence with `{values}`. Counts, sizes, decimals and dates go through `fmt_count`, `fmt_size`, `fmt_decimal`
and `fmt_date`, which write them as the language does (1.234, 2,5 GB, 3,4 ms, 02.11.2026 in German).

`cargo test i18n` checks it all: every marked text has a German and an Italian translation with the same `{values}`,
no translation is left over, and no string that reads like words to a person is left unmarked.
`STARTLINE_I18N_ONLY=src/home.rs cargo test no_text_is_left_unmarked` looks at some files only.

## How Startline speaks

Short, plain sentences, as in the English. Say what happens and what to do; no "please", no exclamation marks.

- **German:** "Sie", as Windows does. Infinitives for commands in menus and buttons ("Ordner öffnen"). Windows's own
  words for Windows's own things. Keys as written on German keyboards: Strg, Umschalt, Alt, Entf, Eingabetaste,
  Rücktaste, Esc; "Strg+Eingabetaste", "Umschalt+Start".
- **Italian:** the informal "tu" in imperatives, as Microsoft's Italian does ("Scegli una cartella"); infinitives in
  menus and buttons ("Apri percorso file"). Keys: Ctrl, Maiusc, Alt, Canc, Invio, Backspace, Esc; "Ctrl+Invio",
  "Maiusc+Start".
- Keep "Startline", "Windows", "Mica", "regex", "Wi-Fi", "Bluetooth", "Game Mode" only where Windows keeps it, file
  names and extensions as they are.
- Keep `{values}` exactly as in the English; move them wherever the sentence needs them.
- Keep the punctuation of the English at the ends of lines (a full stop or none, "…" for something that asks first).

## Glossary

Windows 11's own words wherever it has one.

| English | German | Italian |
|---|---|---|
| Start menu | Startmenü | menu Start |
| Start button / Start key | Startschaltfläche / Windows-Taste | pulsante Start / tasto Windows |
| Pinned | Angeheftet | Aggiunte |
| Pin to Start / Unpin from Start | An "Start" anheften / Von "Start" lösen | Aggiungi a Start / Rimuovi da Start |
| All apps | Alle Apps | Tutte le app |
| Recent files / Most used | Zuletzt verwendete Dateien / Meistverwendet | File recenti / Più usate |
| Search / Search the web | Suchen / Im Web suchen | Cerca / Cerca sul Web |
| Settings | Einstellungen | Impostazioni |
| Taskbar | Taskleiste | Barra delle applicazioni |
| File Explorer | Datei-Explorer | Esplora file |
| File / folder / drive | Datei / Ordner / Laufwerk | file / cartella / unità |
| Documents / Pictures / Music / Videos | Dokumente / Bilder / Musik / Videos | Documenti / Immagini / Musica / Video |
| Network drive / USB drive | Netzlaufwerk / USB-Laufwerk | Unità di rete / Unità USB |
| Control Panel | Systemsteuerung | Pannello di controllo |
| Recycle Bin | Papierkorb | Cestino |
| Open / Open with / Open file location | Öffnen / Öffnen mit / Dateipfad öffnen | Apri / Apri con / Apri percorso file |
| Run as administrator | Als Administrator ausführen | Esegui come amministratore |
| Copy path | Als Pfad kopieren | Copia come percorso |
| Properties | Eigenschaften | Proprietà |
| Uninstall | Deinstallieren | Disinstalla |
| More options | Weitere Optionen anzeigen | Mostra altre opzioni |
| Power / Sleep / Shut down / Restart | Ein/Aus / Energie sparen / Herunterfahren / Neu starten | Arresta / Sospendi / Arresta il sistema / Riavvia il sistema |
| Lock / Sign out / Switch user | Sperren / Abmelden / Benutzer wechseln | Blocca / Disconnetti / Cambia utente |
| Account settings / Sign-in options | Kontoeinstellungen / Anmeldeoptionen | Impostazioni account / Opzioni di accesso |
| Dark mode / Light mode | Dunkelmodus / Hellmodus | Modalità scura / Modalità chiara |
| Transparency effects | Transparenzeffekte | Effetti di trasparenza |
| Accent colour | Akzentfarbe | Colore principale |
| Volume / Brightness | Lautstärke / Helligkeit | Volume / Luminosità |
| Hidden items / File name extensions | Ausgeblendete Elemente / Dateinamenerweiterungen | Elementi nascosti / Estensioni nomi file |
| Startup apps | Autostart-Apps | App di avvio |
| Start with Windows | Mit Windows starten | Avvia con Windows |
| Managed by your organization | Von Ihrer Organisation verwaltet | Gestito dalla tua organizzazione |
| administrator | Administrator | amministratore |
| update(s) | Update(s) | aggiornamento/i |
| Cancel / OK / Close | Abbrechen / OK / Schließen | Annulla / OK / Chiudi |
| keyboard shortcut | Tastenkombination | scelta rapida da tastiera |
| regular expression | regulärer Ausdruck | espressione regolare |
