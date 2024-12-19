#!/usr/bin/env python3

import argparse
import hashlib
import os
import re
import subprocess
import sys
import tempfile
from textwrap import dedent
from typing import Dict, List, Optional, Tuple
from PIL import Image
from pathlib import Path
from datetime import datetime
import shutil
from appdirs import user_cache_dir
import json

from tqdm import tqdm

# structure: absolute dest path / rel image path / (file size, file hash)
CACHE_FILE = Path(user_cache_dir("archive_raw", "rr")) / "cache.json"

SKIP_EXTENSIONS = ('.xmp', '.pto')

EXIF_DATE_TIME_ORIG = 306

Data = Tuple[int, str]
DestCache = Dict[str, Data]
Lookup = Dict[str, List[Tuple[Data, Path]]]

def get_cache() -> Dict[str, DestCache]:
    CACHE_FILE.parent.mkdir(parents=True, exist_ok=True)
    if CACHE_FILE.exists():
        with open(CACHE_FILE, 'rt') as handle:
            cache = json.load(handle)
    else:
        cache = dict()
    return cache

def hash_file(path):
    block_size = 128*64
    md5 = hashlib.md5()
    with open(path, 'rb') as handle:
        while True:
            data = handle.read(block_size)
            if not data:
                break
            md5.update(data)
    return md5.digest()

def refresh_cache(root: Path, old_cache: DestCache) -> DestCache:
    new_cache = dict()
    for file in tqdm(list(root.rglob("*")), desc="Building cache"):
        if not file.is_file():
            continue
        if file.suffix in SKIP_EXTENSIONS:
            continue
        new_size = file.stat().st_size
        new_data = None
        rel_path = str(file.relative_to(root))
        if rel_path in old_cache:
            old_size, old_time = old_cache[rel_path]
            if old_size == new_size:
                new_data = new_size, old_time
        if new_data is None:
            new_data = get_file_data(file, file_size=new_size)
        if new_data is None:
            print(f'WARN: failed to parse file from dest: {file}')
        else:
            new_cache[rel_path] = new_data
    return new_cache

def build_lookup(dest_cache: DestCache) -> Lookup:
    lookup = dict()
    for rel_path, data in dest_cache.items():
        rel_path = Path(rel_path)
        if rel_path.name not in lookup:
            lookup[rel_path.name] = []
        lookup[rel_path.name].append((data, rel_path))
    return lookup

def check_in_lookup(lookup: Lookup, name: str, file_data: Data):
    if name not in lookup:
        return None

    matches = [(data, path) for data, path in lookup[name] if data == file_data]
    assert len(matches) < 2
    return matches[0] if len(matches) > 0 else None

def write_cache(cache):
    with open(CACHE_FILE, 'wt') as handle:
        json.dump(cache, handle)

def get_video_taken(path: Path):
    result = subprocess.check_output(
        f'ffprobe -v quiet -show_streams -select_streams v:0 -of json "{path}"', shell=True).decode()
    fields = json.loads(result)['streams'][0]
    date_taken = fields['tags']['creation_time']
    return datetime.fromisoformat(date_taken)

def get_photo_taken(path: Path):
    with Image.open(str(path)) as img:
        exif = img.getexif()
        if EXIF_DATE_TIME_ORIG in exif:
            return datetime.strptime(exif[EXIF_DATE_TIME_ORIG], "%Y:%m:%d %H:%M:%S")

def get_taken(path: Path) -> Optional[str]:
    dt = None
    if path.suffix in ('.CR2', '.jpg'):
        dt = get_photo_taken(path)
    elif path.suffix == '.MOV':
        dt = get_video_taken(path)
    else:
        print(f"Unsupported file type: {path}")
    if dt is not None:
        return dt.isoformat(timespec='seconds')

def get_file_data(path: Path, date_taken=None, file_size=None):
    if date_taken is None:
        date_taken = get_taken(path)
        if date_taken is None:
            return None

    if file_size is None:
        file_size = path.stat().st_size

    return (file_size, date_taken)

class MoveTask:
    SPLIT_PAT = re.compile(r'(\w+) ([^\s,"]+|"[^"]*") ([^\s,"]+|"[^"]*")(?: (.*))?$')
    WHITE_PAT = re.compile(r'\s')
    def __init__(self, action, src, dest, comment=None):
        self.action = action
        self.src = src
        self.dest = dest
        self.comment = comment if comment is not None else ""

    @classmethod
    def from_line(cls, line: str):
        m = cls.SPLIT_PAT.match(line)
        if m is None:
            raise ValueError(f'Invalid Line: "{line}"')
        return cls(*m.groups())

    @staticmethod
    def opt_quote(phrase: str):
        phrase = str(phrase)
        if MoveTask.WHITE_PAT.search(phrase) is None:
            return str(phrase)
        else:
            return f'"{phrase}"'

    @staticmethod
    def header(is_move: bool):
        operation = "move" if is_move else "copy"
        return dedent(f"""\
        # action source dest comment
        # actions:
        #   y: {operation}
        #   n: skip
        #   f: first, skip all previous
        #   c: cancel, skip all
        """)

    def __str__(self):
        s = ' '.join(self.opt_quote(field) for field in [self.action, self.src, self.dest])
        if self.comment is not None and len(self.comment) > 0:
            s += f' {self.comment}'
        return s

def interactive_tasks(tasks: List[MoveTask], is_move: bool, init_line: Optional[int]=None):
    with tempfile.NamedTemporaryFile(mode='wt', delete=False) as tf:
        file_path = tf.name
        print(MoveTask.header(is_move), file=tf)
        for task in tasks:
            print(task, file=tf)

    args = [os.environ.get("EDITOR", "vim"), str(file_path)]
    if init_line is not None and 'vi' in args[0]:
        args.append(f"+{init_line}")
    subprocess.run(args, check=True)

    with open(file_path, 'rt') as file:
        tasks = []
        for line in file.readlines():
            stripped = line.strip()
            if len(stripped) == 0 or stripped.startswith('#'):
                continue
            try:
                tasks.append(MoveTask.from_line(line))
            except ValueError:
                print(f'Warn: unrecognized line "{line}"')
        return tasks

def parse_args():
    parser = argparse.ArgumentParser()

    parser.add_argument("--dest", type=str, default="/mnt/data/Images/", help='The destination directory (default: /mnt/data/Images/)')
    parser.add_argument(metavar="SRC", dest='src', type=str, help="The source directory (should be called DCIM)")
    parser.add_argument("-m", dest='move', action='store_true', help="Should the files be moved (default: copy)")
    parser.add_argument("-n", dest='dry', action='store_true', help="Do a dry run (no filesystem changes)")
    parser.add_argument("-a", dest='all', action='store_true', help="Move all new files instead of just the most recent sequence")
    parser.add_argument("-v", dest='verbose', action='store_true', help='Show extra information')
    return parser.parse_args()

args = parse_args()

src = Path(args.src)
assert src.exists() and src.is_dir(), f'Source directory {src} must exist'

dest = Path(args.dest)
assert dest.exists() and dest.is_dir(), f'Destination directory {dest} must exist'

if args.verbose:
    print(f"Cache location: {CACHE_FILE}")
cache = get_cache()

if str(dest.absolute()) not in cache:
    dest_cache = dict()
else:
    dest_cache = cache[str(dest.absolute())]

dest_cache = refresh_cache(dest, dest_cache)
cache[str(dest.absolute())] = dest_cache
write_cache(cache)

dest_lookup = build_lookup(dest_cache)

tasks = []

for file in tqdm(list(src.rglob("*")), desc="Reading new images"):
    if file.is_dir():
        continue

    file_data = get_file_data(file)
    if file_data is None:
        print(f"Unrecognized file: {file}")
        continue
    file_size, date_taken = file_data
    file_data = (file_data[0], str(file_data[1]))

    date = str(datetime.fromisoformat(date_taken).date())
    target_dir = dest / date

    task = MoveTask('y', file.relative_to(src), f"{date}/{file.name}", "")

    lookup_match = check_in_lookup(dest_lookup, file.name, file_data)
    if lookup_match is not None:
        task.comment = f'in {lookup_match[1].parent}'
        task.action = 'n'

    if args.verbose:
        task.comment += f'size: {file_size}, time: {date_taken}'

    tasks.append(task)

init_line = 8
if not args.all:
    for idx, task in reversed(list(enumerate(tasks))):
        if task.action == 'n':
            if idx + 1 == len(tasks):
                tasks[-1].action = 'c'
                init_line += idx
            else:
                tasks[idx + 1].action = 'f'
            init_line += idx + 1
            break

tasks = interactive_tasks(tasks, args.move, init_line)

c_idx = None
f_idx = None

for idx, task in enumerate(tasks):
    if task.action == 'c':
        c_idx = idx
    elif task.action == 'f':
        f_idx = idx
    elif task.action not in ('y', 'n'):
        print("Warn: Unrecognized action for task:", str(task), sep='\n')

if c_idx is not None:
    print('found cancel action, exiting', str(tasks[c_idx]), sep='\n')
    sys.exit()

if f_idx is not None:
    print('found first action', str(tasks[f_idx]), sep='\n')
    tasks = tasks[f_idx:]

tasks = [task for task in tasks if task.action in ('y', 'f')]

with tqdm(tasks, disable=args.dry) as pbar:
    for task in pbar:
        pbar.set_description(task.dest)
        source = src / task.src
        assert source.exists(), f'{source} does not exist!'
        target = dest / task.dest
        target.parent.mkdir(exist_ok=True)
        if not args.dry:
            if args.move:
                shutil.move(source, target)
            else:
                shutil.copy2(source, target)
        else:
            print(str(task))
