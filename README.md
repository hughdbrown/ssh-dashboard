# Purpose
A visible running application that can control open port and long-running SSH commands. I'm tired of having:
- local services stop running and
- SSH tunneling / SSH local port forwarding commands fall over and
- not being sure where I was running the command.

This app is the home for such long-lived commands. It keeps track of standing them up on command, shutting them down if requested, showing a dashboard of statuses, and logging a history of restarts.

# Known problems
1. ssh key passphrase prompt
You can avoid this by registering your SSH key before calling `ssh-dashboard`::
```
eval "$(ssh-agent -s)"
ssh-add ~/.ssh/some-machine-private-key-file
```
You will be prompted once for the passphrase of the key file, but not in `ssh-dashboard`.

2. ssh user password prompt
Try to avoid being prompted for a user password. For a start, don't allow password login as an option on your machines.
Secondly, pass `-i ~/.ssh/somemachine-private-key-file` as a command line option to your port-forwarding command to ssh.
