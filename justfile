set dotenv-load := true

tables DB="${RAWDB_DB}":
  sqlite3 {{ DB }} 'PRAGMA application_id'
  sqlite3 {{ DB }} 'PRAGMA user_version'
  sqlite3 {{ DB }} '.headers on' 'PRAGMA table_info(on_disk)'
  sqlite3 {{ DB }} '.headers on' 'PRAGMA table_info(on_camera)'

values DB="${RAWDB_DB}":
  sqlite3 {{ DB }} '.headers on' 'SELECT name, path, size, date FROM on_disk LIMIT 10'
  sqlite3 {{ DB }} '.headers on' 'SELECT name, path, size, date, saved FROM on_camera LIMIT 10'

sample DB="${RAWDB_DB}":
  sqlite3 {{ DB }} "insert into on_camera(name, path, size, checksum, saved) values ('bad.jpg', '', 0, x'', 0)"

run *ARGS:
  cargo run -- {{ ARGS }}
