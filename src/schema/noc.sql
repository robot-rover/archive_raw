BEGIN;

DROP TABLE IF EXISTS new_on_camera;
CREATE TABLE new_on_camera(
  name     TEXT NOT NULL,
  path     TEXT NOT NULL,
  size      INT NOT NULL
) STRICT;

COMMIT;
