CREATE TABLE apps (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    registered INTEGER NOT NULL
) STRICT;

CREATE TABLE app_redirects (
    app TEXT NOT NULL REFERENCES apps (id) ON DELETE CASCADE,
    uri TEXT NOT NULL,
    PRIMARY KEY (app, uri)
) STRICT;

CREATE TABLE authorization_codes (
    fingerprint TEXT PRIMARY KEY,
    account TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    app TEXT NOT NULL,
    redirect TEXT NOT NULL,
    challenge TEXT NOT NULL,
    expires INTEGER NOT NULL,
    FOREIGN KEY (app, redirect) REFERENCES app_redirects (app, uri) ON DELETE CASCADE
) STRICT;

CREATE TABLE connections (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    app TEXT NOT NULL REFERENCES apps (id) ON DELETE CASCADE,
    connected INTEGER NOT NULL,
    last_used INTEGER NOT NULL
) STRICT;

CREATE INDEX connections_by_account ON connections (account);
CREATE INDEX connections_by_app ON connections (app);

CREATE TABLE access_tokens (
    fingerprint TEXT PRIMARY KEY,
    connection TEXT NOT NULL REFERENCES connections (id) ON DELETE CASCADE,
    expires INTEGER NOT NULL
) STRICT;

CREATE INDEX access_tokens_by_connection ON access_tokens (connection);

CREATE TABLE refresh_tokens (
    fingerprint TEXT PRIMARY KEY,
    connection TEXT NOT NULL UNIQUE REFERENCES connections (id) ON DELETE CASCADE
) STRICT;
