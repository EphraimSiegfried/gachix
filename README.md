# Gachix

A content-addressable binary cache for Nix over Git

## Usage

Get the binary with `cargo run --release`. It will be located at
`target/release`

---

Add a file to the cache:

```
gachix add <file>
```

Get the contents of a file given its hash:

```
gachix get <hash>
```
