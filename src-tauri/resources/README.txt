This directory contains generated Windows resources, ignored by Git.

Use scripts\build-windows.ps1. It compiles and checks both:
- minerdesk-backend.exe: GUI subsystem, no terminal, Desktop-owned runtime.
- minerdesk-headless.exe: console subsystem, independent standalone CLI.

scripts/prepare-windows-resources.mjs creates ignored bootstrap placeholders
when a clean Windows Cargo build needs them. They are not usable binaries.
The canonical Windows build copies the matching, verified real binaries into
this directory before building NSIS. Linux does not use these resources.
Do not copy a previous version of either binary into a new installer.
