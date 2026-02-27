# Purpose
A visible running application that can control open port and long-running SSH commands. I'm tired of having:
- local services stop running and
- SSH tunneling / SSH local port forwarding commands fall over and
- not being sure where I was running the command.

This app is the home for such long-lived commands. It keeps track of standing them up on command, shutting them down if requested, showing a dashboard of statuses, and logging a history of restarts.

