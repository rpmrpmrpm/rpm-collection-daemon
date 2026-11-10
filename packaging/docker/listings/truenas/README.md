# TrueNAS SCALE (Community Edition)

TrueNAS apps are Docker Compose files. Until the daemon is in the TrueNAS catalogue, install it as
a custom app:

1. Make a dataset for the libraries, for example `tank/music/rpm`, and share it over SMB.
2. Make a dataset for the daemon's data, for example `tank/apps/rpm-collection-daemon`.
3. In **Apps**, choose **Discover Apps**, then **Custom App**, then **Install via YAML**.
4. Paste `compose.yaml` from this folder and change the marked lines.

The user in `user:` must be able to read the libraries dataset. `568` is the TrueNAS `apps` user;
give it read access to the dataset, or use your own user's id.
