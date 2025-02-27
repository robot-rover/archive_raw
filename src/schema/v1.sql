BEGIN;

CREATE TABLE on_disk(
  name     TEXT NOT NULL,
  path     TEXT NOT NULL,
  size      INT NOT NULL,
  checksum BLOB NOT NULL,
  date     TEXT NOT NULL
) STRICT;

CREATE UNIQUE INDEX on_disk_path
ON on_disk(path);

CREATE INDEX on_disk_join
ON on_disk(name, size, checksum, date);

CREATE TABLE on_camera(
  name     TEXT NOT NULL,
  path     TEXT NOT NULL,
  size      INT NOT NULL,
  checksum BLOB NOT NULL,
  date     TEXT NOT NULL,

  saved     INT NOT NULL DEFAULT 0
) STRICT;

CREATE UNIQUE INDEX on_camera_path
ON on_camera(path);

CREATE INDEX on_camera_join
ON on_camera(name, size, checksum, date);

COMMIT;
