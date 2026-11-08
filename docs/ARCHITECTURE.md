# How it fits together

[Back to the README](../README.md)

```mermaid
flowchart TB
    subgraph always["Always running"]
        session["Session<br/>starts and watches everything"]
        comp["Compositor<br/>COSMIC's, with Lapin changes:<br/>windows, snapping, night light"]
        shell["Shell<br/>taskbar, Start, quick settings,<br/>desktop, Task view"]
        notify["Notifications"]
        session --> comp
        session --> shell
        session --> notify
        shell <-->|Lapin's own protocol| comp
    end
    subgraph opened["Opened when needed"]
        apps["Lapin apps<br/>Task Manager, Control Panel,<br/>Update, Get Started"]
        cosmic["COSMIC programs with Lapin changes<br/>Settings, lock and sign-in, touch keyboard,<br/>Open/Save dialogs, volume pop-ups,<br/>admin prompt, COSMIC app title bars"]
    end
    shared["Shared building blocks<br/>logic and shared controls"]
    shell --> shared
    apps --> shared
```

Lapin UX is built on COSMIC, System76's Rust desktop. It writes its own taskbar, Start menu and apps, and reuses COSMIC's compositor and services, carrying small Lapin changes on top of a few COSMIC programs. Each change is kept in its own file and pinned to COSMIC's stable releases, so COSMIC updates don't break it. The [technical reference](TECHNICAL.md) explains every part.
