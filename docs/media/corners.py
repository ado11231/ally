# Makes the magenta corners of the README recording see through, after ffmpeg has drawn
# them. ffmpeg stops storing only the changes between frames once a GIF has see through
# pixels, which made the file about 90 times larger. Marking the corner color clear in the
# first frame keeps the file small, and later frames never redraw the corners.
# Usage: python3 docs/media/corners.py docs/media/demo-keyed.gif docs/media/demo.gif
import sys

KEY = (255, 0, 255)

data = bytearray(open(sys.argv[1], "rb").read())
flags = data[10]

if not flags & 0x80:
    sys.exit("The GIF has no shared color table. Make it again with the ffmpeg line in demo.tape")
size = 2 << (flags & 7)
table = data[13 : 13 + 3 * size]
index = min(range(size), key=lambda i: sum((table[3 * i + c] - KEY[c]) ** 2 for c in range(3)))
position = 13 + 3 * size

while data[position] == 0x21 and data[position + 1] != 0xF9:
    position += 2
    while data[position]:
        position += data[position] + 1
    position += 1
    
if data[position : position + 2] != b"\x21\xf9" or data[position + 3] & 1:
    sys.exit("The first frame is not as ffmpeg writes it. Make it again with the ffmpeg line in demo.tape")
data[position + 3] |= 1
data[position + 6] = index
open(sys.argv[2], "wb").write(data)
