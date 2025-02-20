BEGIN;

CREATE TABLE on_disk(
  name     TEXT NOT NULL,
  path     TEXT NOT NULL,
  size      INT NOT NULL,
  checksum BLOB NOT NULL,
  date     TEXT,
  duration  INT
) STRICT;

CREATE TABLE on_camera(
  name     TEXT NOT NULL,
  path     TEXT NOT NULL,
  size      INT NOT NULL,
  checksum BLOB NOT NULL,
  date     TEXT,
  duration  INT,

  saved     INT NOT NULL DEFAULT 0
) STRICT;

COMMIT;
