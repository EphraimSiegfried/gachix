# Gachix

Gachix is a decentralized binary cache for Nix. It works on machines without Nix
installed. It stores Nix packages in a Git repository with a very unique
structure. Internally, it reduces many store related features of Nix to the Git
object model and Git operations. This structure simplifies many common Nix store
operations, such as finding the dependency closure of a package and replicating
packages with peers.

## Getting started

### Nix

Try it out in a Nix shell with

```
nix shell github:EphraimSiegfried/gachix
```

### Build from source

The binary cache does not have Nix as a dependency and can be run on any Unix
machine. Follow these steps to build from source:

- [Install Cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html)
- Clone the repository:
  `git clone https://github.com/EphraimSiegfried/gachix.git`
- Cd into the repository and run `cargo install`

## Usage

The Gachix server can be started with:

```
gachix serve
```

To add a Nix package, run

```
gachix add <nix-store-path>
```
