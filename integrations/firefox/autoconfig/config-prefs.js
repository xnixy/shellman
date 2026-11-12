// shellman Islands: makes Firefox run config.js (next to firefox.exe) once at startup.
// Installed into <Firefox install folder>\defaults\pref\ by install.ps1 -Scripts. Remove it to turn scripts off.
pref("general.config.filename", "config.js");
pref("general.config.obscure_value", 0);
// config.js must reach the profile folder and register chrome scripts; the autoconfig sandbox
// (the default) only allows pref() calls, so it has to be off. See README ("Scripts").
pref("general.config.sandbox_enabled", false);
