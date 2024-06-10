# libsane-sys

This crate provides the raw bindings for the [SANE (Scanner Access Now Easy) document scanner library](https://gitlab.com/sane-project).

# Safety

With the exception of `sane_cancel`, all sane functions must be run sequentially. This can be done either by locking or by limiting function calls to one thread.

# Prerequisites

The following libraries need to be installed:

- libclang-dev
- libsane-dev
- libsane (for running)

On Debian and Ubuntu systems, they can be installed with the following command:

```sh
sudo apt install libclang-dev libsane-dev libsane
```

If your scanner isn't recognized, you might be running an outdated version of SANE.
For the latest version, you can add the PPA repository:

```sh
sudo add-apt-repository ppa:sane-project/sane-release
```
