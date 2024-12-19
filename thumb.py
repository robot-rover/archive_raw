import pathlib
import sys
import rawpy
import os
import glob
import pyexiv2
import argparse

parser = argparse.ArgumentParser(description='Extract JPEG and EXIF from a raw to a new file')
in_group = parser.add_mutually_exclusive_group(required=True)
in_group.add_argument('-d', '--dir', type=pathlib.Path)
in_group.add_argument('-i', '--images', type=pathlib.Path, nargs="+")
parser.add_argument('-o', '--out', type=pathlib.Path)

args = parser.parse_args()

out_dir = args.out
if out_dir is None:
    if args.dir is not None:
        out_dir = args.dir.joinpath('converted')
    else:
        out_dir = pathlib.Path('.')
out_dir.mkdir(exist_ok=True)
print('Writing to', out_dir)

if args.dir is not None:
    assert args.dir.exists, "Supply an existing file or directory"
    images = [p for p in args.dir.iterdir() if p.is_file() and p.suffix.lower().startswith('.cr')]
    print(f'Extracting {len(images)} images')
else:
    images = args.images
    for image in images:
        assert image.exists(), f"{image} does not exist"
        assert image.suffix == '.CR2', f"{image}: Only CR2 are supported"

for image in images:
    name = image.stem
    out_path = out_dir.joinpath(f'{name}_thumb.jpg')
    print(image, '->', out_path)

    with rawpy.imread(str(image)) as raw:
        thumb = raw.extract_thumb()
    assert thumb.format == rawpy.ThumbFormat.JPEG
    with open(out_path, 'wb') as f:
        f.write(thumb.data)
    meta_raw = pyexiv2.ImageMetadata(str(image))
    meta_raw.read()
    meta_im = pyexiv2.ImageMetadata(str(out_path))
    meta_im.read()
    meta_raw.copy(meta_im)
    meta_raw.write(preserve_timestamps=True)

