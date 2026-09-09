set positional-arguments

# Rebuild/reinstall this checkout's CLI; defaults to ~/.local, or pass a prefix.
install prefix=(env('HOME') / '.local'):
    cargo install --locked --path crates/app --root "$1" --force
    "$1/bin/unisphere" --version --json
    "$1/bin/unisphere" adapters list --json
