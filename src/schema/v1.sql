BEGIN;

CREATE TABLE on_disk(
  name TEXT NOT NULL,
  path TEXT NOT NULL,
  size  INT NOT NULL,
  date TEXT NOT NULL
) STRICT;

CREATE UNIQUE INDEX on_disk_path
ON on_disk(path);

CREATE UNIQUE INDEX on_disk_uniq
ON on_disk(name, date);

CREATE INDEX on_disk_join
ON on_disk(name, date, size);

CREATE TABLE on_camera(
  name  TEXT NOT NULL,
  path  TEXT NOT NULL,
  size   INT NOT NULL,
  date  TEXT NOT NULL,

  saved  INT NOT NULL DEFAULT 0
) STRICT;

CREATE UNIQUE INDEX on_camera_path
ON on_camera(path);

CREATE UNIQUE INDEX on_camera_uniq
ON on_camera(name, date);

CREATE INDEX on_camera_join
ON on_camera(name, date, size);

COMMIT;
