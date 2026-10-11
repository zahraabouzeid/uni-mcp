CREATE TABLE accounts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    name_key TEXT NOT NULL UNIQUE,
    password TEXT NOT NULL,
    owner INTEGER NOT NULL DEFAULT 0,
    profile TEXT,
    custom_instructions TEXT,
    created INTEGER NOT NULL
) STRICT;

CREATE TABLE invites (
    fingerprint TEXT PRIMARY KEY,
    expires INTEGER NOT NULL
) STRICT;

CREATE TABLE sessions (
    fingerprint TEXT PRIMARY KEY,
    account TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    expires INTEGER NOT NULL
) STRICT;

CREATE TABLE secrets (
    account TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    nonce BLOB NOT NULL,
    sealed BLOB NOT NULL,
    PRIMARY KEY (account, key)
) STRICT;
