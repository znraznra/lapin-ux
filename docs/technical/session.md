# Lapin UX technical reference: Session and first-run defaults

[Back to the technical reference](../TECHNICAL.md)

**Wayland protocol tracing** (#81): create `~/.local/state/lapin-ux/debug-wayland` before signing in to log Wayland messages to `session.log`. It is very chatty, so enable it for one test. After testing, remove the file and sign out and back in; removing it alone does not change the environment of the running shell.
