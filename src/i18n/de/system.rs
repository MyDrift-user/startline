pub const T: &[(&str, &str)] = &[
    // system.rs: the tray icon's menu
    ("Open Startline", "Startline öffnen"),
    ("Settings...", "Einstellungen..."),
    ("Start key opens Startline", "Windows-Taste öffnet Startline"),
    ("Quit Startline", "Startline beenden"),
    // update.rs
    ("The update's signature doesn't match, so it was ignored", "Die Signatur des Updates stimmt nicht, daher wurde es ignoriert"),
    ("The update's file list couldn't be read", "Die Dateiliste des Updates konnte nicht gelesen werden"),
    ("Another account set this for the whole PC. That account or an administrator can change it", "Ein anderes Konto hat dies für den ganzen PC festgelegt. Dieses Konto oder ein Administrator kann es ändern"),
    ("The download failed ({status})", "Der Download ist fehlgeschlagen ({status})"),
    ("Couldn't run curl: {error}", "curl konnte nicht ausgeführt werden: {error}"),
    ("This copy can't update itself", "Diese Kopie kann sich nicht selbst aktualisieren"),
    ("The release has no {name}", "Diese Version enthält keine Datei {name}"),
    ("The downloaded update didn't match its signed checksum, so it was deleted", "Das heruntergeladene Update stimmte nicht mit seiner signierten Prüfsumme überein und wurde daher gelöscht"),
    ("This copy isn't installed by an installer", "Diese Kopie wurde nicht mit einem Installationsprogramm installiert"),
    ("Startline's folder is unknown", "Der Ordner von Startline ist unbekannt"),
    ("Couldn't prepare the update: {error}", "Das Update konnte nicht vorbereitet werden: {error}"),
    ("Couldn't start the update: {error}", "Das Update konnte nicht gestartet werden: {error}"),
    ("Nothing is ready to install", "Es ist nichts zum Installieren bereit"),
    ("Couldn't install the update: {error}", "Das Update konnte nicht installiert werden: {error}"),
    ("There's no folder to download updates to", "Es gibt keinen Ordner, in den Updates heruntergeladen werden können"),
    // main.rs
    ("Startline couldn't open its window.\n\n{error}", "Startline konnte sein Fenster nicht öffnen.\n\n{error}"),
    // platform.rs: the name shown when Windows doesn't say who's signed in
    ("You", "Sie"),
];
