/* LovePlus+ voice decoder: port of the game's G.722.1 / G.722.1C fixed-point decoder (G7221_* @0x2000c64).
 * Lookup tables are read from the user's own decompressed ARM9 image (work/arm9_dec.bin) at the game's addresses,
 * so no proprietary tables are stored in this repository.
 *
 * Build: clang -O2 -o siren14dec tools/siren14dec.c
 * Usage: siren14dec TABLES VTB SPP CLIP_INDEX OUT.wav [bitrate=16000 bandwidth=14000]
 *   TABLES = content/tables/g7221.pack (LPTB pack written by tools/lp_extract.py) or a raw decompressed ARM9 image.
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef int16_t s16; typedef uint16_t u16; typedef int32_t s32; typedef uint32_t u32;
static uint8_t *IMG; static size_t IMGLEN;
#define BASE 0x2000000u
static const s16 *S16(u32 a) { return (const s16 *)(IMG + (a - BASE)); }
static u32 U32(u32 a) { u32 v; memcpy(&v, IMG + (a - BASE), 4); return v; }

static s32 sat(s32 v) { return v > 32767 ? 32767 : (v < -32768 ? -32768 : v); }

/* ---- bit reader (state struct: left, word, next, bits_left) ---- */
typedef struct { u16 left; u16 word; const u16 *next; s16 bits_left; } BR;
static int nextbit(BR *b) {
    u16 w = b->word; s32 l = (s32)b->left - 1;
    if (l < 0) { w = *b->next++; b->word = w; l = 15; }
    b->left = (u16)l;
    return (w >> (l & 0xff)) & 1;
}

static const s16 *bits_tab(void) { return S16(0x2113c10); }

static void calc_initial_categories(s16 *cat, const s16 *pidx, int nreg, int offset) {
    for (int i = 0; i < nreg; i++) { s32 c = (offset - pidx[i]) >> 1; if (c < 0) c = 0; if (c > 7) c = 7; cat[i] = (s16)c; }
}
static int calc_category_offset(const s16 *pidx, int nreg, int avail, s16 *scratch) {
    u32 delta = 0x20; s32 offset = -0x20;
    do {
        for (int i = 0; i < nreg; i++) { s32 c = (s32)(offset + delta - pidx[i]) >> 1; if (c < 0) c = 0; if (c > 7) c = 7; scratch[i] = (s16)c; }
        u32 total = 0; for (int i = 0; i < nreg; i++) total += bits_tab()[scratch[i]];
        if ((u32)(avail - 0x20) <= total) offset += (s32)delta;
        delta >>= 1;
    } while (delta);
    return offset;
}
static void rebalance(s16 *cat, s32 *balances, const s16 *pidx, int avail, int nreg, int ctrl, int offset) {
    s16 maxc[28], minc[28]; s32 rc[64]; const s16 *bt = bits_tab();
    int total = 0; for (int i = 0; i < nreg; i++) total += bt[cat[i]];
    memcpy(minc, cat, nreg * 2); memcpy(maxc, cat, nreg * 2);
    int imax = ctrl, imin = ctrl, tmax = total, tmin = total, lastmax = 0, lastmin = 0;
    for (int k = 0; k < ctrl - 1; k++) {
        if (avail * 2 < tmax + tmin) {
            int best = -99;
            for (int j = nreg - 1; j >= 0; j--) if (maxc[j] < 7) { int v = (offset - pidx[j]) + maxc[j] * -2; if (best < v) { best = v; lastmax = j; } }
            rc[imax++] = lastmax;
            int c = maxc[lastmax]; s16 old = bt[c]; if (c < 7) c++; maxc[lastmax] = (s16)c; tmax = (tmax - old) + bt[c];
        } else {
            int best = 99;
            for (int j = 0; j < nreg; j++) if (minc[j] > 0) { int v = (offset - pidx[j]) + minc[j] * -2; if (v < best) { best = v; lastmin = j; } }
            imin--; rc[imin] = lastmin;
            int c = minc[lastmin]; s16 old = bt[c]; if (c > 0) c--; minc[lastmin] = (s16)c; tmin = (tmin - old) + bt[c];
        }
    }
    memcpy(cat, minc, nreg * 2);
    memcpy(balances, rc + imin, (ctrl - 1) * 4);
}
static void categorize(int avail, int nreg, int ctrl, const s16 *pidx, s16 *cat, s32 *bal) {
    int lim = (nreg == 14) ? 0x140 : 0x280;
    if (lim < avail) avail = lim + (((avail - lim) * 5) >> 3);
    s16 scratch[28];
    int off = calc_category_offset(pidx, nreg, avail, scratch);
    calc_initial_categories(cat, pidx, nreg, off);
    rebalance(cat, bal, pidx, avail, nreg, ctrl, off);
}

static void region_power_envelope(BR *b, int nreg, s32 *stddev, s16 *pidx, s32 *mag_shift) {
    s32 diff[28]; int v = 0;
    for (int i = 0; i < 5; i++) v = nextbit(b) + v * 2;
    diff[0] = v - 7; b->bits_left -= 5;
    for (int r = 1; r < nreg; r++) {
        const s16 *tree = S16(0x2115110 + r * 0x5c); int node = 0;
        do { int bit = nextbit(b); node = tree[bit + node * 2]; b->bits_left -= 1; } while (node > 0);
        diff[r] = -node;
    }
    pidx[0] = (s16)diff[0];
    for (int r = 1; r < nreg; r++) pidx[r] = (s16)(pidx[r - 1] + (s16)diff[r] - 12);
    const s16 *st = S16(0x2115b60);
    s32 suma = 0, maxp = 0;
    for (int r = 0; r < nreg; r++) { s32 idx = pidx[r] + 0x18; if (maxp < idx) maxp = idx; suma += st[idx]; }
    s32 sh = 9;
    for (;;) { if (sh < 0 || (suma - 8 < 0 && maxp - 0x1c < 1)) break; suma /= 2; maxp -= 2; sh--; }
    *mag_shift = sh;
    for (int r = 0; r < nreg; r++) stddev[r] = st[sh * 2 + 0x18 + pidx[r]];
}

static u32 next_random(u32 *r) {
    u32 v = r[0] + r[3]; if (v & 0x8000) v++;
    r[3] = r[2]; r[2] = r[1]; r[1] = r[0]; r[0] = v & 0xffff; return r[0];
}
static int unpack_index(int idx, s16 *out, int cat) {
    s16 a = S16(0x2115b50)[cat], bb = S16(0x2115b40)[cat]; int dim = S16(0x2115b30)[cat] - 1; int cnt = 0;
    for (int k = dim; k >= 0; k--) {
        int q = (idx * bb) >> 15; idx = idx - q * (a + 1); out[k] = (s16)idx; if (idx != 0) cnt++; idx = q;
    }
    return cnt;
}
static void decode_vq(BR *b, u32 *rnd, int nreg, const s32 *stddev, s16 *cat, s16 *coefs) {
    int err = 0; s32 fill[8] = {0, 0, 0, 0, 0, (s16)U32(0x20fbc3c), (s16)U32(0x20fbc40), (s16)U32(0x20fbc44)};
    u32 sign_bits = 0, sign_mask = 0;
    for (int r = 0; r < nreg; r++) {
        int c = cat[r]; s16 *p = coefs + r * 20; s32 sd = stddev[r];
        if (c < 7) {
            const s16 *tree = S16(U32(0x2113c20 + c * 4));
            int nvec = S16(0x2115b20)[c], vdim = S16(0x2115b30)[c]; const s16 *cent = S16(0x2113d74 + c * 0x20);
            for (int v = 0; v < nvec; v++) {
                int idx = 0;
                do { if (b->bits_left < 1) { err = 1; break; } int bit = nextbit(b); idx = tree[bit + idx * 2]; b->bits_left -= 1; } while (idx > 0);
                if (err) break;
                s16 dig[6]; int nz = unpack_index(-idx, dig, c);
                if (b->bits_left < nz) { err = 1; break; }
                if (nz) { sign_bits = 0; for (int k = 0; k < nz; k++) { sign_bits = nextbit(b) + sign_bits * 2; b->bits_left -= 1; } sign_mask = 1u << (nz - 1); }
                for (int k = 0; k < vdim; k++) {
                    s32 x = sd * cent[dig[k]]; x = (s32)((u32)x + ((u32)(x >> 11) >> 20)) >> 12;
                    if (x != 0) { if ((sign_bits & sign_mask) == 0) x = -x; sign_mask = (u32)((s32)sign_mask / 2); }
                    *p++ = (s16)x;
                }
            }
            if (err) { for (int j = r + 1; j < nreg; j++) cat[j] = 7; c = 7; p = coefs + r * 20; }
        }
        if (c == 5 || c == 6) {
            p = coefs + r * 20; s32 x = sd * fill[c]; s32 nz = (s32)((u32)x + ((u32)(x >> 14) >> 17)) >> 15; s32 neg = -nz;
            u32 rv = next_random(rnd);
            for (int k = 0; k < 10; k++, p++) if (*p == 0) { *p = (s16)((rv & 1) == 0 ? neg : nz); rv = (u32)((s32)rv / 2); }
            rv = next_random(rnd);
            for (int k = 0; k < 10; k++, p++) if (*p == 0) { *p = (s16)((rv & 1) == 0 ? neg : nz); rv = (u32)((s32)rv / 2); }
        }
        if (c == 7) {
            p = coefs + r * 20; s32 x = sd * fill[7]; s16 nz = (s16)((s32)((u32)x + ((u32)(x >> 14) >> 17)) >> 15);
            u32 rv = next_random(rnd);
            for (int k = 0; k < 10; k++) { *p++ = (rv & 1) ? nz : (s16)-nz; rv = (u32)((s32)rv / 2); }
            rv = next_random(rnd);
            for (int k = 0; k < 10; k++) { *p++ = (rv & 1) ? nz : (s16)-nz; rv = (u32)((s32)rv / 2); }
        }
    }
    if (err) b->bits_left -= 1;
}
static void test_errors(BR *b, int nreg, int ctrl, u32 *err, int cc, const s16 *pidx) {
    if (b->bits_left < 1) { if (b->bits_left < 0 && cc < ctrl - 1) *err |= 2; }
    else for (int i = 0; i < b->bits_left; i++) if (nextbit(b) == 0) *err = 1;
    for (int i = 0; i < nreg; i++) { int p = pidx[i]; if (p - 0x18 > 0 || p + 0xf < 0) *err |= 4; }
}

/* ---- inverse MLT ---- */
static void bf_sign(int N, const u32 *signs, const s16 *in, s16 *out) {
    s16 *p6 = out, *p8 = out + N - 1; int cnt = 2; u32 bits = 0;
    for (;;) {
        cnt -= 2; if (cnt == 0) { bits = *signs++; cnt = 0x20; }
        s16 a = in[0], bb = in[1]; in += 2;
        s32 v1 = ((a + bb) - ((s32)bits >> 31)) >> 1; *p6++ = (s16)sat(v1);
        u32 t = bits & 0x7fffffff; bits <<= 2;
        s32 v2 = (s32)((a + (s32)(t >> 30)) - bb) >> 1; *p8-- = (s16)sat(v2);
        if (!(p6 < p8)) break;
    }
}
static void bf_sumdiff(u32 N, u32 stage, const s16 *in, s16 *out) {
    int blocks = 1 << stage;
    do {
        s16 *p5 = out, *p7 = out + (N >> stage) - 1;
        for (;;) {
            s16 a = in[0], bb = in[1]; in += 2;
            *p5++ = (s16)sat((s32)a + bb); *p7-- = (s16)sat((s32)a - bb);
            if (!(p5 < p7)) break;
        }
        blocks--; out += (N >> stage);
    } while (blocks);
}
static void mat10(u32 groups, s16 *buf, s16 *tmp) {
    const s16 *m = S16(0x2113008); s16 *in = buf, *o = tmp;
    for (u32 g = 0; g < groups; g++, in += 10) for (int j = 0; j < 10; j++) {
        s32 s = 0; for (int k = 0; k < 10; k++) s += (s32)m[10 * k + j] * in[k];
        *o++ = (s16)sat(((s >> 14) + 1) >> 1);
    }
    memcpy(buf, tmp, groups * 10 * 2);
}
static void twiddle(const s16 *p1, const s16 *p2, s16 *p3, s16 *p4, const s16 *t) {
    do {
        s16 a1 = p1[0], a2 = p1[1]; p1 += 2; s16 b1 = p2[0], b2 = p2[1]; p2 += 2;
        s16 c5 = t[0], c6 = t[1], c7 = t[2], c8 = t[3]; t += 4;
        p4[-1] = (s16)sat(((((s32)b1 * c5 + (s32)c6 * a1) >> 13) + 1) >> 1);
        p3[0] = (s16)sat(((((s32)b1 * -(s32)c6 + (s32)c5 * a1) >> 13) + 1) >> 1);
        p3[1] = (s16)sat(((((s32)b2 * c8 + (s32)c7 * a2) >> 13) + 1) >> 1);
        p4[-2] = (s16)sat(((((s32)b2 * -(s32)c7 + (s32)c8 * a2) >> 13) + 1) >> 1);
        p3 += 2; p4 -= 2;
    } while (p3 < p4);
}
static void inverse_dct4(const s16 *coefs, s16 *out, int N) {
    static s16 A[640], B[640], F[640];
    int lg = (N == 0x140) ? 6 : 7;
    const u32 *signs = (const u32 *)(IMG + ((N == 0x140 ? 0x2112ea0u : 0x2112f18u) - BASE));
    bf_sign(N, signs, coefs, A);
    s16 *cur = A, *oth = B;
    for (int st = 1; st <= lg - 2; st++) { bf_sumdiff(N, st, cur, oth); s16 *t = cur; cur = oth; oth = t; }
    mat10(1u << (lg - 1), cur, F);
    u32 tp = 0x2112e60;
    for (int u = lg - 2; u >= 0; u--) {
        s16 *src = cur, *dst = (u == 0) ? out : oth; int blocks = 1 << u; u32 len = (u32)N >> u;
        const s16 *tab = S16(U32(tp));
        for (; blocks > 0; blocks--) { twiddle(src, src + (len >> 1), dst, dst + len, tab); dst += len; src += len; }
        tp += 4; s16 *t = cur; cur = oth; oth = t;
    }
    if (N == 0x140) { const s16 *tb = S16(0x2113210); for (int i = 0; i < 0x140; i++) out[i] = (s16)sat((s32)out[i] + tb[i]); }
}
static void window_ola(int N, s16 *out, const s16 *mid, s16 *hist) {
    const s16 *win = S16(N == 0x140 ? 0x2115be0u : 0x2115e60u); int half = N >> 1; const s16 *tmp = mid - half;
    for (int k = 0; k < half; k++) out[k] = (s16)sat(((((s32)hist[k] * win[N - 1 - k] + (s32)win[k] * mid[-1 - k]) >> 12) + 1) >> 1);
    for (int k = 0; k < half; k++) out[half + k] = (s16)sat(((((s32)hist[half - 1 - k] * -(s32)win[N - 1 - half - k] + (s32)win[half + k] * tmp[k]) >> 12) + 1) >> 1);
    for (int k = 0; k < half; k++) hist[k] = tmp[half + k];
}
typedef struct { u32 rnd[4]; s16 old_coefs[640]; s16 hist[320]; s32 old_mag; int nreg, N; } Dec;
static void dec_init(Dec *d, int bandwidth) {
    memset(d, 0, sizeof *d); for (int i = 0; i < 4; i++) d->rnd[i] = 1;
    if (bandwidth == 7000) { d->nreg = 14; d->N = 320; } else { d->nreg = 28; d->N = 640; }
}
static int dec_frame(Dec *d, const u16 *frame, int bits, s16 *out) {
    int nreg = d->nreg, N = d->N; s16 mlt[640]; memset(mlt, 0, sizeof mlt);
    BR b = { 0, frame[0], frame, (s16)bits };
    int ncoefs = (nreg == 14) ? 0x118 : 0x230, ctrl_bits = (nreg == 14) ? 4 : 5, ctrl = (nreg == 14) ? 0x10 : 0x20;
    s16 pidx[28], cat[28]; s32 bal[64], stddev[28], mag;
    region_power_envelope(&b, nreg, stddev, pidx, &mag);
    int cc = 0; for (int i = 0; i < ctrl_bits; i++) cc = nextbit(&b) + cc * 2;
    b.bits_left -= ctrl_bits;
    categorize(b.bits_left, nreg, ctrl, pidx, cat, bal);
    for (int k = 0; k < cc; k++) cat[bal[k]]++;
    decode_vq(&b, d->rnd, nreg, stddev, cat, mlt);
    u32 err = 0; test_errors(&b, nreg, ctrl, &err, cc, pidx);
    if (err == 0) { memcpy(d->old_coefs, mlt, ncoefs * 2); d->old_mag = mag; }
    else { memcpy(mlt, d->old_coefs, ncoefs * 2); memset(d->old_coefs, 0, ncoefs * 2); mag = d->old_mag; d->old_mag = 0; }
    memset(mlt + ncoefs, 0, (N - ncoefs) * 2);
    s16 tmp[640]; inverse_dct4(mlt, tmp, N);
    if (mag > 0) for (int i = 0; i < N; i++) tmp[i] = (s16)((s32)tmp[i] >> mag);
    else if (mag < 0) for (int i = 0; i < N; i++) tmp[i] = (s16)((s32)tmp[i] << -mag);
    window_ola(N, out, tmp + N / 2, d->hist);
    return (int)err;
}
static void le32(FILE *f, u32 v) { fwrite(&v, 4, 1, f); }
static void le16(FILE *f, u16 v) { fwrite(&v, 2, 1, f); }
int main(int argc, char **argv) {
    if (argc < 6) { fprintf(stderr, "usage: %s ARM9_IMAGE VTB SPP CLIP OUT.wav [bitrate bandwidth]\n", argv[0]); return 1; }
    int bitrate = argc > 6 ? atoi(argv[6]) : 16000, bw = argc > 7 ? atoi(argv[7]) : 14000;
    FILE *f = fopen(argv[1], "rb"); if (!f) { perror("tables"); return 1; }
    fseek(f, 0, SEEK_END); size_t flen = ftell(f); fseek(f, 0, SEEK_SET); uint8_t *raw = malloc(flen); fread(raw, 1, flen, f); fclose(f);
    if (flen > 8 && memcmp(raw, "LPTB", 4) == 0) {            /* table pack: (addr,len) ranges + data */
        u32 n; memcpy(&n, raw + 4, 4); u32 maxend = 0; size_t dp = 8 + (size_t)n * 8;
        for (u32 i = 0; i < n; i++) { u32 a, l; memcpy(&a, raw + 8 + i * 8, 4); memcpy(&l, raw + 12 + i * 8, 4); if (a + l > maxend) maxend = a + l; }
        IMGLEN = maxend - BASE; IMG = calloc(IMGLEN + 16, 1);
        for (u32 i = 0; i < n; i++) { u32 a, l; memcpy(&a, raw + 8 + i * 8, 4); memcpy(&l, raw + 12 + i * 8, 4); memcpy(IMG + (a - BASE), raw + dp, l); dp += l; }
        free(raw);
    } else { IMG = raw; IMGLEN = flen; }
    int clip = atoi(argv[4]);
    f = fopen(argv[2], "rb"); fseek(f, (long)clip * 8, SEEK_SET); u32 ent[2]; fread(ent, 4, 2, f); fclose(f);
    u32 off = ent[0] / 16, size = ent[1] / 16;
    f = fopen(argv[3], "rb"); fseek(f, off, SEEK_SET); uint8_t *buf = calloc(size + 16, 1); fread(buf, 1, size, f); fclose(f);
    int bits = bitrate / 50, bpf = bits >> 3; Dec d; dec_init(&d, bw);
    int frames = size / bpf, N = d.N; s16 *pcm = calloc((size_t)frames * N, 2); int errs = 0;
    for (int i = 0; i < frames; i++) {
        u16 fr[64]; memset(fr, 0, sizeof fr); memcpy(fr, buf + i * bpf, bpf);
        if (dec_frame(&d, fr, bits, pcm + (size_t)i * N)) errs++;
    }
    FILE *o = fopen(argv[5], "wb"); u32 dl = frames * N * 2;
    fwrite("RIFF", 1, 4, o); le32(o, 36 + dl); fwrite("WAVEfmt ", 1, 8, o); le32(o, 16); le16(o, 1); le16(o, 1);
    le32(o, bw == 7000 ? 16000 : 32000); le32(o, (bw == 7000 ? 16000 : 32000) * 2); le16(o, 2); le16(o, 16);
    fwrite("data", 1, 4, o); le32(o, dl); fwrite(pcm, 2, (size_t)frames * N, o); fclose(o);
    fprintf(stderr, "clip %d: %d frames, %d error frames, %d samples\n", clip, frames, errs, frames * N);
    return 0;
}
