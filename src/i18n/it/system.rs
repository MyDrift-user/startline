pub const T: &[(&str, &str)] = &[
    // system.rs: the tray icon's menu
    ("Open Startline", "Apri Startline"),
    ("Settings...", "Impostazioni..."),
    ("Start key opens Startline", "Il tasto Windows apre Startline"),
    ("Open the Windows Start menu", "Apri il menu Start di Windows"),
    ("Quit Startline", "Esci da Startline"),
    // update.rs
    ("The update's signature doesn't match, so it was ignored", "La firma dell'aggiornamento non corrisponde, quindi è stato ignorato"),
    ("The update's file list couldn't be read", "Impossibile leggere l'elenco dei file dell'aggiornamento"),
    ("Another account set this for the whole PC. That account or an administrator can change it", "Un altro account ha impostato questa opzione per tutto il PC. Può modificarla quell'account o un amministratore"),
    ("The download failed ({status})", "Download non riuscito ({status})"),
    ("Couldn't run curl: {error}", "Impossibile eseguire curl: {error}"),
    ("This copy can't update itself", "Questa copia non può aggiornarsi da sola"),
    ("The release has no {name}", "Questa versione non contiene {name}"),
    ("The downloaded update didn't match its signed checksum, so it was deleted", "L'aggiornamento scaricato non corrispondeva al checksum firmato, quindi è stato eliminato"),
    ("This copy isn't installed by an installer", "Questa copia non è stata installata con un programma di installazione"),
    ("Startline's folder is unknown", "La cartella di Startline non è nota"),
    ("Couldn't prepare the update: {error}", "Impossibile preparare l'aggiornamento: {error}"),
    ("Couldn't start the update: {error}", "Impossibile avviare l'aggiornamento: {error}"),
    ("Nothing is ready to install", "Non c'è niente di pronto da installare"),
    ("Couldn't install the update: {error}", "Impossibile installare l'aggiornamento: {error}"),
    ("There's no folder to download updates to", "Non c'è una cartella in cui scaricare gli aggiornamenti"),
    // main.rs
    ("Startline couldn't open its window.\n\n{error}", "Startline non è riuscito ad aprire la sua finestra.\n\n{error}"),
    // platform.rs: the name shown when Windows doesn't say who's signed in
    ("You", "Tu"),
];
