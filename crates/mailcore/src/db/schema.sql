-- mailclient schema v1.
-- Conventions: lowercase keywords, snake_case, UTC RFC3339 TEXT timestamps,
-- foreign keys with ON DELETE CASCADE, no secrets in the DB (see
-- accounts.auth_vault_key which references the OS keyring).

create table if not exists schema_meta (
    key   text primary key,
    value text not null
);

-- ---------------------------------------------------------------- accounts
create table if not exists accounts (
    id                 integer primary key autoincrement,
    name               text not null,
    email_address      text not null,
    imap_host          text not null,
    imap_port          integer not null default 993,
    imap_security      text not null default 'tls',
    imap_username      text not null default '',
    smtp_host          text not null,
    smtp_port          integer not null default 465,
    smtp_security      text not null default 'tls',
    smtp_username      text not null default '',
    auth_vault_key     text not null,
    check_interval_secs integer not null default 300,
    created_at         text not null,
    updated_at         text not null
);
create unique index if not exists idx_accounts_email on accounts (email_address);

-- ----------------------------------------------------------------- folders
create table if not exists folders (
    id            integer primary key autoincrement,
    account_id    integer not null references accounts (id) on delete cascade,
    path          text not null,
    delimiter     text not null default '/',
    role          text not null default 'custom',
    uid_validity  integer,
    uid_next      integer,
    subscribed    integer not null default 1,
    last_sync_at  text,
    created_at    text not null,
    updated_at    text not null,
    unique (account_id, path)
);
create index if not exists idx_folders_account on folders (account_id);

-- ---------------------------------------------------------------- messages
create table if not exists messages (
    id                 integer primary key autoincrement,
    account_id         integer not null references accounts (id) on delete cascade,
    folder_id          integer not null references folders (id) on delete cascade,
    uid                integer not null,
    message_id_header  text,
    thread_id          text,
    subject            text,
    from_addr          text,
    to_addrs           text not null default '[]',
    cc_addrs           text not null default '[]',
    bcc_addrs          text not null default '[]',
    reply_to           text,
    date               text,
    snippet            text,
    body_text          text,
    body_html          text,
    is_read            integer not null default 0,
    is_starred         integer not null default 0,
    is_draft           integer not null default 0,
    has_attachments    integer not null default 0,
    keywords           text not null default '[]',
    size               integer not null default 0,
    downloaded_full    integer not null default 0,
    created_at         text not null,
    updated_at         text not null,
    unique (account_id, folder_id, uid)
);
create index if not exists idx_messages_folder_date
    on messages (folder_id, date desc, id desc);
create index if not exists idx_messages_account_thread
    on messages (account_id, thread_id);
create index if not exists idx_messages_unread
    on messages (folder_id, is_read);

-- --------------------------------------------- full-text search (FTS5, external content)
create virtual table if not exists messages_fts using fts5 (
    subject,
    from_addr,
    body_text,
    content = 'messages',
    content_rowid = 'id'
);
create trigger if not exists trg_messages_ai after insert on messages begin
    insert into messages_fts (rowid, subject, from_addr, body_text)
    values (new.id, new.subject, new.from_addr, new.body_text);
end;
create trigger if not exists trg_messages_ad after delete on messages begin
    insert into messages_fts (messages_fts, rowid, subject, from_addr, body_text)
    values ('delete', old.id, old.subject, old.from_addr, old.body_text);
end;
create trigger if not exists trg_messages_au after update on messages begin
    insert into messages_fts (messages_fts, rowid, subject, from_addr, body_text)
    values ('delete', old.id, old.subject, old.from_addr, old.body_text);
    insert into messages_fts (rowid, subject, from_addr, body_text)
    values (new.id, new.subject, new.from_addr, new.body_text);
end;

-- ------------------------------------------------------------- attachments
create table if not exists attachments (
    id           integer primary key autoincrement,
    message_id   integer not null references messages (id) on delete cascade,
    filename     text,
    mime_type    text,
    size         integer not null default 0,
    content_id   text,
    storage_path text,
    created_at   text not null
);
create index if not exists idx_attachments_message on attachments (message_id);

-- ---------------------------------------------------------------- contacts
create table if not exists contacts (
    address      text primary key,
    name         text,
    times_seen   integer not null default 1,
    last_seen_at text not null
);
create index if not exists idx_contacts_seen on contacts (times_seen desc, last_seen_at desc);

-- -------------------------------------------------------------- send_queue
create table if not exists send_queue (
    id          integer primary key autoincrement,
    account_id  integer not null references accounts (id) on delete cascade,
    message_id  integer references messages (id) on delete set null,
    status      text not null default 'queued',
    last_error  text,
    retries     integer not null default 0,
    created_at  text not null,
    updated_at  text not null
);
create index if not exists idx_send_queue_pending
    on send_queue (status, created_at);

-- ------------------------------------------------------------ app settings
-- Simple key/value store for user preferences (see store::settings for keys
-- and defaults). Secrets never belong here — they live in the OS keyring.
create table if not exists settings (
    key   text primary key,
    value text not null
);
