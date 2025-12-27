Inspired by [parrot](https://github.com/aquelemiguel/parrot).
Removed multiple message and response to avoid notifications and spamming channels. Can create playlists and reload them, and has a previous feature.

## Installation

This project requires OpenSSL library at compile time. You will want to install it before compiling:

```sh
sudo apt-get update
sudo apt-get install libssl-dev
cargo build --release
```

## To do

- [ ] Separate between major errors which should return directly and minor errors for which a simple warning message should be issued (failed to update queue message should not be a major error)
- [x] Save and load playlists
- [x] Bot stops streaming after 30 minutes or so
- [ ] Review locks, might be locking way too much
