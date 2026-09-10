"""Compare recorded layout contracts and desktop pixels; run with Pillow installed."""
import json
import sys
from pathlib import Path
from PIL import Image, ImageChops

root = Path(sys.argv[1] if len(sys.argv) > 1 else '/tmp/cccc-voice-workspace-modes')
before = {row['key']: row['state'] for row in json.loads((root / 'before.json').read_text())}
after = json.loads((root / 'after.json').read_text())
assert len(before) == len(after) == 30, 'Need 12 mobile + 18 desktop scenes'
for row in after:
    key, state = row['key'], row['state']
    assert not row['failures'], (key, row['failures'])
    if '-390-' in key:
        baseline = before[key]
        if key.startswith('instruction-') and baseline['activity']['height'] < 300:
            print('BASELINE would fail activity >=300px', key)
        if key.startswith('document-') and baseline['path']['width'] < 190:
            print('BASELINE would fail readable path >=190px', key)
        print('PASS mobile layout', key)
        continue
    assert state == before[key], ('desktop geometry or controls changed', key)
    # Paired captures use the saved pre-change CSS and current CSS on the same DOM.
    # This is only valid for this task's inert data-* TSX changes; original PNGs remain.
    a = Image.open(root / ('stable-before-' + key + '.png')).convert('RGB')
    b = Image.open(root / ('stable-after-' + key + '.png')).convert('RGB')
    assert a.size == b.size and ImageChops.difference(a, b).getbbox() is None, key
    print('PASS desktop pixels', key)
