#!/usr/bin/env python3
"""Decompress a BLZ (bottom-LZ) compressed static module. Usage: blz_decompress.py in.bin end_offset out.bin"""
import struct, sys

def blz_decode(data, end):
    word, extra = struct.unpack_from('<II', data, end - 8)
    hdr, enc = word >> 24, word & 0xFFFFFF
    out = bytearray(data[:end]) + bytearray(extra)
    pin, pout, stop = end - hdr, end + extra, end - enc
    while pin > stop:
        flags = out[pin - 1]; pin -= 1
        for _ in range(8):
            if pin <= stop: break
            if flags & 0x80:
                b1, b2 = out[pin - 1], out[pin - 2]; pin -= 2
                n, disp = (b1 >> 4) + 3, (((b1 & 0xF) << 8) | b2) + 3
                for _ in range(n):
                    pout -= 1; out[pout] = out[pout + disp]
            else:
                pin -= 1; pout -= 1; out[pout] = out[pin]
            flags = (flags << 1) & 0xFF
    assert pout == stop, (hex(pout), hex(stop))
    return bytes(out)

if __name__ == '__main__':
    d = open(sys.argv[1], 'rb').read()
    o = blz_decode(d, int(sys.argv[2], 0))
    open(sys.argv[3], 'wb').write(o)
    print(f'{len(d):#x} -> {len(o):#x}')
